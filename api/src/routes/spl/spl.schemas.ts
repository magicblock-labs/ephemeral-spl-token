import { z } from "@hono/zod-openapi";
import { PublicKey } from "@solana/web3.js";

const DEFAULT_DEPOSIT_VALIDATOR = "MAS1Dt9qreoRMQ14YQuhg8UTZMMzDdKhmkZMECCzk57";
const DEFAULT_DEPOSIT_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const DEFAULT_DEPOSIT_DEVNET_MINT = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";
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

const isNonNegativeBigIntString = (value: string) => {
  if (!/^\d+$/.test(value)) {
    return false;
  }

  try {
    BigInt(value);
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
  .safe()
  .min(1)
  .openapi({
    example: 1000000,
    description: "Base-unit amount as an integer JSON value with minimum 1.",
  });

export const depositAmountSchema = z
  .number()
  .int()
  .safe()
  .min(1)
  .openapi({
    example: 1,
    description: "Base-unit amount as an integer JSON value with minimum 1.",
  });

export const withdrawAmountSchema = z
  .number()
  .int()
  .safe()
  .min(1)
  .openapi({
    example: 1000000,
    description: "Base-unit amount as an integer JSON value with minimum 1.",
  });

export const optionalBigIntStringSchema = z
  .string()
  .refine(isNonNegativeBigIntString, "Must be a non-negative bigint string")
  .openapi({
    example: "0",
  });

export const optionalBoolean = z.preprocess((value) => {
  if (typeof value === "string") {
    const normalized = value.trim().toLowerCase();
    if (normalized === "true") return true;
    if (normalized === "false") return false;
  }
  return value;
}, z.boolean().optional().default(false));

export const clusterSchema = z.union([
  z.enum(["mainnet", "devnet"]),
  z.string().refine((value) => /^https?:\/\//.test(value), {
    message: "must be a http(s) URL",
  }),
]).openapi({
  example: "mainnet",
  description: "Optional. Use `mainnet` for BASE_RPC_URL and EPHEMERAL_RPC_URL, `devnet` for BASE_DEVNET_RPC_URL and EPHEMERAL_DEVNET_RPC_URL, or provide a custom http(s) RPC URL to override the base RPC while keeping the configured ephemeral RPC.",
});

export const visibilitySchema = z.enum(["public", "private"]).openapi("TransferVisibility");
export const balanceLocationSchema = z.enum(["base", "ephemeral"]).openapi("BalanceLocation");
export const optionalAuthTokenSchema = z.object({
  authorization: z.string().openapi({
    example: "Bearer 1234567890",
    description: "Optional. Authentication token for requests that need to connect to the Private Ephemeral Rollup.",
  }).optional()
});
export const requiredAuthTokenSchema = z.object({
  authorization: z.string().openapi({
    example: "Bearer 1234567890",
    description: "Required. Authentication token for private-balance requests.",
  })
});

export const transactionResponseSchema = z.object({
  kind: z.enum(["deposit", "withdraw", "transfer"]),
  version: z.enum(["legacy", "v0"]),
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

export const mintInitializationQuerySchema = z.object({
  mint: publicKeySchema.openapi({
    example: DEFAULT_DEPOSIT_MINT,
  }),
  cluster: clusterSchema.optional(),
  validator: publicKeySchema.openapi({
    example: DEFAULT_DEPOSIT_VALIDATOR,
    description: "Optional. Defaults to the selected ephemeral RPC identity resolved via `getIdentity`.",
  }).optional(),
}).openapi("MintInitializationQuery", {
  example: {
    mint: DEFAULT_DEPOSIT_MINT,
    validator: DEFAULT_DEPOSIT_VALIDATOR,
  },
});

export const mintInitializationResponseSchema = z.object({
  mint: publicKeySchema,
  validator: publicKeySchema,
  transferQueue: publicKeySchema,
  initialized: z.boolean(),
}).openapi("MintInitializationResponse");

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

export const initializeMintResponseSchema = transactionResponseSchema.extend({
  kind: z.literal("initializeMint"),
  validator: publicKeySchema,
  transferQueue: publicKeySchema,
  rentPda: publicKeySchema,
}).openapi("InitializeMintResponse");

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
  clientRefId: optionalBigIntStringSchema.openapi({
    example: "42",
    description: "Optional. Private transfer only. Encrypted client reference ID that can be used to confirm a payment.",
  }).optional(),
  split: z.int().positive().max(15).openapi({
    example: 1,
    description: "Optional. Private transfer only. Defaults to 1. Must be between 1 and 15.",
  }).optional(),
  gasless: z.boolean().openapi({
    example: true,
    description: "Optional. Private transfer only. When true, the API uses the configured sponsor as transaction fee payer and prepends a relay-fee token transfer to the sponsor ATA.",
  }).optional(),
  legacy: z.boolean().openapi({
    description: "Optional. Defaults to false. When true, skips lookup-table compilation and returns a legacy transaction.",
  })
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
    initVaultIfMissing: false,
    memo: "Order #1042",
    minDelayMs: "0",
    maxDelayMs: "0",
    clientRefId: "42",
    split: 1,
    gasless: true,
  },
});

export const balanceQuerySchema = z.object({
  address: publicKeySchema,
  mint: publicKeySchema,
  cluster: clusterSchema.optional(),
}).openapi("BalanceQuery", {
  example: {
    address: BALANCE_EXAMPLE_ADDRESS,
    mint: DEFAULT_DEPOSIT_MINT,
  },
});

export const challengeQuerySchema = z.object({
  cluster: clusterSchema.optional(),
  pubkey: publicKeySchema.openapi({
    example: BALANCE_EXAMPLE_ADDRESS,
    description: "The public key of the wallet that will read private data",
  }),
  mock: optionalBoolean,
});

export const challengeResponseSchema = z.object({
  challenge: z.string().openapi({
    example: "1234567890",
    description: "The challenge string generated by the Private Ephemeral Rollup",
  }),
});

export const loginQuerySchema = z.object({
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
  mock: optionalBoolean,
});

export const loginResponseSchema = z.object({
  token: z.string().openapi({
    example: "1234567890",
    description: "The authentication token provided by the Private Ephemeral Rollup",
  }),
});
