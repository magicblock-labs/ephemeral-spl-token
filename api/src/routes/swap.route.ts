import type { Context } from "hono";
import { createRoute, OpenAPIHono, z } from "@hono/zod-openapi";
import {
  AddressLookupTableAccount,
  Connection,
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

const DEFAULT_FALLBACK_VALIDATOR = new PublicKey(
  "MAS1Dt9qreoRMQ14YQuhg8UTZMMzDdKhmkZMECCzk57",
);
const TOKEN_PROGRAM_ID = new PublicKey(
  "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
);
const ASSOCIATED_TOKEN_PROGRAM_ID = new PublicKey(
  "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
);

const tags = ["Swap"];
const USDC_TO_USDT_QUOTE_EXAMPLE = {
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
      BZtgQEyS6eXUXicYPHecYQ7PybqodXQMvkjUbP4R8mUU: "999357",
      Czfq3xZZDmsdGdUyrNLtRhGc47cXcZtLG4crryfu44zE: "11681475",
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

const SWAP_REQUEST_EXAMPLE = {
  userPublicKey: "3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE",
  quoteResponse: USDC_TO_USDT_QUOTE_EXAMPLE,
} as const;

const unsignedIntegerStringSchema = z
  .string()
  .regex(/^\d+$/, "Must be an unsigned integer string");

const optionalBooleanQuerySchema = z.preprocess((value) => {
  if (typeof value === "string") {
    const normalized = value.trim().toLowerCase();
    if (normalized === "true") return true;
    if (normalized === "false") return false;
  }
  return value;
}, z.boolean().optional());

const optionalIntegerQuerySchema = z.preprocess((value) => {
  if (value === undefined || value === "") {
    return undefined;
  }

  if (typeof value === "string" && /^\d+$/.test(value)) {
    return Number(value);
  }

  return value;
}, z.number().int().nonnegative().optional());

const swapModeSchema = z.enum(["ExactIn", "ExactOut"]).openapi("SwapMode");
const instructionVersionSchema = z.enum(["V1", "V2"]).openapi("InstructionVersion");

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
  slippageBps: optionalIntegerQuerySchema.openapi({
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
  restrictIntermediateTokens: optionalBooleanQuerySchema.openapi({
    example: true,
    description: "Restrict intermediate tokens to a more stable set.",
  }),
  onlyDirectRoutes: optionalBooleanQuerySchema.openapi({
    example: false,
    description: "Limit routing to a single hop.",
  }),
  asLegacyTransaction: optionalBooleanQuerySchema.openapi({
    example: false,
    description: "Request a legacy transaction-compatible route.",
  }),
  platformFeeBps: optionalIntegerQuerySchema.openapi({
    example: 20,
    description: "Optional platform fee in basis points.",
  }),
  maxAccounts: optionalIntegerQuerySchema.openapi({
    example: 64,
    description: "Approximate maximum account budget for the route.",
  }),
  instructionVersion: instructionVersionSchema.optional().openapi({
    example: "V1",
    description: "Instruction format to target.",
  }),
  dynamicSlippage: optionalBooleanQuerySchema.openapi({
    example: false,
    description: "Keep for compatibility with upstream quote parameters.",
  }),
  forJitoBundle: optionalBooleanQuerySchema.openapi({
    example: false,
    description: "Exclude routes that are incompatible with Jito bundles.",
  }),
  supportDynamicIntermediateTokens: optionalBooleanQuerySchema.openapi({
    example: false,
    description: "Allow dynamic selection of intermediate tokens.",
  }),
}).openapi("SwapQuoteQuery");

const quoteRoutePlanSchema = z.object({
  swapInfo: z.object({
    ammKey: z.string(),
    inputMint: z.string(),
    outputMint: z.string(),
    inAmount: unsignedIntegerStringSchema,
    outAmount: unsignedIntegerStringSchema,
    label: z.string(),
    outAmountAfterSlippage: unsignedIntegerStringSchema.optional(),
  }).passthrough(),
  percent: z.number().int().nonnegative(),
  bps: z.number().int().nonnegative().nullable(),
}).passthrough();

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

const prioritizationFeeLamportsSchema = z.union([
  z.number().int().nonnegative(),
  z.object({
    priorityLevelWithMaxLamports: z.object({
      priorityLevel: z.string(),
      maxLamports: z.number().int().nonnegative(),
      global: z.boolean().optional(),
    }).optional(),
    jitoTipLamports: z.number().int().nonnegative().optional(),
    jitoTipLamportsWithPayer: z.number().int().nonnegative().optional(),
  }).passthrough(),
]);

const positiveSlippageSchema = z.object({
  bps: z.number().int().nonnegative(),
  feeAccount: z.string().optional(),
}).passthrough();

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
    description: "Number of queue entries to split the transfer across. Required when visibility=private.",
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

const privateTransferDiagnosticSchema = z.object({
  stashAta: z.string(),
  hydraCrankPda: z.string(),
  shuttleId: z.number().int().nonnegative(),
});

const swapResponseSchema = z.object({
  swapTransaction: z.string(),
  lastValidBlockHeight: z.number().int().nonnegative().optional(),
  prioritizationFeeLamports: z.number().int().nonnegative().optional(),
  privateTransfer: privateTransferDiagnosticSchema.optional().openapi({
    description: "Present when visibility=private. Diagnostic metadata about the appended schedule_private_transfer instruction.",
  }),
}).passthrough().openapi("SwapResponse");

const quoteRoute = createRoute({
  path: "/v1/swap/quote",
  method: "get",
  tags,
  description: "Get a swap quote.",
  request: {
    query: quoteQuerySchema,
  },
  responses: {
    200: jsonContent(quoteResponseSchema, "Swap quote", USDC_TO_USDT_QUOTE_EXAMPLE),
    500: jsonContent(errorResponseSchema, "Configuration error"),
    502: jsonContent(errorResponseSchema, "Upstream error"),
  },
});

const swapRoute = createRoute({
  path: "/v1/swap/swap",
  method: "post",
  tags,
  description: "Build an unsigned swap transaction from a quote.",
  request: {
    body: jsonContentRequired(swapRequestSchema, "Swap request", SWAP_REQUEST_EXAMPLE),
  },
  responses: {
    200: jsonContent(swapResponseSchema, "Swap transaction"),
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

async function proxyGet(c: Context<{ Bindings: AppBindings }>, upstreamPath: string) {
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
  }
  catch (error) {
    throw new ApiError(502, "SWAP_UPSTREAM_ERROR", "Failed to reach the swap upstream", {
      message: error instanceof Error ? error.message : String(error),
    });
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
  }
  catch (error) {
    throw new ApiError(502, "SWAP_UPSTREAM_ERROR", "Failed to reach the swap upstream", {
      message: error instanceof Error ? error.message : String(error),
    });
  }
}

app.openapi(quoteRoute, ((c: Context<{ Bindings: AppBindings }>) => proxyGet(c, "quote")) as any);
app.openapi(
  swapRoute,
  (async (c: Context<{ Bindings: AppBindings }>) => {
    const request = c.req as typeof c.req & {
      valid: (target: "json") => z.infer<typeof swapRequestSchema>;
    };
    const body = request.valid("json");

    if (body.visibility !== "private") {
      return proxyPost(c, "swap", body);
    }

    return handlePrivateSwap(c, body);
  }) as any,
);

// ---------------------------------------------------------------------------
// Private swap: Jupiter → stash ATA → schedule_private_transfer
// ---------------------------------------------------------------------------

async function handlePrivateSwap(
  c: Context<{ Bindings: AppBindings }>,
  body: z.infer<typeof swapRequestSchema>,
): Promise<Response> {
  const { destination, minDelayMs, maxDelayMs, split, clientRefId, validator } = body;

  if (!destination || minDelayMs === undefined || maxDelayMs === undefined || split === undefined) {
    throw new ApiError(
      400,
      "INVALID_REQUEST",
      "destination, minDelayMs, maxDelayMs, and split are required when visibility=private",
    );
  }

  let userPubkey: PublicKey;
  let mintPubkey: PublicKey;
  let destinationPubkey: PublicKey;
  let validatorPubkey: PublicKey;
  try {
    userPubkey = new PublicKey(body.userPublicKey);
    mintPubkey = new PublicKey(body.quoteResponse.outputMint);
    destinationPubkey = new PublicKey(destination);
    validatorPubkey = validator
      ? new PublicKey(validator)
      : DEFAULT_FALLBACK_VALIDATOR;
  }
  catch (error) {
    throw new ApiError(400, "INVALID_REQUEST", "Invalid public key", {
      message: error instanceof Error ? error.message : String(error),
    });
  }

  const [stashPda] = deriveStashPda(userPubkey, mintPubkey);
  const [stashAta] = deriveStashAta(userPubkey, mintPubkey);
  const [hydraCrankPda] = deriveHydraCrankPda(stashPda);

  if (body.destinationTokenAccount && body.destinationTokenAccount !== stashAta.toBase58()) {
    throw new ApiError(
      400,
      "INVALID_REQUEST",
      "destinationTokenAccount is controlled by the server when visibility=private",
      { expected: stashAta.toBase58() },
    );
  }

  const minDelayBig = BigInt(minDelayMs);
  const maxDelayBig = BigInt(maxDelayMs);
  const clientRefIdBig = clientRefId !== undefined ? BigInt(clientRefId) : undefined;
  if (maxDelayBig < minDelayBig) {
    throw new ApiError(400, "INVALID_REQUEST", "maxDelayMs must be >= minDelayMs");
  }

  const shuttleId = Math.floor(Math.random() * 0x1_0000_0000);

  // --- forward the swap request to Metis with the stash ATA forced ---
  const upstreamBody = buildUpstreamSwapBody(body, stashAta);
  const env = getEnv(c.env);
  const metisUrl = getMetisSwapApiUrl(c.env);
  if (!env.BASE_RPC_URL) {
    throw new ApiError(500, "CONFIG_ERROR", "Missing worker environment variable `BASE_RPC_URL`");
  }

  const metisResponse = await (async () => {
    try {
      return await fetch(`${metisUrl}/swap`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(upstreamBody),
      });
    }
    catch (error) {
      throw new ApiError(502, "SWAP_UPSTREAM_ERROR", "Failed to reach the swap upstream", {
        message: error instanceof Error ? error.message : String(error),
      });
    }
  })();

  if (!metisResponse.ok) {
    // Pass upstream error body through unchanged.
    return new Response(metisResponse.body, {
      status: metisResponse.status,
      headers: metisResponse.headers,
    });
  }

  const metisJson = await metisResponse.json() as {
    swapTransaction: string;
    lastValidBlockHeight?: number;
    prioritizationFeeLamports?: number;
    [k: string]: unknown;
  };

  // --- deserialize, prepend ATA-create, append schedule_private_transfer ---
  const connection = new Connection(env.BASE_RPC_URL, "confirmed");
  const rebuilt = await rebuildSwapTransaction({
    connection,
    base64Tx: metisJson.swapTransaction,
    payer: userPubkey,
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
  body: z.infer<typeof swapRequestSchema>,
  stashAta: PublicKey,
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
    destinationTokenAccount: stashAta.toBase58(),
    asLegacyTransaction: false,
  };
}

type RebuildInput = {
  connection: Connection;
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
    connection,
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

  const txBytes = Uint8Array.from(atob(base64Tx), (c) => c.charCodeAt(0));
  const versionedTx = VersionedTransaction.deserialize(txBytes);

  const altKeys = versionedTx.message.addressTableLookups.map((l) => l.accountKey);
  const lookupTables: AddressLookupTableAccount[] = [];
  for (const key of altKeys) {
    const resp = await connection.getAddressLookupTable(key);
    if (resp.value) {
      lookupTables.push(resp.value);
    }
  }

  const message = TransactionMessage.decompile(versionedTx.message, {
    addressLookupTableAccounts: lookupTables,
  });

  message.instructions.unshift(
    createAssociatedTokenAccountIdempotentInstruction(
      payer,
      stashAta,
      stashPda,
      mint,
    ),
  );
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
      clientRefId,
    ),
  );

  const rebuilt = new VersionedTransaction(
    message.compileToV0Message(lookupTables),
  );

  return bytesToBase64(rebuilt.serialize());
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

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i += 1) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

export default app;
