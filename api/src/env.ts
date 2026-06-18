import { z } from "zod";

import { ApiError } from "./lib/errors";

const optionalString = z
  .string()
  .optional()
  .transform((value) => {
    if (value === undefined) {
      return undefined;
    }

    const trimmed = value.trim();
    return trimmed.length > 0 ? trimmed : undefined;
  });

const optionalUrl = optionalString.pipe(z.string().url().optional());

export const envSchema = z.object({
  CLUSTER: z.literal(["mainnet", "devnet", "mainnet-private", "devnet-private", "custom"]).default("mainnet"),
  BASE_RPC_URL: z.string().url(),
  EPHEMERAL_RPC_URL: z.string().url(),
  BASE_DEVNET_RPC_URL: optionalUrl,
  EPHEMERAL_DEVNET_RPC_URL: optionalUrl,
  EPHEMERAL_TEE_RPC_URL: optionalUrl,
  EPHEMERAL_DEVNET_TEE_RPC_URL: optionalUrl,
  TRANSFER_QUEUE_CRANK_RPC_URL: optionalUrl,
  TRANSFER_QUEUE_DEVNET_CRANK_RPC_URL: optionalUrl,
  METIS_SWAP_API_URL: optionalUrl,
  PRIVATE_BASE_TO_BASE_TRANSFER_MAINNET_LOOKUP_TABLE: optionalString,
  PRIVATE_BASE_TO_BASE_TRANSFER_DEVNET_LOOKUP_TABLE: optionalString,
  GASLESS_SPONSOR_SECRET_KEY: optionalString,
  CORS_ORIGIN: optionalString,
});

export type AppBindings = {
  CLUSTER?: string;
  BASE_RPC_URL?: string;
  EPHEMERAL_RPC_URL?: string;
  BASE_DEVNET_RPC_URL?: string;
  EPHEMERAL_DEVNET_RPC_URL?: string;
  EPHEMERAL_TEE_RPC_URL?: string;
  EPHEMERAL_DEVNET_TEE_RPC_URL?: string;
  TRANSFER_QUEUE_CRANK_RPC_URL?: string;
  TRANSFER_QUEUE_DEVNET_CRANK_RPC_URL?: string;
  METIS_SWAP_API_URL?: string;
  PRIVATE_BASE_TO_BASE_TRANSFER_MAINNET_LOOKUP_TABLE?: string;
  PRIVATE_BASE_TO_BASE_TRANSFER_DEVNET_LOOKUP_TABLE?: string;
  GASLESS_SPONSOR_SECRET_KEY?: string;
  CORS_ORIGIN?: string;
};

export type AppEnv = z.infer<typeof envSchema>;

export function getEnv(bindings: AppBindings): AppEnv {
  try {
    return envSchema.parse(bindings);
  } catch (error) {
    if (error instanceof z.ZodError) {
      throw new ApiError(
        500,
        "CONFIG_ERROR",
        "Missing or invalid worker environment variables",
        {
          issues: error.issues.map(issue => ({
            path: issue.path.map(segment =>
              typeof segment === "number" ? segment : String(segment),
            ),
            message: issue.message,
          })),
          hint: "Create .dev.vars from .dev.vars.example and set BASE_RPC_URL and EPHEMERAL_RPC_URL before running wrangler dev. If you want cluster=devnet, also set BASE_DEVNET_RPC_URL and EPHEMERAL_DEVNET_RPC_URL. If you want cluster=mainnet-private or cluster=devnet-private, also set EPHEMERAL_TEE_RPC_URL or EPHEMERAL_DEVNET_TEE_RPC_URL. To enable gasless private transfers, also set GASLESS_SPONSOR_SECRET_KEY to a JSON-encoded secret key array.",
        },
      );
    }

    throw error;
  }
}
