import { createRoute, OpenAPIHono } from "@hono/zod-openapi";
import type { RouteHandler } from "@hono/zod-openapi";

import { getEnv, type AppBindings } from "../env";
import { openApiDefaultHook } from "../lib/create-app";
import { errorResponseSchema, validationErrorResponseSchema } from "../lib/errors";
import { jsonContent, jsonContentRequired } from "../lib/openapi";
import { sendSignedTransaction } from "../lib/solana";
import { optionalAuthTokenSchema } from "../schema";
import { parseAuthToken, splitAuthToken } from "../lib/auth";
import {
  SendTransactionRequest,
  sendTransactionRequestSchema,
  SendTransactionResponse,
  sendTransactionResponseSchema,
} from "./transaction.schemas";

const tags = ["Transaction"];
const sendTransactionRequestExample: SendTransactionRequest = {
  transactionBase64: "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAA==",
  sendTo: "base",
};
const sendTransactionResponseExample: SendTransactionResponse = {
  signature: "5Nf6YwU8z2Y4QyQZ9pQ6xT2P7mK3sV4nY8rD1hJ5kL9m",
  sendTo: "base",
  confirmed: false,
  confirmationRpcEndpoint: "https://api.mainnet-beta.solana.com",
  confirmationRequiresAuthToken: false,
};

const sendTransactionRoute = createRoute({
  path: "/v1/transaction/send",
  method: "post",
  tags,
  description: "Submit a signed serialized transaction to the base or ephemeral RPC selected by `sendTo`.",
  request: {
    body: jsonContentRequired(sendTransactionRequestSchema, "Send transaction request", sendTransactionRequestExample),
    headers: optionalAuthTokenSchema,
  },
  responses: {
    200: jsonContent(sendTransactionResponseSchema, "Submitted transaction", sendTransactionResponseExample),
    400: jsonContent(errorResponseSchema, "Send error"),
    422: jsonContent(validationErrorResponseSchema, "Validation error"),
    502: jsonContent(errorResponseSchema, "RPC error"),
  },
});

type RouteEnv = { Bindings: AppBindings };

const sendTransactionHandler: RouteHandler<typeof sendTransactionRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const body = c.req.valid("json") as SendTransactionRequest;
  const rawAuthToken = parseAuthToken(c.req.header());
  const authToken = rawAuthToken === undefined ? undefined : splitAuthToken(rawAuthToken).token;
  const response = await sendSignedTransaction(env, body, authToken);
  return c.json(response, 200);
};

const app = new OpenAPIHono<{ Bindings: AppBindings }>({
  defaultHook: openApiDefaultHook,
});

app.openapi(sendTransactionRoute, sendTransactionHandler);

export default app;
