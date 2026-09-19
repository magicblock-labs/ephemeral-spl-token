import { z } from "@hono/zod-openapi";
import type { Context } from "hono";
import type { AppBindings } from "../env";
import { ApiError, errorResponseSchema } from "../lib/errors";
import { callObject, paymentConfig } from "./config";
import { paymentCredential } from "./protocols";
import { idSchema, walletSchema } from "./schemas";

export type Env = { Bindings: AppBindings };
type PaymentAuth = { merchantId?: string; accessToken?: string };
export const jsonResponse = { description: "Payment API response", content: { "application/json": { schema: z.record(z.string(), z.unknown()) } } };
export const responses = {
  200: jsonResponse,
  202: { ...jsonResponse, description: "Payment pending; retry status without creating another charge" },
  400: { description: "Invalid request", content: { "application/json": { schema: errorResponseSchema } } },
  401: { description: "Invalid credentials", content: { "application/json": { schema: errorResponseSchema } } },
  409: { ...jsonResponse, description: "Order conflict or failed payment" },
  503: { description: "Payments unavailable", content: { "application/json": { schema: errorResponseSchema } } },
};
export const body = <T extends z.ZodType>(schema: T) => ({ body: { required: true, content: { "application/json": { schema } } } });

export const paymentParams = z.object({ id: idSchema });

function bearer(c: Context<Env>): string {
  const match = /^Bearer ([A-Za-z0-9_-]{1,256})$/.exec(c.req.header("Authorization") ?? "");
  if (!match) throw new ApiError(401, "PAYMENT_UNAUTHORIZED", "Bearer credential required");
  return match[1];
}

export async function merchant(c: Context<Env>) {
  const config = paymentConfig(c.env);
  const merchantId = walletSchema.parse(c.req.header("X-Merchant-Id"));
  return callObject<{ merchantId: string; wallet: string }>(config.merchants, merchantId, "/authenticate", { apiKey: bearer(c) });
}

export async function authorization(c: Context<Env>): Promise<PaymentAuth> {
  if (c.req.header("X-Merchant-Id")) return { merchantId: (await merchant(c)).merchantId };
  return { accessToken: bearer(c) };
}

export function ledger<T>(c: Context<Env>, id: string, action: string, input: unknown): Promise<T> {
  idSchema.parse(id);
  return callObject<T>(paymentConfig(c.env).ledger, id, action, input);
}

export function protocolInput<T>(operation: () => T): T {
  try {
    return operation();
  } catch {
    throw new ApiError(400, "INVALID_PAYMENT_CREDENTIAL", "Payment credential does not match checkout");
  }
}

export function protocolCredential(headers: Headers, protocol: "x402" | "mpp") {
  return protocolInput(() => {
    const credential = paymentCredential(headers);
    if (credential && credential.protocol !== protocol) throw new Error("Wrong payment protocol");
    return credential?.value;
  });
}
