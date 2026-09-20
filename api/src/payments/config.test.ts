import { describe, expect, it } from "vitest";
import type { AppBindings } from "../env";
import { getEnv } from "../env";
import { paymentConfig } from "./config";

const configured: AppBindings = {
  PAYMENTS_PUBLIC_URL: "https://payments.example.com",
  PAYMENTS_SECRET: "test-only-server-secret-at-least-32-characters",
  PAYMENT_MERCHANTS: {} as DurableObjectNamespace,
  PAYMENT_LEDGER: {} as DurableObjectNamespace,
};

describe("payment configuration", () => {
  it("keeps invalid checkout configuration from breaking legacy RPC configuration", () => {
    const env = {
      ...configured, PAYMENTS_PUBLIC_URL: "not a URL",
      BASE_RPC_URL: "https://base.example.com", EPHEMERAL_RPC_URL: "https://er.example.com",
    };
    expect(getEnv(env)).toMatchObject({ BASE_RPC_URL: env.BASE_RPC_URL, EPHEMERAL_RPC_URL: env.EPHEMERAL_RPC_URL });
    expect(() => paymentConfig(env)).toThrow(expect.objectContaining({ status: 503, code: "PAYMENTS_UNAVAILABLE" }));
  });

  it.each([
    "not a URL", "https://", "http://payments.example.com", "https://user:secret@payments.example.com",
    "https://payments.example.com/api", "https://payments.example.com?token=secret", "https://payments.example.com#fragment",
  ])("returns a sanitized unavailable error for invalid public origin %j", (origin) => {
    expect(() => paymentConfig({ ...configured, PAYMENTS_PUBLIC_URL: origin })).toThrow(expect.objectContaining({
      status: 503,
      code: "PAYMENTS_UNAVAILABLE",
      message: "PAYMENTS_PUBLIC_URL must be a public HTTPS origin",
    }));
  });

  it.each(["https://payments.example.com", "http://localhost:8787", "http://127.0.0.1:8787"])("accepts configured origin %j", (origin) => {
    expect(paymentConfig({ ...configured, PAYMENTS_PUBLIC_URL: origin }).origin).toBe(origin);
  });

  it("requires both state bindings and a sufficiently long server secret", () => {
    for (const missing of [
      { PAYMENT_LEDGER: undefined },
      { PAYMENT_MERCHANTS: undefined },
      { PAYMENTS_SECRET: "short" },
    ]) {
      expect(() => paymentConfig({ ...configured, ...missing })).toThrow(expect.objectContaining({ status: 503, code: "PAYMENTS_UNAVAILABLE" }));
    }
  });
});
