import { z } from "@hono/zod-openapi";

import { balanceLocationSchema, clusterSchema } from "../schema";

export const sendTransactionRequestSchema = z.object({
  transactionBase64: z.string().min(1).openapi({
    example: "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAA==",
    description: "Signed serialized Solana transaction encoded as base64.",
  }),
  sendTo: balanceLocationSchema.openapi({
    description: "RPC target for submission. Use the `sendTo` value returned by transaction-builder endpoints.",
  }),
  sendRpcEndpoint: z.string().url().optional().openapi({
    description: "Optional exact ephemeral RPC endpoint returned by transaction-builder endpoints. Only valid when `sendTo` is `ephemeral`.",
  }),
  cluster: clusterSchema.optional(),
  confirm: z.boolean().optional().openapi({
    example: false,
    description: "Optional. Defaults to false. When true, `recentBlockhash` and `lastValidBlockHeight` are required.",
  }),
  recentBlockhash: z.string().optional().openapi({
    example: "9A4VhP8M8fQZxP4h7rB6mP6eM8w2pJkYh7QdZk7V4r2x",
    description: "Optional. Required only when `confirm` is true.",
  }),
  lastValidBlockHeight: z.number().int().nonnegative().optional().openapi({
    example: 284512337,
    description: "Optional. Required only when `confirm` is true.",
  }),
  skipPreflight: z.boolean().optional(),
  maxRetries: z.number().int().nonnegative().optional(),
}).openapi("SendTransactionRequest", {
  example: {
    transactionBase64: "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAA==",
    sendTo: "base",
  },
});
export type SendTransactionRequest = z.infer<typeof sendTransactionRequestSchema>;

export const sendTransactionResponseSchema = z.object({
  signature: z.string(),
  sendTo: balanceLocationSchema,
  confirmed: z.boolean(),
  confirmationRpcEndpoint: z.string().url(),
  confirmationRequiresAuthToken: z.boolean(),
}).openapi("SendTransactionResponse");
export type SendTransactionResponse = z.infer<typeof sendTransactionResponseSchema>;
