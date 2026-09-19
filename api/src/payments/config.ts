import { sha256 } from "@noble/hashes/sha256";
import { hmac } from "@noble/hashes/hmac";
import type { AppBindings } from "../env";
import { ApiError } from "../lib/errors";

export function digest(value: string): string {
  return Buffer.from(sha256(new TextEncoder().encode(value))).toString("hex");
}

export function paymentConfig(env: AppBindings) {
  if (!env.PAYMENTS_PUBLIC_URL || !env.PAYMENTS_SECRET || env.PAYMENTS_SECRET.length < 32
    || !env.PAYMENT_MERCHANTS || !env.PAYMENT_LEDGER) {
    throw new ApiError(503, "PAYMENTS_UNAVAILABLE", "Payment checkout configuration is incomplete");
  }
  let url: URL;
  try {
    url = new URL(env.PAYMENTS_PUBLIC_URL);
  } catch {
    throw new ApiError(503, "PAYMENTS_UNAVAILABLE", "PAYMENTS_PUBLIC_URL must be a public HTTPS origin");
  }
  if ((url.protocol !== "https:" && !(url.protocol === "http:" && ["localhost", "127.0.0.1"].includes(url.hostname)))
    || url.username || url.password || url.search || url.hash || url.pathname !== "/") {
    throw new ApiError(503, "PAYMENTS_UNAVAILABLE", "PAYMENTS_PUBLIC_URL must be a public HTTPS origin");
  }
  return { origin: url.origin, realm: url.host, merchants: env.PAYMENT_MERCHANTS, ledger: env.PAYMENT_LEDGER };
}

export function checkoutToken(env: AppBindings, id: string): string {
  paymentConfig(env);
  return Buffer.from(hmac(sha256, new TextEncoder().encode(env.PAYMENTS_SECRET!), `checkout:${id}`)).toString("base64url");
}

export async function callObject<T>(namespace: DurableObjectNamespace, name: string, path: string, body: unknown): Promise<T> {
  const response = await namespace.get(namespace.idFromName(name)).fetch(`https://internal${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const result = await response.json() as T & { error?: { code: string; message: string; details?: unknown } };
  if (!response.ok) {
    throw new ApiError(response.status, result.error?.code ?? "PAYMENT_ERROR", result.error?.message ?? "Payment operation failed", result.error?.details);
  }
  return result;
}
