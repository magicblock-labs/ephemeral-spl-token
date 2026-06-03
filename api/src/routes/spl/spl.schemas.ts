import { z } from "@hono/zod-openapi";
import { boolean } from "zod";
import { amountSchema, balanceLocationSchema, clusterSchema, depositAmountSchema, optionalBigIntStringSchema, publicKeySchema, visibilitySchema, withdrawAmountSchema } from "../../schema";

const DEFAULT_DEPOSIT_VALIDATOR = "MAS1Dt9qreoRMQ14YQuhg8UTZMMzDdKhmkZMECCzk57";
const DEFAULT_DEPOSIT_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const DEFAULT_DEPOSIT_DEVNET_MINT = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";
const DEPOSIT_EXAMPLE_OWNER = "3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE";
const TRANSFER_EXAMPLE_TO = "Bt9oNR5cCtnfuMmXgWELd6q5i974PdEMQDUE55nBC57L";
const BALANCE_EXAMPLE_ADDRESS = "Bt9oNR5cCtnfuMmXgWELd6q5i974PdEMQDUE55nBC57L";

export const transferFeesSchema = z.object({
  lamports: z.string().openapi({
    example: "2039280",
    description: "Total lamport fees charged by the transfer. Returns \"0\" when no lamport fee is charged.",
  }),
  tokens: z.string().openapi({
    example: "205000",
    description: "Total token fees charged by the transfer, in mint base units. Returns \"0\" when no token fee is charged.",
  }),
}).openapi("TransferFees");
export type TransferFees = z.infer<typeof transferFeesSchema>;

export const transactionResponseSchema = z.object({
  kind: z.enum(["deposit", "withdraw", "transfer", "initializeMint"]),
  version: z.enum(["legacy", "v0"]),
  transactionBase64: z.string(),
  sendTo: balanceLocationSchema,
  from: balanceLocationSchema.optional(),
  recentBlockhash: z.string(),
  lastValidBlockHeight: z.number().int(),
  instructionCount: z.number().int().nonnegative(),
  requiredSigners: z.array(publicKeySchema),
  validator: publicKeySchema.optional(),
  fees: transferFeesSchema.optional(),
}).openapi("UnsignedTransactionResponse");
export type TransactionResponse = z.infer<typeof transactionResponseSchema>;

export const balanceResponseSchema = z.object({
  address: publicKeySchema,
  mint: publicKeySchema,
  ata: publicKeySchema,
  location: balanceLocationSchema,
  balance: z.string(),
}).openapi("BalanceResponse");
export type BalanceResponse = z.infer<typeof balanceResponseSchema>;

export const mintInitializationRequestSchema = z.object({
  mint: publicKeySchema.openapi({
    example: DEFAULT_DEPOSIT_MINT,
  }),
  cluster: clusterSchema.optional(),
  validator: publicKeySchema.openapi({
    example: DEFAULT_DEPOSIT_VALIDATOR,
    description: "Optional. Defaults to the selected ephemeral RPC identity resolved via `getIdentity`.",
  }).optional(),
}).openapi("MintInitializationRequest", {
  example: {
    mint: DEFAULT_DEPOSIT_MINT,
    validator: DEFAULT_DEPOSIT_VALIDATOR,
  },
});
export type MintInitializationRequest = z.infer<typeof mintInitializationRequestSchema>;

export const mintInitializationResponseSchema = z.object({
  mint: publicKeySchema,
  validator: publicKeySchema,
  transferQueue: publicKeySchema,
  initialized: z.boolean(),
}).openapi("MintInitializationResponse");
export type MintInitializationResponse = z.infer<typeof mintInitializationResponseSchema>;

export const initializeMintRequestSchema = z.object({
  payer: publicKeySchema.openapi({
    example: DEPOSIT_EXAMPLE_OWNER,
  }),
  mint: publicKeySchema.openapi({
    example: DEFAULT_DEPOSIT_MINT,
  }),
  cluster: clusterSchema.optional(),
  validator: publicKeySchema.openapi({
    example: DEFAULT_DEPOSIT_VALIDATOR,
    description: "Optional. Defaults to the selected ephemeral RPC identity resolved via `getIdentity`.",
  }).optional(),
}).openapi("InitializeMintRequest", {
  example: {
    payer: DEPOSIT_EXAMPLE_OWNER,
    mint: DEFAULT_DEPOSIT_MINT,
    validator: DEFAULT_DEPOSIT_VALIDATOR,
  },
});
export type InitializeMintRequest = z.infer<typeof initializeMintRequestSchema>;

export const initializeMintResponseSchema = transactionResponseSchema.extend({
  kind: z.literal("initializeMint"),
  validator: publicKeySchema,
  transferQueue: publicKeySchema,
  rentPda: publicKeySchema,
}).openapi("InitializeMintResponse");
export type InitializeMintResponse = z.infer<typeof initializeMintResponseSchema>;

export const depositRequestSchema = z.object({
  owner: publicKeySchema.openapi({
    example: DEPOSIT_EXAMPLE_OWNER,
  }),
  cluster: clusterSchema.optional(),
  mint: publicKeySchema.openapi({
    example: DEFAULT_DEPOSIT_MINT,
    description: `Optional. Defaults to Solana USDC on mainnet: ${DEFAULT_DEPOSIT_MINT}. On devnet it defaults to devnet USDC: ${DEFAULT_DEPOSIT_DEVNET_MINT}.`,
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
    initVaultIfMissing: false,
    initAtasIfMissing: true,
    idempotent: true,
  },
});
export type DepositRequest = z.infer<typeof depositRequestSchema>;

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
export type WithdrawRequest = z.infer<typeof withdrawRequestSchema>;

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
  clientRefId: optionalBigIntStringSchema.openapi({
    example: "42",
    description: "Optional. Private transfer only. Encrypted client reference ID that can be used to confirm a payment.",
  }).optional(),
  split: z.int().positive().max(15).openapi({
    example: 1,
    description: "Optional. Private transfer only. Defaults to 1. Must be between 1 and 15.",
  }).optional(),
  exactOut: z.boolean().openapi({
    example: boolean,
    description: "Optional. If true, the fees are deducted from the sender, else from the recipient amount",
  }).optional(),
  gasless: z.boolean().openapi({
    example: true,
    description: "Optional. When true, the API uses the configured sponsor as transaction fee payer and prepends a 0.2 USDC/USDT relay-fee token transfer to the sponsor ATA. Requires GASLESS_SPONSOR_SECRET_KEY, an approved stablecoin mint (mainnet USDC/USDT or devnet USDC), and at least 5 USDC/USDT.",
  }).optional(),
  legacy: z.boolean().openapi({
    description: "Optional. Defaults to false. When true, skips lookup-table compilation and returns a legacy transaction.",
  }).optional(),
}).openapi("TransferRequest", {
  example: {
    from: DEPOSIT_EXAMPLE_OWNER,
    to: TRANSFER_EXAMPLE_TO,
    mint: DEFAULT_DEPOSIT_MINT,
    amount: 5000000,
    visibility: "private",
    fromBalance: "base",
    toBalance: "base",
    memo: "Order #1042",
    minDelayMs: "0",
    maxDelayMs: "0",
    clientRefId: "42",
    gasless: true,
  },
});
export type TransferRequest = z.infer<typeof transferRequestSchema>;

export const balanceRequestSchema = z.object({
  address: publicKeySchema,
  mint: publicKeySchema,
  cluster: clusterSchema.optional(),
}).openapi("BalanceRequest", {
  example: {
    address: BALANCE_EXAMPLE_ADDRESS,
    mint: DEFAULT_DEPOSIT_MINT,
  },
});
export type BalanceRequest = z.infer<typeof balanceRequestSchema>;

export const challengeRequestSchema = z.object({
  cluster: clusterSchema.optional(),
  pubkey: publicKeySchema.openapi({
    example: BALANCE_EXAMPLE_ADDRESS,
    description: "The public key of the wallet that will read private data",
  }),
});
export type ChallengeRequest = z.infer<typeof challengeRequestSchema>;

export const challengeResponseSchema = z.object({
  challenge: z.string().openapi({
    example: "1234567890",
    description: "The challenge string generated by the Private Ephemeral Rollup",
  }),
  error: z.string().optional(),
});
export type ChallengeResponse = z.infer<typeof challengeResponseSchema>;

export const loginRequestSchema = z.object({
  cluster: clusterSchema.optional(),
  pubkey: publicKeySchema.openapi({
    example: BALANCE_EXAMPLE_ADDRESS,
    description: "The public key of the wallet that will read private data",
  }),
  challenge: z.string().openapi({
    example: "1234567890",
    description: "The challenge string generated by the Private Ephemeral Rollup",
  }),
  signature: z.string().openapi({
    example: "1234567890",
    description: "The signature of the challenge by the wallet",
  }),
});
export type LoginRequest = z.infer<typeof loginRequestSchema>;

export const loginResponseSchema = z.object({
  token: z.string().openapi({
    example: "1234567890",
    description: "The authentication token provided by the Private Ephemeral Rollup",
  }),
  error: z.string().optional(),
});
export type LoginResponse = z.infer<typeof loginResponseSchema>;
