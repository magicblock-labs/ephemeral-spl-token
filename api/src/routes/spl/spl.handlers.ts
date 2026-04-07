import type { RouteHandler } from "@hono/zod-openapi";
import { z } from "@hono/zod-openapi";

import { getEnv, type AppBindings } from "../../env";
import {
  buildDepositTransaction,
  buildInitializeMintTransaction,
  buildTransferTransaction,
  buildWithdrawTransaction,
  getBaseBalance,
  getMintInitializationStatus,
  getPrivateBalance,
} from "../../lib/solana";
import {
  balanceRoute,
  challengeRoute,
  depositRoute,
  initializeMintRoute,
  loginRoute,
  mintInitializationRoute,
  privateBalanceRoute,
  transferRoute,
  withdrawRoute,
} from "./spl.routes";
import {
  balanceQuerySchema,
  challengeQuerySchema,
  depositRequestSchema,
  initializeMintRequestSchema,
  loginQuerySchema,
  mintInitializationQuerySchema,
  transferRequestSchema,
  withdrawRequestSchema,
} from "./spl.schemas";
import { getChallenge, login, parseAuthToken } from "../../lib/auth";

type RouteEnv = { Bindings: AppBindings };
type BackgroundScheduler = {
  waitUntil: (promise: Promise<unknown>) => void;
};

function getBackgroundScheduler(c: { executionCtx: BackgroundScheduler }) {
  try {
    return c.executionCtx;
  }
  catch {
    return undefined;
  }
}

export const depositHandler: RouteHandler<typeof depositRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const body = c.req.valid("json") as z.infer<typeof depositRequestSchema>;
  const response = await buildDepositTransaction(env, body);
  return c.json(response, 200);
};

export const withdrawHandler: RouteHandler<typeof withdrawRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const body = c.req.valid("json") as z.infer<typeof withdrawRequestSchema>;
  const response = await buildWithdrawTransaction(env, body);
  return c.json(response, 200);
};

export const initializeMintHandler: RouteHandler<typeof initializeMintRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const body = c.req.valid("json") as z.infer<typeof initializeMintRequestSchema>;
  const response = await buildInitializeMintTransaction(env, body);
  return c.json(response, 200);
};

export const transferHandler: RouteHandler<typeof transferRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const body = c.req.valid("json") as z.infer<typeof transferRequestSchema>;
  const authToken = parseAuthToken(c.req.header());
  const response = await buildTransferTransaction(env, body, authToken);
  return c.json(response, 200);
};

export const balanceHandler: RouteHandler<typeof balanceRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const query = c.req.valid("query") as z.infer<typeof balanceQuerySchema>;
  const response = await getBaseBalance(env, query);
  return c.json(response, 200);
};

export const privateBalanceHandler: RouteHandler<typeof privateBalanceRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const query = c.req.valid("query") as z.infer<typeof balanceQuerySchema>;
  const authToken = parseAuthToken(c.req.header());
  if (!authToken) {
    return c.json({ error: { code: "MISSING_AUTH_TOKEN", message: "authToken is required for private balance" } }, 400);
  }
  const response = await getPrivateBalance(env, query, authToken);
  return c.json(response, 200);
};

export const mintInitializationHandler: RouteHandler<typeof mintInitializationRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const query = c.req.valid("query") as z.infer<typeof mintInitializationQuerySchema>;
  const response = await getMintInitializationStatus(
    env,
    query,
    getBackgroundScheduler(c as { executionCtx: BackgroundScheduler }),
  );
  return c.json(response, 200);
};

export const challengeHandler: RouteHandler<typeof challengeRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const query = c.req.valid("query") as z.infer<typeof challengeQuerySchema>;
  const response = await getChallenge(env, query);
  return c.json(response, 200);
};

export const loginHandler: RouteHandler<typeof loginRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const body = c.req.valid("json") as z.infer<typeof loginQuerySchema>;
  const response = await login(env, body);
  return c.json(response, 200);
};
