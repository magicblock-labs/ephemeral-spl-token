import { z } from "@hono/zod-openapi";
import { PublicKey } from "@solana/web3.js";

const DEFAULT_DEPOSIT_VALIDATOR = "MAS1Dt9qreoRMQ14YQuhg8UTZMMzDdKhmkZMECCzk57";
const DEFAULT_DEPOSIT_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const DEPOSIT_EXAMPLE_OWNER = "3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE";
const TRANSFER_EXAMPLE_TO = "Bt9oNR5cCtnfuMmXgWELd6q5i974PdEMQDUE55nBC57L";
const BALANCE_EXAMPLE_ADDRESS = "Bt9oNR5cCtnfuMmXgWELd6q5i974PdEMQDUE55nBC57L";

const isPublicKey = (value: string) => {
  try {
    new PublicKey(value);
    return true;
  }
  catch {
    return false;
  }
};

export const publicKeySchema = z
  .string()
  .refine(isPublicKey, "Invalid public key")
  .openapi({
    example: "So11111111111111111111111111111111111111112",
  });

export const amountSchema = z
  .number()
  .int()
  .min(1)
  .openapi({
    example: 1000000,
    description: "Base-unit amount as an integer JSON value with minimum 1.",
  });

export const depositAmountSchema = z
  .number()
  .int()
  .min(1)
  .openapi({
    example: 1,
    description: "Base-unit amount as an integer JSON value with minimum 1.",
  });

export const withdrawAmountSchema = z
  .number()
  .int()
  .min(1)
  .openapi({
    example: 1000000,
    description: "Base-unit amount as an integer JSON value with minimum 1.",
  });

export const optionalBigIntStringSchema = z
  .string()
  .regex(/^\d+$/, "Must be an integer string")
  .openapi({
    example: "0",
  });

export const clusterSchema = z.string().openapi({
  example: "mainnet",
  description: "Optional. Use `mainnet` for BASE_RPC_URL and EPHEMERAL_RPC_URL, `devnet` for BASE_DEVNET_RPC_URL and EPHEMERAL_DEVNET_RPC_URL, or provide a custom http(s) RPC URL to override the base RPC while keeping the configured ephemeral RPC.",
});

export const visibilitySchema = z.enum(["public", "private"]).openapi("TransferVisibility");
export const balanceLocationSchema = z.enum(["base", "ephemeral"]).openapi("BalanceLocation");

export const transactionResponseSchema = z.object({
  kind: z.enum(["deposit", "withdraw", "transfer"]),
  version: z.literal("legacy"),
  transactionBase64: z.string(),
  sendTo: balanceLocationSchema,
  recentBlockhash: z.string(),
  lastValidBlockHeight: z.number().int(),
  instructionCount: z.number().int().nonnegative(),
  requiredSigners: z.array(publicKeySchema),
  validator: publicKeySchema.optional(),
}).openapi("UnsignedTransactionResponse");

export const balanceResponseSchema = z.object({
  address: publicKeySchema,
  mint: publicKeySchema,
  ata: publicKeySchema,
  location: balanceLocationSchema,
  balance: z.string(),
}).openapi("BalanceResponse");

export const depositRequestSchema = z.object({
  owner: publicKeySchema.openapi({
    example: DEPOSIT_EXAMPLE_OWNER,
  }),
  cluster: clusterSchema.optional(),
  mint: publicKeySchema.openapi({
    example: DEFAULT_DEPOSIT_MINT,
    description: `Optional. Defaults to Solana USDC: ${DEFAULT_DEPOSIT_MINT}.`,
  }).optional(),
  amount: depositAmountSchema,
  validator: publicKeySchema.openapi({
    example: DEFAULT_DEPOSIT_VALIDATOR,
    description: "Optional. Defaults to the selected ephemeral RPC identity resolved via `getIdentity`.",
  }).optional(),
  initIfMissing: z.boolean().optional(),
  initVaultIfMissing: z.boolean().optional(),
  initAtasIfMissing: z.boolean().optional(),
  idempotent: z.boolean().optional(),
}).openapi("DepositRequest", {
  example: {
    owner: DEPOSIT_EXAMPLE_OWNER,
    amount: 1,
    initIfMissing: true,
    initVaultIfMissing: true,
    initAtasIfMissing: true,
    idempotent: true,
  },
});

export const withdrawRequestSchema = z.object({
  owner: publicKeySchema.openapi({
    example: DEPOSIT_EXAMPLE_OWNER,
  }),
  cluster: clusterSchema.optional(),
  mint: publicKeySchema.openapi({
    example: DEFAULT_DEPOSIT_MINT,
    description: "SPL mint on Solana.",
  }),
  amount: withdrawAmountSchema,
  validator: publicKeySchema.openapi({
    example: DEFAULT_DEPOSIT_VALIDATOR,
    description: "Optional. Defaults to the selected ephemeral RPC identity resolved via `getIdentity`.",
  }).optional(),
  initIfMissing: z.boolean().optional(),
  initAtasIfMissing: z.boolean().optional(),
  escrowIndex: z.int().nonnegative().optional(),
  idempotent: z.boolean().optional(),
}).openapi("WithdrawRequest", {
  example: {
    owner: DEPOSIT_EXAMPLE_OWNER,
    mint: DEFAULT_DEPOSIT_MINT,
    amount: 1000000,
    idempotent: true,
  },
});

export const transferRequestSchema = z.object({
  from: publicKeySchema,
  to: publicKeySchema,
  cluster: clusterSchema.optional(),
  mint: publicKeySchema,
  amount: amountSchema,
  visibility: visibilitySchema,
  fromBalance: balanceLocationSchema,
  toBalance: balanceLocationSchema,
  validator: publicKeySchema.openapi({
    example: DEFAULT_DEPOSIT_VALIDATOR,
    description: "Optional. When this transfer route needs a validator and none is provided, the API resolves it from the selected ephemeral RPC via `getIdentity`.",
  }).optional(),
  initIfMissing: z.boolean().optional(),
  initAtasIfMissing: z.boolean().optional(),
  initVaultIfMissing: z.boolean().optional(),
  memo: z.string().openapi({
    example: "Order #1042",
    description: "Optional. Appends a final Memo Program instruction with this UTF-8 message.",
  }).optional(),
  minDelayMs: optionalBigIntStringSchema.openapi({
    example: "0",
    description: "Optional. Private transfer only. Defaults to 0.",
  }).optional(),
  maxDelayMs: optionalBigIntStringSchema.openapi({
    example: "0",
    description: "Optional. Private transfer only. Defaults to 0 when omitted, or to minDelayMs when minDelayMs is set.",
  }).optional(),
  split: z.int().positive().openapi({
    example: 1,
    description: "Optional. Private transfer only. Defaults to 1.",
  }).optional(),
}).openapi("TransferRequest", {
  example: {
    from: DEPOSIT_EXAMPLE_OWNER,
    to: TRANSFER_EXAMPLE_TO,
    mint: DEFAULT_DEPOSIT_MINT,
    amount: 1000000,
    visibility: "private",
    fromBalance: "base",
    toBalance: "base",
    initIfMissing: true,
    initAtasIfMissing: true,
    initVaultIfMissing: true,
    memo: "Order #1042",
    minDelayMs: "0",
    maxDelayMs: "0",
    split: 1,
  },
});

export const balanceQuerySchema = z.object({
  address: publicKeySchema.openapi({
    example: BALANCE_EXAMPLE_ADDRESS,
  }),
  cluster: clusterSchema.optional(),
  mint: publicKeySchema.openapi({
    example: DEFAULT_DEPOSIT_MINT,
  }),
}).openapi("BalanceQuery", {
  example: {
    address: BALANCE_EXAMPLE_ADDRESS,
    mint: DEFAULT_DEPOSIT_MINT,
  },
});
