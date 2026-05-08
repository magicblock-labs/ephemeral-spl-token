import { PublicKey } from "@solana/web3.js";
import { z } from "@hono/zod-openapi";

const isPublicKey = (value: string) => {
  try {
    new PublicKey(value);
    return true;
  } catch {
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
  } catch {
    return false;
  }
};

export const publicKeySchema = z
  .string()
  .refine(isPublicKey, "Invalid public key")
  .openapi({
    example: "So11111111111111111111111111111111111111112",
  });

export const amountSchema = z.number().int().safe().min(1).openapi({
  example: 1000000,
  description: "Base-unit amount as an integer JSON value with minimum 1.",
});

export const depositAmountSchema = z.number().int().safe().min(1).openapi({
  example: 1,
  description: "Base-unit amount as an integer JSON value with minimum 1.",
});

export const withdrawAmountSchema = z.number().int().safe().min(1).openapi({
  example: 1000000,
  description: "Base-unit amount as an integer JSON value with minimum 1.",
});

export const optionalBigIntStringSchema = z
  .string()
  .refine(isNonNegativeBigIntString, "Must be a non-negative bigint string")
  .openapi({
    example: "0",
  });

export const optionalIntegerSchema = z.preprocess((value) => {
  if (value === undefined || value === "") {
    return undefined;
  }

  if (typeof value === "string" && /^\d+$/.test(value)) {
    return Number(value);
  }

  return value;
}, z.number().int().nonnegative().optional());

export const unsignedIntegerStringSchema = z
  .string()
  .regex(/^\d+$/, "Must be an unsigned integer string");

export const swapModeSchema = z
  .enum(["ExactIn", "ExactOut"])
  .openapi("SwapMode");

export const instructionVersionSchema = z
  .enum(["V1", "V2"])
  .openapi("InstructionVersion");

export const optionalBooleanSchema = z.preprocess((value) => {
  if (typeof value === "string") {
    const normalized = value.trim().toLowerCase();
    if (normalized === "true") return true;
    if (normalized === "false") return false;
  }
  return value;
}, z.boolean().optional());

export const clusterSchema = z
  .union([
    z.enum(["mainnet", "devnet"]),
    z
      .string()
      .url()
      .refine(
        (value) => {
          const protocol = new URL(value).protocol;
          return protocol === "http:" || protocol === "https:";
        },
        { message: "must be a http(s) URL" },
      ),
  ])
  .openapi({
    example: "mainnet",
    description:
      "Optional. Use `mainnet` for BASE_RPC_URL and EPHEMERAL_RPC_URL, `devnet` for BASE_DEVNET_RPC_URL and EPHEMERAL_DEVNET_RPC_URL, or provide a custom http(s) RPC URL to override the base RPC while keeping the configured ephemeral RPC.",
  });

export const visibilitySchema = z
  .enum(["public", "private"])
  .openapi("TransferVisibility");

export const balanceLocationSchema = z
  .enum(["base", "ephemeral"])
  .openapi("BalanceLocation");

export const optionalAuthTokenSchema = z.object({
  authorization: z
    .string()
    .openapi({
      example: "Bearer 1234567890",
      description:
        "Optional. Authentication token for requests that need to connect to the Private Ephemeral Rollup.",
    })
    .optional(),
});

export const requiredAuthTokenSchema = z.object({
  authorization: z.string().openapi({
    example: "Bearer 1234567890",
    description: "Required. Authentication token for private-balance requests.",
  }),
});

export const prioritizationFeeLamportsSchema = z.union([
  z.number().int().nonnegative(),
  z
    .object({
      priorityLevelWithMaxLamports: z
        .object({
          priorityLevel: z.string(),
          maxLamports: z.number().int().nonnegative(),
          global: z.boolean().optional(),
        })
        .optional(),
      jitoTipLamports: z.number().int().nonnegative().optional(),
      jitoTipLamportsWithPayer: z.number().int().nonnegative().optional(),
    })
    .passthrough()
    .refine(
      v =>
        v.priorityLevelWithMaxLamports !== undefined
        || v.jitoTipLamports !== undefined
        || v.jitoTipLamportsWithPayer !== undefined,
      "At least one prioritization fee field must be provided",
    ),
]);

export const positiveSlippageSchema = z
  .object({
    bps: z.number().int().nonnegative(),
    feeAccount: z.string().optional(),
  })
  .passthrough();

export const privateTransferDiagnosticSchema = z.object({
  stashAta: z.string(),
  hydraCrankPda: z.string(),
  shuttleId: z.number().int().nonnegative(),
});

export const quoteRoutePlanSchema = z
  .object({
    swapInfo: z
      .object({
        ammKey: z.string(),
        inputMint: z.string(),
        outputMint: z.string(),
        inAmount: unsignedIntegerStringSchema,
        outAmount: unsignedIntegerStringSchema,
        label: z.string(),
        outAmountAfterSlippage: unsignedIntegerStringSchema.optional(),
      })
      .passthrough(),
    percent: z.number().int().nonnegative(),
    bps: z.number().int().nonnegative().nullable(),
  })
  .passthrough();

export const validationIssueSchema = z.object({
  code: z.string(),
  message: z.string(),
  path: z.array(z.union([z.string(), z.number()])),
});

export const errorPayloadSchema = z.object({
  code: z.string(),
  message: z.string(),
  details: z.unknown().optional(),
});
