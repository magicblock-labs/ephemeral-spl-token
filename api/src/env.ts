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
  BASE_RPC_URL: z.string().url(),
  EPHEMERAL_RPC_URL: z.string().url(),
  BASE_DEVNET_RPC_URL: optionalUrl,
  EPHEMERAL_DEVNET_RPC_URL: optionalUrl,
  CORS_ORIGIN: optionalString,
  MOCK_PER: z.boolean().optional().default(false),
});

export type AppBindings = {
  BASE_RPC_URL?: string;
  EPHEMERAL_RPC_URL?: string;
  BASE_DEVNET_RPC_URL?: string;
  EPHEMERAL_DEVNET_RPC_URL?: string;
  CORS_ORIGIN?: string;
  MOCK_PER?: boolean;
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
          issues: error.issues.map((issue) => ({
            path: issue.path.map((segment) =>
              typeof segment === "number" ? segment : String(segment),
            ),
            message: issue.message,
          })),
          hint: "Create .dev.vars from .dev.vars.example and set BASE_RPC_URL and EPHEMERAL_RPC_URL before running wrangler dev. If you want cluster=devnet, also set BASE_DEVNET_RPC_URL and EPHEMERAL_DEVNET_RPC_URL.",
        },
      );
    }

    throw error;
  }
}
