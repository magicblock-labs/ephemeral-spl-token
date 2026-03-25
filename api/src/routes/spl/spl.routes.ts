import { createRoute } from "@hono/zod-openapi";

import {
  errorResponseSchema,
  validationErrorResponseSchema,
} from "../../lib/errors";
import {
  jsonContent,
  jsonContentRequired,
} from "../../lib/openapi";
import {
  balanceQuerySchema,
  balanceResponseSchema,
  depositRequestSchema,
  transactionResponseSchema,
  transferRequestSchema,
  withdrawRequestSchema,
} from "./spl.schemas";

const tags = ["SPL"];
const depositResponseExample = {
  kind: "deposit" as const,
  version: "legacy" as const,
  transactionBase64: "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAIDKmcfsS5XfSOLaLlaBHJry50iH2Ufk2TMz4STC2fHzIcFKkerg3q2DD3Yn8TISmGeKoxSLz+BiP7iQ4pYqXYXsgu8D8C7R8ovdMQRLpSrE8+jxjTl3BfqywPNGiPNfnh8eS+smowIxqKDcCjw5liNXQkkCbBSDCBDFwtrgCKqoQ0DAgEBBAECAwQCAQEEAgIDBAIBAQQDAgME",
  sendTo: "base" as const,
  recentBlockhash: "9A4VhP8M8fQZxP4h7rB6mP6eM8w2pJkYh7QdZk7V4r2x",
  lastValidBlockHeight: 284512337,
  instructionCount: 3,
  requiredSigners: ["3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE"],
  validator: "MAS1Dt9qreoRMQ14YQuhg8UTZMMzDdKhmkZMECCzk57",
};
const withdrawResponseExample = {
  kind: "withdraw" as const,
  version: "legacy" as const,
  transactionBase64: "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAIDKmcfsS5XfSOLaLlaBHJry50iH2Ufk2TMz4STC2fHzIcFKkerg3q2DD3Yn8TISmGeKoxSLz+BiP7iQ4pYqXYXsgu8D8C7R8ovdMQRLpSrE8+jxjTl3BfqywPNGiPNfnh8AazZ0ixOauLjpxaRgDCv6MChaoMAZAJg8BnPbZl31jECAgEBBAECAwQCAQEEAgIDBA==",
  sendTo: "base" as const,
  recentBlockhash: "7YH7nE6qj8vH3L9pR5uM2cD1xK4sT8wQ6bN3fJ2mP9z",
  lastValidBlockHeight: 284512451,
  instructionCount: 2,
  requiredSigners: ["3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE"],
  validator: "MAS1Dt9qreoRMQ14YQuhg8UTZMMzDdKhmkZMECCzk57",
};
const baseBalanceResponseExample = {
  address: "Bt9oNR5cCtnfuMmXgWELd6q5i974PdEMQDUE55nBC57L",
  mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
  ata: "3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE",
  location: "base" as const,
  balance: "1000000",
};
const privateBalanceResponseExample = {
  address: "Bt9oNR5cCtnfuMmXgWELd6q5i974PdEMQDUE55nBC57L",
  mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
  ata: "3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE",
  location: "ephemeral" as const,
  balance: "1000000",
};

export const depositRoute = createRoute({
  path: "/v1/spl/deposit",
  method: "post",
  tags,
  description: "Deposit SPL tokens from Solana into an ephemeral rollup.",
  request: {
    body: jsonContentRequired(depositRequestSchema, "Deposit request"),
  },
  responses: {
    200: jsonContent(transactionResponseSchema, "Unsigned serialized transaction", depositResponseExample),
    400: jsonContent(errorResponseSchema, "Build error"),
    422: jsonContent(validationErrorResponseSchema, "Validation error"),
  },
});

export const withdrawRoute = createRoute({
  path: "/v1/spl/withdraw",
  method: "post",
  tags,
  description: "Withdraw SPL tokens from an ephemeral rollup back to Solana.",
  request: {
    body: jsonContentRequired(withdrawRequestSchema, "Withdraw request"),
  },
  responses: {
    200: jsonContent(transactionResponseSchema, "Unsigned serialized transaction", withdrawResponseExample),
    400: jsonContent(errorResponseSchema, "Build error"),
    422: jsonContent(validationErrorResponseSchema, "Validation error"),
  },
});

export const transferRoute = createRoute({
  path: "/v1/spl/transfer",
  method: "post",
  tags,
  description: "Transfer SPL tokens publicly or privately trough an ephemeral rollup.",
  request: {
    body: jsonContentRequired(transferRequestSchema, "Transfer request"),
  },
  responses: {
    200: jsonContent(transactionResponseSchema, "Unsigned serialized transaction"),
    400: jsonContent(errorResponseSchema, "Build error"),
    422: jsonContent(validationErrorResponseSchema, "Validation error"),
  },
});

export const balanceRoute = createRoute({
  path: "/v1/spl/balance",
  method: "get",
  tags,
  description: "Get the balance for the owner's ATA on the base RPC.",
  request: {
    query: balanceQuerySchema,
  },
  responses: {
    200: jsonContent(balanceResponseSchema, "Base-chain token balance", baseBalanceResponseExample),
    400: jsonContent(errorResponseSchema, "Query error"),
    422: jsonContent(validationErrorResponseSchema, "Validation error"),
  },
});

export const privateBalanceRoute = createRoute({
  path: "/v1/spl/private-balance",
  method: "get",
  tags,
  description: "Get the balance for the owner's ATA on the ephemeral RPC.",
  request: {
    query: balanceQuerySchema,
  },
  responses: {
    200: jsonContent(balanceResponseSchema, "Ephemeral token balance", privateBalanceResponseExample),
    400: jsonContent(errorResponseSchema, "Query error"),
    422: jsonContent(validationErrorResponseSchema, "Validation error"),
  },
});
