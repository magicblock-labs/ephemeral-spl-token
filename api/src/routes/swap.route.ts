import type { Context } from "hono";
import { createRoute, OpenAPIHono, z } from "@hono/zod-openapi";
import {
  ComputeBudgetProgram,
  PublicKey,
  SystemProgram,
  TransactionInstruction,
  TransactionMessage,
  VersionedTransaction,
} from "@solana/web3.js";
import {
  deriveHydraCrankPda,
  deriveStashAta,
  deriveStashPda,
  schedulePrivateTransferIx,
} from "@magicblock-labs/ephemeral-rollups-sdk";

import { getEnv, type AppBindings } from "../env";
import { openApiDefaultHook } from "../lib/create-app";
import { ApiError, errorResponseSchema } from "../lib/errors";
import { jsonContent, jsonContentRequired } from "../lib/openapi";
import { ASSOCIATED_TOKEN_PROGRAM_ID, TOKEN_PROGRAM_ID } from "../lib/solana";
import { instructionVersionSchema, optionalBooleanSchema, optionalIntegerSchema, positiveSlippageSchema, prioritizationFeeLamportsSchema, privateTransferDiagnosticSchema, quoteRoutePlanSchema, swapModeSchema, unsignedIntegerStringSchema } from "../schema";
import {
  getCachedAddressLookupTables,
  type AddressLookupTable,
} from "../lib/rpc-cache";

const DEFAULT_FALLBACK_VALIDATOR = new PublicKey(
  "MAS1Dt9qreoRMQ14YQuhg8UTZMMzDdKhmkZMECCzk57",
);

// Extra compute units to cover the appended
// `schedule_private_transfer` ix. Measured conservatively — the ix does
// ~10 `create_program_address` derivations, one system Transfer, one Hydra
// Create CPI (which itself creates an account), and one post-top-up
// Transfer (~30k CU in practice; 40k leaves comfortable headroom).
const SCHEDULE_IX_CU_BUDGET = 40_000;
// ComputeBudget instruction layout: 1-byte discriminator + 4-byte u32.
const COMPUTE_BUDGET_SET_UNIT_LIMIT_DISC = 0x02;
const COMPUTE_BUDGET_SET_UNIT_LIMIT_IX_LEN = 5;
const COMPUTE_UNIT_LIMIT_MAX = 1_400_000;
const PRIVATE_TRANSFER_MAX_DELAY_MS_LIMIT = 10n * 60n * 1000n;
const PRIVATE_SWAP_MAX_SPLIT = 14;
const SOLANA_WIRE_TRANSACTION_SIZE_LIMIT = 1232;
const PRIVATE_SWAP_DEFAULT_MAX_ACCOUNTS = 39;
const PRIVATE_SWAP_MIN_MAX_ACCOUNTS = 1;

class PrivateSwapUpstreamError extends Error {
  causeValue?: unknown;

  constructor(message: string, causeValue?: unknown) {
    super(message);
    this.name = "PrivateSwapUpstreamError";
    this.causeValue = causeValue;
  }
}

class PrivateSwapTooLargeError extends Error {
  txLength?: number;
  causeValue?: unknown;

  constructor(message: string, txLength?: number, causeValue?: unknown) {
    super(message);
    this.name = "PrivateSwapTooLargeError";
    this.txLength = txLength;
    this.causeValue = causeValue;
  }
}

const tags = ["Swap"];
const USDC_TO_USDT_QUOTE_EXAMPLE: QuoteResponse = {
  inputMint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
  inAmount: "1000000",
  outputMint: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
  outAmount: "999519",
  otherAmountThreshold: "994522",
  swapMode: "ExactIn",
  slippageBps: 50,
  platformFee: null,
  priceImpactPct: "0",
  routePlan: [
    {
      swapInfo: {
        ammKey: "DB3sUCP2H4icbeKmK6yb6nUxU5ogbcRHtGuq7W2RoRwW",
        label: "HumidiFi",
        inputMint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
        outputMint: "So11111111111111111111111111111111111111112",
        inAmount: "1000000",
        outAmount: "11687625",
        updateContextSlot: "414697032",
      },
      percent: 100,
      bps: null,
    },
    {
      swapInfo: {
        ammKey: "4rJggoVMajEUtipev1XhSMjESYk8Zibz6CDHPtUe1mem",
        label: "GoonFi V2",
        inputMint: "So11111111111111111111111111111111111111112",
        outputMint: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
        inAmount: "11687625",
        outAmount: "999519",
        updateContextSlot: "414697032",
      },
      percent: 100,
      bps: null,
    },
  ],
  contextSlot: 414697033,
  timeTaken: 0.001218002,
  swapUsdValue: "1",
  mostReliableAmmsQuoteReport: {
    info: {
      "BZtgQEyS6eXUXicYPHecYQ7PybqodXQMvkjUbP4R8mUU": "999357",
      "Czfq3xZZDmsdGdUyrNLtRhGc47cXcZtLG4crryfu44zE": "11681475",
      "8sjV1AqBFvFuADBCQHhotaRq5DFFYSjjg1jMyVWMqXvZ": "999504",
    },
  },
  longtailMarketQuoteReport: null,
  useIncurredSlippageForQuoting: null,
  useRewards: null,
  otherRoutePlans: null,
  loadedLongtailToken: false,
  instructionVersion: null,
} as const;

const SWAP_REQUEST_EXAMPLE: SwapRequest = {
  userPublicKey: "3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE",
  quoteResponse: USDC_TO_USDT_QUOTE_EXAMPLE,
} as const;

const SWAP_REQUEST_PRIVATE_EXAMPLE: SwapRequest = {
  userPublicKey: "3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE",
  quoteResponse: USDC_TO_USDT_QUOTE_EXAMPLE,
  visibility: "private",
  destination: "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
  minDelayMs: "0",
  maxDelayMs: "0",
  split: 1,
} as const;

const SWAP_RESPONSE_PUBLIC_EXAMPLE: SwapResponse = {
  swapTransaction: "AQABA...base64...",
  lastValidBlockHeight: 318_120_000,
} as const;

const SWAP_RESPONSE_PRIVATE_EXAMPLE: SwapResponse = {
  swapTransaction: "AQABA...base64 with appended ATA-create + schedule_private_transfer...",
  lastValidBlockHeight: 318_120_000,
  privateTransfer: {
    stashAta: "9mZP3xKHvVqXd3xtk72nSeEwJjmsgqmXvUDfwNA9BM4K",
    hydraCrankPda: "HyDraCrNkPdA11111111111111111111111111111111",
    shuttleId: 2_147_483_647,
  },
} as const;

const quoteQuerySchema = z.object({
  inputMint: z.string().openapi({
    example: "So11111111111111111111111111111111111111112",
    description: "Input token mint address.",
  }),
  outputMint: z.string().openapi({
    example: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    description: "Output token mint address.",
  }),
  amount: unsignedIntegerStringSchema.openapi({
    example: "1000000",
    description: "Raw amount to swap before decimals are applied.",
  }),
  slippageBps: optionalIntegerSchema.openapi({
    example: 50,
    description: "Slippage threshold in basis points.",
  }),
  swapMode: swapModeSchema.optional().openapi({
    example: "ExactIn",
    description: "Use `ExactIn` for fixed input amount or `ExactOut` for fixed output amount.",
  }),
  dexes: z.string().optional().openapi({
    example: "Raydium,Orca+V2",
    description: "Optional comma-separated list of DEX labels to include.",
  }),
  excludeDexes: z.string().optional().openapi({
    example: "Meteora+DLMM",
    description: "Optional comma-separated list of DEX labels to exclude.",
  }),
  restrictIntermediateTokens: optionalBooleanSchema.openapi({
    example: true,
    description: "Restrict intermediate tokens to a more stable set.",
  }),
  onlyDirectRoutes: optionalBooleanSchema.openapi({
    example: false,
    description: "Limit routing to a single hop.",
  }),
  asLegacyTransaction: optionalBooleanSchema.openapi({
    example: false,
    description: "Request a legacy transaction-compatible route.",
  }),
  platformFeeBps: optionalIntegerSchema.openapi({
    example: 20,
    description: "Optional platform fee in basis points.",
  }),
  maxAccounts: optionalIntegerSchema.openapi({
    example: 64,
    description: "Approximate maximum account budget for the route.",
  }),
  instructionVersion: instructionVersionSchema.optional().openapi({
    example: "V1",
    description: "Instruction format to target.",
  }),
  dynamicSlippage: optionalBooleanSchema.openapi({
    example: false,
    description: "Keep for compatibility with upstream quote parameters.",
  }),
  forJitoBundle: optionalBooleanSchema.openapi({
    example: false,
    description: "Exclude routes that are incompatible with Jito bundles.",
  }),
  supportDynamicIntermediateTokens: optionalBooleanSchema.openapi({
    example: false,
    description: "Allow dynamic selection of intermediate tokens.",
  }),
}).openapi("SwapQuoteQuery");
export type QuoteQuery = z.infer<typeof quoteQuerySchema>;

const quoteResponseSchema = z.object({
  inputMint: z.string(),
  inAmount: unsignedIntegerStringSchema,
  outputMint: z.string(),
  outAmount: unsignedIntegerStringSchema,
  otherAmountThreshold: unsignedIntegerStringSchema,
  swapMode: swapModeSchema,
  slippageBps: z.number().int().nonnegative(),
  priceImpactPct: z.string(),
  routePlan: z.array(quoteRoutePlanSchema),
  instructionVersion: instructionVersionSchema.nullable().optional(),
  platformFee: z.object({
    amount: unsignedIntegerStringSchema,
    feeBps: z.number().int().nonnegative(),
  }).passthrough().nullable().optional(),
  contextSlot: z.number().int().nonnegative().optional(),
  timeTaken: z.number().optional(),
  additionalIntermediateTokens: z.array(z.string()).nullable().optional(),
}).passthrough().openapi("SwapQuoteResponse");
export type QuoteResponse = z.infer<typeof quoteResponseSchema>;

const swapRequestSchema = z.object({
  userPublicKey: z.string().openapi({
    example: "3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE",
    description: "Public key of the wallet that will sign the swap transaction.",
  }),
  quoteResponse: quoteResponseSchema,
  payer: z.string().optional().openapi({
    example: "3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE",
    description: "Optional fee payer for transaction fees and rent.",
  }),
  wrapAndUnwrapSol: z.boolean().optional().openapi({
    example: true,
    description: "Automatically wrap and unwrap native SOL when needed.",
  }),
  useSharedAccounts: z.boolean().optional().openapi({
    example: true,
    description: "Allow shared accounts for intermediate routing state.",
  }),
  feeAccount: z.string().optional().openapi({
    example: "6QxLzE2KfM1NAB7sTzUPk4cNsA6V9fB3eYq9s6d9r9hP",
    description: "Optional initialized token account used to collect platform fees.",
  }),
  trackingAccount: z.string().optional().openapi({
    example: "Bt9oNR5cCtnfuMmXgWELd6q5i974PdEMQDUE55nBC57L",
    description: "Optional public key used for downstream transaction tracking.",
  }),
  prioritizationFeeLamports: prioritizationFeeLamportsSchema.optional().openapi({
    description: "Optional priority fee configuration or fixed lamport amount.",
  }),
  asLegacyTransaction: z.boolean().optional().openapi({
    example: false,
    description: "Build a legacy transaction instead of a versioned one.",
  }),
  destinationTokenAccount: z.string().optional().openapi({
    example: "6QxLzE2KfM1NAB7sTzUPk4cNsA6V9fB3eYq9s6d9r9hP",
    description: "Optional destination token account for the output mint.",
  }),
  nativeDestinationAccount: z.string().optional().openapi({
    example: "Bt9oNR5cCtnfuMmXgWELd6q5i974PdEMQDUE55nBC57L",
    description: "Optional destination account for native SOL output.",
  }),
  dynamicComputeUnitLimit: z.boolean().optional().openapi({
    example: false,
    description: "Estimate compute usage and set the compute unit limit automatically.",
  }),
  skipUserAccountsRpcCalls: z.boolean().optional().openapi({
    example: false,
    description: "Skip extra RPC checks for required user accounts.",
  }),
  dynamicSlippage: z.boolean().optional().openapi({
    example: false,
    description: "Let the upstream swap builder overwrite slippage on the transaction.",
  }),
  computeUnitPriceMicroLamports: z.number().int().nonnegative().optional().openapi({
    example: 1000,
    description: "Optional exact compute unit price in micro-lamports.",
  }),
  blockhashSlotsToExpiry: z.number().int().nonnegative().optional().openapi({
    example: 10,
    description: "Optional transaction expiry window in slots.",
  }),
  positiveSlippage: positiveSlippageSchema.optional().openapi({
    description: "Optional positive slippage collection settings.",
  }),
  visibility: z.enum(["public", "private"]).optional().openapi({
    example: "public",
    description: "Public swap (default) proxies Jupiter/Metis as-is. `private` forces Jupiter's output into a program-owned stash ATA and appends a scheduled private transfer.",
  }),
  destination: z.string().optional().openapi({
    example: "3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE",
    description: "Final private-transfer recipient (wallet pubkey). Required when visibility=private.",
  }),
  minDelayMs: unsignedIntegerStringSchema.optional().openapi({
    example: "0",
    description: "Earliest (ms) the queued transfer may settle. Required when visibility=private.",
  }),
  maxDelayMs: unsignedIntegerStringSchema.optional().openapi({
    example: "60000",
    description: "Latest (ms) the queued transfer may settle. Required when visibility=private.",
  }),
  split: z.number().int().positive().optional().openapi({
    example: 1,
    description: "Number of queue entries to split the transfer across. Required when visibility=private. Must be between 1 and 14.",
  }),
  clientRefId: unsignedIntegerStringSchema.optional().openapi({
    example: "0",
    description: "Optional u64 client correlation id attached to each queued split.",
  }),
  validator: z.string().optional().openapi({
    example: "MAS1Dt9qreoRMQ14YQuhg8UTZMMzDdKhmkZMECCzk57",
    description: "Optional validator pubkey for the transfer-queue PDA. Defaults to the well-known MagicBlock validator.",
  }),
}).passthrough().openapi("SwapRequest");
export type SwapRequest = z.infer<typeof swapRequestSchema>;

const swapResponseSchema = z.object({
  swapTransaction: z.string(),
  lastValidBlockHeight: z.number().int().nonnegative().optional(),
  prioritizationFeeLamports: z.number().int().nonnegative().optional(),
  privateTransfer: privateTransferDiagnosticSchema.optional().openapi({
    description: "Present when visibility=private. Diagnostic metadata about the appended schedule_private_transfer instruction.",
  }),
}).passthrough().openapi("SwapResponse");
export type SwapResponse = z.infer<typeof swapResponseSchema>;

const quoteRoute = createRoute({
  path: "/v1/swap/quote",
  method: "get",
  tags,
  description: "Get a swap quote.",
  request: {
    query: quoteQuerySchema,
  },
  responses: {
    200: jsonContent(
      quoteResponseSchema,
      "Swap quote",
      USDC_TO_USDT_QUOTE_EXAMPLE,
    ),
    500: jsonContent(errorResponseSchema, "Configuration error"),
    502: jsonContent(errorResponseSchema, "Upstream error"),
  },
});

const swapRoute = createRoute({
  path: "/v1/swap/swap",
  method: "post",
  tags,
  description: [
    "Build an unsigned swap transaction from a quote.",
    "",
    "Supports two visibility modes:",
    "",
    "- **`visibility: \"public\"`** (default) — pure pass-through to the Jupiter/Metis",
    "  upstream. The returned transaction is whatever the upstream produces.",
    "- **`visibility: \"private\"`** — the server forces Jupiter's output into a",
    "  program-owned stash ATA (deterministically derived from `(userPublicKey,",
    "  quoteResponse.outputMint)`), prepends an idempotent ATA-create, and appends a",
    "  `schedule_private_transfer` instruction that registers a one-shot Hydra",
    "  crank. When the crank fires, it self-CPIs into the on-chain private-transfer",
    "  flow to deliver the swapped tokens to `destination` with the requested",
    "  delay/split policy. The returned transaction is a v0 `VersionedTransaction`",
    "  that is still unsigned — the client signs and submits.",
    "",
    "When `visibility = \"private\"`, the fields `destination`, `minDelayMs`,",
    "`maxDelayMs`, and `split` are **required**. `clientRefId` and `validator` are",
    "optional. Explicitly setting `destinationTokenAccount` to anything other than",
    "the server-derived stash ATA returns 400.",
  ].join("\n"),
  request: {
    body: jsonContentRequired(swapRequestSchema, "Swap request", undefined, {
      // `private` first so Scalar renders it as the default Request Body —
      // it's the richer payload that surfaces every new field.
      private: {
        summary: "Private swap + scheduled transfer",
        description:
          "Forces Jupiter's output into the program-owned stash ATA and appends schedule_private_transfer.",
        value: SWAP_REQUEST_PRIVATE_EXAMPLE,
      },
      public: {
        summary: "Public swap (default behaviour, upstream proxy)",
        value: SWAP_REQUEST_EXAMPLE,
      },
    }),
  },
  responses: {
    200: jsonContent(swapResponseSchema, "Swap transaction", undefined, {
      private: {
        summary: "Response for visibility=private (with diagnostic block)",
        value: SWAP_RESPONSE_PRIVATE_EXAMPLE,
      },
      public: {
        summary: "Response for visibility=public",
        value: SWAP_RESPONSE_PUBLIC_EXAMPLE,
      },
    }),
    400: jsonContent(
      errorResponseSchema,
      "Invalid request (missing or conflicting private-transfer fields)",
    ),
    500: jsonContent(errorResponseSchema, "Configuration error"),
    502: jsonContent(errorResponseSchema, "Upstream error"),
  },
});

const app = new OpenAPIHono<{ Bindings: AppBindings }>({
  defaultHook: openApiDefaultHook,
});

function getMetisSwapApiUrl(bindings: AppBindings) {
  const env = getEnv(bindings);
  if (!env.METIS_SWAP_API_URL) {
    throw new ApiError(
      500,
      "CONFIG_ERROR",
      "Missing worker environment variable `METIS_SWAP_API_URL`",
      {
        hint: "Set `METIS_SWAP_API_URL` to the configured swap upstream base URL.",
      },
    );
  }

  return env.METIS_SWAP_API_URL.replace(/\/+$/, "");
}

async function proxyGet(
  c: Context<{ Bindings: AppBindings }>,
  upstreamPath: string,
) {
  const targetUrl = new URL(`${getMetisSwapApiUrl(c.env)}/${upstreamPath}`);
  targetUrl.search = new URL(c.req.url).search;

  const headers = new Headers();
  const accept = c.req.header("accept");

  if (accept) {
    headers.set("accept", accept);
  }

  try {
    const upstreamResponse = await fetch(targetUrl, {
      method: "GET",
      headers,
    });

    return new Response(upstreamResponse.body, {
      status: upstreamResponse.status,
      headers: upstreamResponse.headers,
    });
  } catch (error) {
    throw new ApiError(
      502,
      "SWAP_UPSTREAM_ERROR",
      "Failed to reach the swap upstream",
      {
        message: error instanceof Error ? error.message : String(error),
      },
    );
  }
}

async function proxyPost(
  c: Context<{ Bindings: AppBindings }>,
  upstreamPath: string,
  body: unknown,
) {
  const targetUrl = new URL(`${getMetisSwapApiUrl(c.env)}/${upstreamPath}`);

  const headers = new Headers({
    "content-type": "application/json",
  });
  const accept = c.req.header("accept");

  if (accept) {
    headers.set("accept", accept);
  }

  try {
    const upstreamResponse = await fetch(targetUrl, {
      method: "POST",
      headers,
      body: JSON.stringify(body),
    });

    return new Response(upstreamResponse.body, {
      status: upstreamResponse.status,
      headers: upstreamResponse.headers,
    });
  } catch (error) {
    throw new ApiError(
      502,
      "SWAP_UPSTREAM_ERROR",
      "Failed to reach the swap upstream",
      {
        message: error instanceof Error ? error.message : String(error),
      },
    );
  }
}

app.openapi(quoteRoute, ((c: Context<{ Bindings: AppBindings }>) =>
  proxyGet(c, "quote")) as any);
app.openapi(swapRoute, (async (c: Context<{ Bindings: AppBindings }>) => {
  const request = c.req as typeof c.req & {
    valid: (target: "json") => SwapRequest;
  };
  const body = request.valid("json");

  if (body.visibility !== "private") {
    return proxyPost(c, "swap", body);
  }

  return handlePrivateSwap(c, body);
}) as any);

// ---------------------------------------------------------------------------
// Private swap: Jupiter → stash ATA → schedule_private_transfer
// ---------------------------------------------------------------------------

async function handlePrivateSwap(
  c: Context<{ Bindings: AppBindings }>,
  body: SwapRequest,
): Promise<Response> {
  const { destination, minDelayMs, maxDelayMs, split, clientRefId, validator }
    = body;

  if (
    !destination
    || minDelayMs === undefined
    || maxDelayMs === undefined
    || split === undefined
  ) {
    throw new ApiError(
      400,
      "INVALID_REQUEST",
      "destination, minDelayMs, maxDelayMs, and split are required when visibility=private",
    );
  }

  let userPubkey: PublicKey;
  let payerPubkey: PublicKey;
  let mintPubkey: PublicKey;
  let destinationPubkey: PublicKey;
  let validatorPubkey: PublicKey;
  try {
    userPubkey = new PublicKey(body.userPublicKey);
    payerPubkey = body.payer ? new PublicKey(body.payer) : userPubkey;
    mintPubkey = new PublicKey(body.quoteResponse.outputMint);
    destinationPubkey = new PublicKey(destination);
    validatorPubkey = validator
      ? new PublicKey(validator)
      : DEFAULT_FALLBACK_VALIDATOR;
  } catch (error) {
    throw new ApiError(400, "INVALID_REQUEST", "Invalid public key", {
      message: error instanceof Error ? error.message : String(error),
    });
  }

  const [stashPda] = deriveStashPda(userPubkey, mintPubkey);
  const [stashAta] = deriveStashAta(userPubkey, mintPubkey);

  if (body.destinationTokenAccount) {
    throw new ApiError(
      400,
      "INVALID_REQUEST",
      "destinationTokenAccount is not supported when visibility=private",
    );
  }

  if (body.asLegacyTransaction) {
    throw new ApiError(
      400,
      "INVALID_REQUEST",
      "asLegacyTransaction is not supported when visibility=private",
    );
  }

  if (body.nativeDestinationAccount) {
    throw new ApiError(
      400,
      "INVALID_REQUEST",
      "nativeDestinationAccount is not supported when visibility=private",
    );
  }

  const minDelayBig = BigInt(minDelayMs);
  const maxDelayBig = BigInt(maxDelayMs);
  const clientRefIdBig
    = clientRefId !== undefined ? BigInt(clientRefId) : undefined;
  if (maxDelayBig < minDelayBig) {
    throw new ApiError(
      400,
      "INVALID_REQUEST",
      "maxDelayMs must be >= minDelayMs",
    );
  }

  if (maxDelayBig > PRIVATE_TRANSFER_MAX_DELAY_MS_LIMIT) {
    throw new ApiError(
      400,
      "INVALID_REQUEST",
      "maxDelayMs must be less than or equal to 600000",
    );
  }

  if (split > PRIVATE_SWAP_MAX_SPLIT) {
    throw new ApiError(
      400,
      "INVALID_REQUEST",
      "split must be an integer between 1 and 14 when visibility=private",
    );
  }

  const shuttleId = Math.floor(Math.random() * 0x1_0000_0000);
  const [hydraCrankPda] = deriveHydraCrankPda(stashPda, shuttleId);
  const env = getEnv(c.env);
  if (!env.BASE_RPC_URL) {
    throw new ApiError(
      500,
      "CONFIG_ERROR",
      "Missing worker environment variable `BASE_RPC_URL`",
    );
  }

  let metisJson: UpstreamSwapResponse | undefined;
  let rebuilt: string | undefined;
  try {
    const firstAttempt = await tryBuildPrivateSwapAttempt({
      bindings: c.env,
      baseRpcUrl: env.BASE_RPC_URL,
      body,
      quoteResponse: body.quoteResponse,
      payer: payerPubkey,
      mint: mintPubkey,
      stashPda,
      stashAta,
      destinationOwner: destinationPubkey,
      shuttleId,
      minDelayMs: minDelayBig,
      maxDelayMs: maxDelayBig,
      split,
      validator: validatorPubkey,
      clientRefId: clientRefIdBig,
    });

    if (firstAttempt.kind === "upstream_non_ok") {
      return new Response(firstAttempt.response.body, {
        status: firstAttempt.response.status,
        headers: firstAttempt.response.headers,
      });
    }

    if (firstAttempt.kind === "success") {
      metisJson = firstAttempt.metisJson;
      rebuilt = firstAttempt.rebuilt;
    } else {
      for (
        let maxAccounts = PRIVATE_SWAP_DEFAULT_MAX_ACCOUNTS;
        maxAccounts >= PRIVATE_SWAP_MIN_MAX_ACCOUNTS;
        maxAccounts -= 1
      ) {
        const requoted = await requotePrivateSwap(
          c.env,
          body.quoteResponse,
          maxAccounts,
        );
        if (!requoted) {
          continue;
        }

        const requoteAttempt = await tryBuildPrivateSwapAttempt({
          bindings: c.env,
          baseRpcUrl: env.BASE_RPC_URL,
          body,
          quoteResponse: requoted,
          payer: payerPubkey,
          mint: mintPubkey,
          stashPda,
          stashAta,
          destinationOwner: destinationPubkey,
          shuttleId,
          minDelayMs: minDelayBig,
          maxDelayMs: maxDelayBig,
          split,
          validator: validatorPubkey,
          clientRefId: clientRefIdBig,
        });

        if (
          requoteAttempt.kind === "upstream_non_ok"
          || requoteAttempt.kind === "too_large"
        ) {
          continue;
        }

        metisJson = requoteAttempt.metisJson;
        rebuilt = requoteAttempt.rebuilt;
        break;
      }
    }
  } catch (error) {
    if (error instanceof PrivateSwapUpstreamError) {
      throw new ApiError(
        502,
        "SWAP_UPSTREAM_ERROR",
        error.message,
        error.causeValue === undefined
          ? undefined
          : {
              message:
                error.causeValue instanceof Error
                  ? error.causeValue.message
                  : String(error.causeValue),
            },
      );
    }

    if (error instanceof SyntaxError) {
      throw new ApiError(
        502,
        "SWAP_UPSTREAM_ERROR",
        "Invalid JSON in upstream swap response",
        {
          message: error.message,
        },
      );
    }

    throw error;
  }

  if (!metisJson || !rebuilt) {
    throw new ApiError(
      502,
      "SWAP_UPSTREAM_ERROR",
      `Failed to fit private swap transaction within ${SOLANA_WIRE_TRANSACTION_SIZE_LIMIT} bytes`,
      {
        startingMaxAccounts: PRIVATE_SWAP_DEFAULT_MAX_ACCOUNTS,
      },
    );
  }

  const responseBody = {
    ...metisJson,
    swapTransaction: rebuilt,
    privateTransfer: {
      stashAta: stashAta.toBase58(),
      hydraCrankPda: hydraCrankPda.toBase58(),
      shuttleId,
    },
  };

  return c.json(responseBody);
}

function buildUpstreamSwapBody(
  body: SwapRequest,
  stashAta: PublicKey,
  quoteResponse: QuoteResponse = body.quoteResponse,
): Record<string, unknown> {
  // Strip the private-only fields before forwarding; override the
  // destination ATA; force v0 so we can splice instructions back in.
  const {
    visibility: _visibility,
    destination: _destination,
    minDelayMs: _minDelayMs,
    maxDelayMs: _maxDelayMs,
    split: _split,
    clientRefId: _clientRefId,
    validator: _validator,
    ...rest
  } = body;
  void _visibility;
  void _destination;
  void _minDelayMs;
  void _maxDelayMs;
  void _split;
  void _clientRefId;
  void _validator;
  return {
    ...rest,
    quoteResponse,
    destinationTokenAccount: stashAta.toBase58(),
    asLegacyTransaction: false,
  };
}

type UpstreamSwapResponse = {
  swapTransaction: string;
  lastValidBlockHeight?: number;
  prioritizationFeeLamports?: number;
  [k: string]: unknown;
};

type PrivateSwapAttemptResult
  = | {
    kind: "success";
    metisJson: UpstreamSwapResponse;
    rebuilt: string;
  }
  | {
    kind: "too_large";
  }
  | {
    kind: "upstream_non_ok";
    response: Response;
  };

type PrivateSwapAttemptInput = {
  bindings: AppBindings;
  baseRpcUrl: string;
  body: SwapRequest;
  quoteResponse: QuoteResponse;
  payer: PublicKey;
  mint: PublicKey;
  stashPda: PublicKey;
  stashAta: PublicKey;
  destinationOwner: PublicKey;
  shuttleId: number;
  minDelayMs: bigint;
  maxDelayMs: bigint;
  split: number;
  validator: PublicKey;
  clientRefId?: bigint;
};

async function fetchUpstreamSwap(
  bindings: AppBindings,
  body: unknown,
): Promise<Response> {
  try {
    return await fetch(`${getMetisSwapApiUrl(bindings)}/swap`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
  } catch (error) {
    throw new ApiError(
      502,
      "SWAP_UPSTREAM_ERROR",
      "Failed to reach the swap upstream",
      {
        message: error instanceof Error ? error.message : String(error),
      },
    );
  }
}

function buildRequoteParams(
  quoteResponse: QuoteResponse,
  maxAccounts: number,
): URLSearchParams {
  const params = new URLSearchParams({
    inputMint: quoteResponse.inputMint,
    outputMint: quoteResponse.outputMint,
    amount:
      quoteResponse.swapMode === "ExactOut"
        ? quoteResponse.outAmount
        : quoteResponse.inAmount,
    slippageBps: String(quoteResponse.slippageBps),
    swapMode: quoteResponse.swapMode,
    maxAccounts: String(maxAccounts),
  });

  if (quoteResponse.instructionVersion) {
    params.set("instructionVersion", quoteResponse.instructionVersion);
  }

  if (quoteResponse.platformFee?.feeBps !== undefined) {
    params.set("platformFeeBps", String(quoteResponse.platformFee.feeBps));
  }

  return params;
}

async function requotePrivateSwap(
  bindings: AppBindings,
  quoteResponse: QuoteResponse,
  maxAccounts: number,
): Promise<QuoteResponse | null> {
  const targetUrl = new URL(`${getMetisSwapApiUrl(bindings)}/quote`);
  targetUrl.search = buildRequoteParams(quoteResponse, maxAccounts).toString();

  let upstreamResponse: Response;
  try {
    upstreamResponse = await fetch(targetUrl, {
      method: "GET",
      headers: { accept: "application/json" },
    });
  } catch (error) {
    throw new ApiError(
      502,
      "SWAP_UPSTREAM_ERROR",
      "Failed to reach the swap upstream",
      {
        message: error instanceof Error ? error.message : String(error),
      },
    );
  }

  if (!upstreamResponse.ok) {
    return null;
  }

  let parsed: unknown;
  try {
    parsed = await upstreamResponse.json();
  } catch (error) {
    throw new PrivateSwapUpstreamError(
      "Invalid JSON in upstream quote response",
      error,
    );
  }

  const validated = quoteResponseSchema.safeParse(parsed);
  if (!validated.success) {
    throw new PrivateSwapUpstreamError("Invalid upstream quote response");
  }

  return validated.data;
}

async function tryBuildPrivateSwapAttempt(
  input: PrivateSwapAttemptInput,
): Promise<PrivateSwapAttemptResult> {
  const {
    bindings,
    baseRpcUrl,
    body,
    quoteResponse,
    payer,
    mint,
    stashPda,
    stashAta,
    destinationOwner,
    shuttleId,
    minDelayMs,
    maxDelayMs,
    split,
    validator,
    clientRefId,
  } = input;

  const metisResponse = await fetchUpstreamSwap(
    bindings,
    buildUpstreamSwapBody(body, stashAta, quoteResponse),
  );

  if (!metisResponse.ok) {
    return {
      kind: "upstream_non_ok",
      response: metisResponse,
    };
  }

  const parsed = (await metisResponse.json()) as {
    swapTransaction?: unknown;
    lastValidBlockHeight?: number;
    prioritizationFeeLamports?: number;
    [k: string]: unknown;
  };

  if (
    typeof parsed.swapTransaction !== "string"
    || parsed.swapTransaction.length === 0
  ) {
    throw new PrivateSwapUpstreamError(
      "Upstream swap response missing swapTransaction",
    );
  }

  const metisJson: UpstreamSwapResponse = {
    ...parsed,
    swapTransaction: parsed.swapTransaction,
  };

  try {
    const rebuilt = await rebuildSwapTransaction({
      baseRpcUrl,
      base64Tx: metisJson.swapTransaction,
      payer,
      mint,
      stashPda,
      stashAta,
      destinationOwner,
      shuttleId,
      minDelayMs,
      maxDelayMs,
      split,
      validator,
      clientRefId,
    });

    return {
      kind: "success",
      metisJson,
      rebuilt,
    };
  } catch (error) {
    if (error instanceof PrivateSwapTooLargeError) {
      return { kind: "too_large" };
    }
    throw error;
  }
}

type RebuildInput = {
  baseRpcUrl: string;
  base64Tx: string;
  payer: PublicKey;
  mint: PublicKey;
  stashPda: PublicKey;
  stashAta: PublicKey;
  destinationOwner: PublicKey;
  shuttleId: number;
  minDelayMs: bigint;
  maxDelayMs: bigint;
  split: number;
  validator: PublicKey;
  clientRefId?: bigint;
};

async function rebuildSwapTransaction(input: RebuildInput): Promise<string> {
  const {
    baseRpcUrl,
    base64Tx,
    payer,
    mint,
    stashPda,
    stashAta,
    destinationOwner,
    shuttleId,
    minDelayMs,
    maxDelayMs,
    split,
    validator,
    clientRefId,
  } = input;

  let txBytes: Uint8Array;
  try {
    txBytes = Uint8Array.from(atob(base64Tx), c => c.charCodeAt(0));
  } catch (error) {
    throw new PrivateSwapUpstreamError(
      "Invalid upstream swap transaction encoding",
      error,
    );
  }

  let versionedTx: VersionedTransaction;
  try {
    versionedTx = VersionedTransaction.deserialize(txBytes);
  } catch (error) {
    throw new PrivateSwapUpstreamError(
      "Invalid upstream swap transaction",
      error,
    );
  }

  const altKeys = versionedTx.message.addressTableLookups.map(
    l => l.accountKey,
  );
  let lookupTables: AddressLookupTable[];
  try {
    lookupTables = await getCachedAddressLookupTables(baseRpcUrl, altKeys);
  } catch (error) {
    throw new PrivateSwapUpstreamError(
      "Failed to fetch swap address lookup table",
      error,
    );
  }

  let message: TransactionMessage;
  try {
    message = TransactionMessage.decompile(versionedTx.message, {
      addressLookupTableAccounts: lookupTables,
    });
  } catch (error) {
    throw new PrivateSwapUpstreamError(
      "Failed to decompile upstream swap transaction",
      error,
    );
  }

  message.instructions.unshift(
    createAssociatedTokenAccountIdempotentInstruction(
      payer,
      stashAta,
      stashPda,
      mint,
    ),
  );

  // Jupiter/Metis sizes the tx's SetComputeUnitLimit for its own swap only.
  // Our appended ATA-create + schedule_private_transfer need extra CU, so
  // we bump an existing SetComputeUnitLimit in place when present and leave
  // the message unchanged when none exists.
  bumpComputeUnitLimit(message.instructions, SCHEDULE_IX_CU_BUDGET);
  message.instructions.push(
    schedulePrivateTransferIx(
      payer,
      mint,
      shuttleId,
      destinationOwner,
      minDelayMs,
      maxDelayMs,
      split,
      validator,
      undefined,
      clientRefId,
    ),
  );

  const rebuilt = new VersionedTransaction(
    message.compileToV0Message(lookupTables),
  );

  let serialized: Uint8Array;
  try {
    serialized = rebuilt.serialize();
  } catch (error) {
    if (error instanceof RangeError) {
      throw new PrivateSwapTooLargeError(
        `Rebuilt private swap transaction exceeds ${SOLANA_WIRE_TRANSACTION_SIZE_LIMIT} bytes`,
        undefined,
        error,
      );
    }
    throw error;
  }

  if (serialized.length > SOLANA_WIRE_TRANSACTION_SIZE_LIMIT) {
    throw new PrivateSwapTooLargeError(
      `Rebuilt private swap transaction exceeds ${SOLANA_WIRE_TRANSACTION_SIZE_LIMIT} bytes`,
      serialized.length,
    );
  }

  return Buffer.from(serialized).toString("base64");
}

function createAssociatedTokenAccountIdempotentInstruction(
  payer: PublicKey,
  ata: PublicKey,
  owner: PublicKey,
  mint: PublicKey,
): TransactionInstruction {
  // Discriminator 1 = CreateIdempotent.
  return new TransactionInstruction({
    programId: ASSOCIATED_TOKEN_PROGRAM_ID,
    keys: [
      { pubkey: payer, isSigner: true, isWritable: true },
      { pubkey: ata, isSigner: false, isWritable: true },
      { pubkey: owner, isSigner: false, isWritable: false },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
    ],
    data: Buffer.from([1]),
  });
}

/**
 * If the tx already carries a `SetComputeUnitLimit`, rewrite its u32
 * payload to `min(existing + extraCu, MAX)` in place so our appended ix
 * has enough CU. If it doesn't, leave the tx alone — Solana's default
 * budget (200k × ix_count, capped at 1.4M) already covers both Jupiter
 * and our appended ixs, so adding a limit would only risk capping below
 * that default. Solana rejects duplicate ComputeBudget ixs anyway.
 */
function bumpComputeUnitLimit(
  instructions: TransactionInstruction[],
  extraCu: number,
): void {
  for (const ix of instructions) {
    if (
      ix.programId.equals(ComputeBudgetProgram.programId)
      && ix.data.length === COMPUTE_BUDGET_SET_UNIT_LIMIT_IX_LEN
      && ix.data[0] === COMPUTE_BUDGET_SET_UNIT_LIMIT_DISC
    ) {
      const data = Buffer.from(ix.data);
      const existing = data.readUInt32LE(1);
      const bumped = Math.min(existing + extraCu, COMPUTE_UNIT_LIMIT_MAX);
      data.writeUInt32LE(bumped, 1);
      ix.data = data;
      return;
    }
  }
}

export default app;
