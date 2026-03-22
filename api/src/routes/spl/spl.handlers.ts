import type { RouteHandler } from "@hono/zod-openapi";
import { z } from "@hono/zod-openapi";

import { getEnv, type AppBindings } from "../../env";
import {
  buildDepositTransaction,
  buildTransferTransaction,
  buildWithdrawTransaction,
  getBaseBalance,
  getPrivateBalance,
} from "../../lib/solana";
import {
  balanceRoute,
  depositRoute,
  privateBalanceRoute,
  transferRoute,
  withdrawRoute,
} from "./spl.routes";
import {
  balanceQuerySchema,
  depositRequestSchema,
  transferRequestSchema,
  withdrawRequestSchema,
} from "./spl.schemas";

type RouteEnv = { Bindings: AppBindings };

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

export const transferHandler: RouteHandler<typeof transferRoute, RouteEnv> = async (c) => {
  const env = getEnv(c.env);
  const body = c.req.valid("json") as z.infer<typeof transferRequestSchema>;
  const response = await buildTransferTransaction(env, body);
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
  const response = await getPrivateBalance(env, query);
  return c.json(response, 200);
};
