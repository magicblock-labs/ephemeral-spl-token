import type { RouteHandler } from "@hono/zod-openapi";

import { getEnv, type AppBindings } from "../../env";
import {
  buildDepositTransaction,
  buildInitializeMintTransaction,
  buildUpdateStealthPoolTransaction,
  buildTransferTransaction,
  buildUndelegateEphemeralAtaTransaction,
  buildWithdrawTransaction,
  ensureTransferQueueCrank,
  getBaseBalance,
  getMintInitializationStatus,
  getPrivateBalance,
  getStealthPoolStatus,
} from "../../lib/solana";
import {
  balanceRoute,
  challengeRoute,
  depositRoute,
  initializeMintRoute,
  loginRoute,
  mintInitializationRoute,
  privateBalanceRoute,
  stealthPoolRoute,
  stealthPoolStatusRoute,
  transferQueueEnsureCrankRoute,
  transferRoute,
  undelegateEphemeralAtaRoute,
  withdrawRoute,
} from "./spl.routes";
import {
  BalanceRequest,
  ChallengeRequest,
  DepositRequest,
  InitializeMintRequest,
  LoginRequest,
  MintInitializationRequest,
  StealthPoolRequest,
  StealthPoolStatusRequest,
  TransferRequest,
  TransferQueueEnsureCrankRequest,
  UndelegateEphemeralAtaRequest,
  WithdrawRequest,
} from "./spl.schemas";
import { getChallenge, login, parseAuthToken, requireAuthTokenFor, splitAuthToken } from "../../lib/auth";

type RouteEnv = { Bindings: AppBindings };
type BackgroundScheduler = {
  waitUntil: (promise: Promise<unknown>) => void;
};

function getBackgroundScheduler(c: { executionCtx: BackgroundScheduler }) {
  try {
    return c.executionCtx;
  } catch {
    return undefined;
  }
}

export const depositHandler: RouteHandler<typeof depositRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const body = c.req.valid("json") as DepositRequest;
  const response = await buildDepositTransaction(env, body);
  return c.json(response, 200);
};

export const withdrawHandler: RouteHandler<typeof withdrawRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const body = c.req.valid("json") as WithdrawRequest;
  const response = await buildWithdrawTransaction(env, body);
  return c.json(response, 200);
};

export const initializeMintHandler: RouteHandler<typeof initializeMintRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const body = c.req.valid("json") as InitializeMintRequest;
  const response = await buildInitializeMintTransaction(env, body);
  return c.json(response, 200);
};

export const transferHandler: RouteHandler<typeof transferRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const body = c.req.valid("json") as TransferRequest;
  const rawAuthToken = parseAuthToken(c.req.header());
  // Spending a shielded balance is scoped to `from`; other routes only unwrap.
  const authToken = rawAuthToken === undefined
    ? undefined
    : body.fromBalance === "ephemeral"
      ? requireAuthTokenFor(body.from, rawAuthToken)
      : splitAuthToken(rawAuthToken).token;
  const response = await buildTransferTransaction(env, body, authToken);
  return c.json(response, 200);
};

export const undelegateEphemeralAtaHandler: RouteHandler<typeof undelegateEphemeralAtaRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const body = c.req.valid("json") as UndelegateEphemeralAtaRequest;
  const rawAuthToken = parseAuthToken(c.req.header());
  const authToken = rawAuthToken === undefined
    ? undefined
    : requireAuthTokenFor(body.payer, rawAuthToken);
  const response = await buildUndelegateEphemeralAtaTransaction(env, body, authToken);
  return c.json(response, 200);
};

export const stealthPoolHandler: RouteHandler<typeof stealthPoolRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const body = c.req.valid("json") as StealthPoolRequest;
  const rawAuthToken = parseAuthToken(c.req.header());
  const authToken = rawAuthToken === undefined ? undefined : splitAuthToken(rawAuthToken).token;

  const response = await buildUpdateStealthPoolTransaction(env, body, authToken);
  return c.json(response, 200);
};

export const stealthPoolStatusHandler: RouteHandler<typeof stealthPoolStatusRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const query = c.req.valid("query") as StealthPoolStatusRequest;

  const response = await getStealthPoolStatus(env, query);
  return c.json(response, 200);
};

export const balanceHandler: RouteHandler<typeof balanceRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const query = c.req.valid("query") as BalanceRequest;
  const response = await getBaseBalance(env, query);
  return c.json(response, 200);
};

export const privateBalanceHandler: RouteHandler<typeof privateBalanceRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const query = c.req.valid("query") as BalanceRequest;
  const rawAuthToken = parseAuthToken(c.req.header());

  if (!rawAuthToken) {
    return c.json({ error: { code: "MISSING_AUTH_TOKEN", message: "authToken is required for private balance" } }, 400);
  }
  const authToken = requireAuthTokenFor(query.address, rawAuthToken);
  const response = await getPrivateBalance(env, query, authToken);
  return c.json(response, 200);
};

export const mintInitializationHandler: RouteHandler<typeof mintInitializationRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const query = c.req.valid("query") as MintInitializationRequest;
  const response = await getMintInitializationStatus(
    env,
    query,
    getBackgroundScheduler(c as { executionCtx: BackgroundScheduler }),
  );
  return c.json(response, 200);
};

export const transferQueueEnsureCrankHandler: RouteHandler<typeof transferQueueEnsureCrankRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const body = c.req.valid("json") as TransferQueueEnsureCrankRequest;
  const response = await ensureTransferQueueCrank(env, body);
  return c.json(response, 200);
};

export const challengeHandler: RouteHandler<typeof challengeRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const query = c.req.valid("query") as ChallengeRequest;
  const response = await getChallenge(env, query);
  return c.json(response, 200);
};

export const loginHandler: RouteHandler<typeof loginRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const body = c.req.valid("json") as LoginRequest;
  const response = await login(env, body);
  return c.json(response, 200);
};
