import { Keypair } from "@solana/web3.js";
import nacl from "tweetnacl";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AppBindings } from "../env";
import { digest } from "./config";
import { PaymentMerchants } from "./merchants";
import type { PaymentLink } from "./schemas";

class MemoryStorage {
  records = new Map<string, unknown>();

  async get<T>(key: string): Promise<T | undefined> {
    return structuredClone(this.records.get(key)) as T | undefined;
  }

  async put(key: string, value: unknown) {
    this.records.set(key, structuredClone(value));
  }

  async delete(key: string) {
    return this.records.delete(key);
  }

  async transaction<T>(callback: (storage: MemoryStorage) => Promise<T>): Promise<T> {
    const snapshot = structuredClone(this.records);
    try {
      return await callback(this);
    } catch (error) {
      this.records = snapshot;
      throw error;
    }
  }
}

const env: AppBindings = {
  PAYMENTS_PUBLIC_URL: "https://payments.merchant.test",
  PAYMENTS_SECRET: "test-only-checkout-secret-with-32-characters",
  PAYMENT_MERCHANTS: {} as DurableObjectNamespace,
  PAYMENT_LEDGER: {} as DurableObjectNamespace,
};
const wallet = Keypair.generate();
const otherWallet = Keypair.generate();

type Challenge = { id: string; wallet: string; purpose: string; message: string; expiresAt: number };

function signature(message: string, signer = wallet) {
  return Buffer.from(nacl.sign.detached(new TextEncoder().encode(message), signer.secretKey)).toString("base64");
}

async function call(actor: PaymentMerchants, path: string, body: unknown) {
  const response = await actor.fetch(new Request(`https://internal${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  }));
  return { status: response.status, body: await response.json() as any };
}

async function challenge(actor: PaymentMerchants, purpose: "register" | "rotate-key" = "register") {
  const response = await call(actor, "/challenge", { wallet: wallet.publicKey.toBase58(), purpose });
  expect(response.status).toBe(200);
  return response.body as Challenge;
}

function proof(challenge: Challenge) {
  return { wallet: challenge.wallet, challengeId: challenge.id, signature: signature(challenge.message) };
}

async function register(actor: PaymentMerchants) {
  const pending = await challenge(actor);
  const result = await call(actor, "/register", proof(pending));
  expect(result.status).toBe(200);
  return { challenge: pending, ...result.body } as { challenge: Challenge; merchantId: string; wallet: string; apiKey: string };
}

describe("wallet-authenticated merchant registry", () => {
  let storage: MemoryStorage;
  let actor: PaymentMerchants;
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-09-19T12:00:00Z"));
    storage = new MemoryStorage();
    actor = new PaymentMerchants({ storage } as unknown as DurableObjectState, env);
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("binds the signed challenge to the configured origin, wallet, purpose, random nonce and expiry", async () => {
    const pending = await challenge(actor);
    expect(pending.wallet).toBe(wallet.publicKey.toBase58());
    expect(pending.purpose).toBe("register");
    expect(pending.expiresAt).toBe(Date.now() + 300_000);
    expect(pending.message).toBe([
      "https://payments.merchant.test requests merchant register",
      `Wallet: ${wallet.publicKey.toBase58()}`,
      `Nonce: ${pending.id}`,
      `Expires: ${new Date(pending.expiresAt).toISOString()}`,
    ].join("\n"));
    expect(pending.id).toMatch(/^[0-9a-f-]{36}$/);
    expect(await challenge(actor)).toEqual(pending);
    vi.advanceTimersByTime(300_001);
    expect((await challenge(actor)).id).not.toBe(pending.id);
  });

  it("registers with a real wallet signature and stores only the API key hash", async () => {
    const merchant = await register(actor);
    expect(merchant.merchantId).toBe(wallet.publicKey.toBase58());
    expect(merchant.wallet).toBe(wallet.publicKey.toBase58());
    expect(merchant.apiKey).toMatch(/^mb_[A-Za-z0-9_-]{43}$/);
    expect(await storage.get("merchant")).toEqual({
      id: merchant.merchantId,
      wallet: merchant.wallet,
      apiKeyHash: digest(merchant.apiKey),
      createdAt: new Date().toISOString(),
    });
    expect(JSON.stringify([...storage.records])).not.toContain(merchant.apiKey);
    expect(await storage.get("challenge:register")).toBeUndefined();
    expect(await call(actor, "/authenticate", { apiKey: merchant.apiKey })).toEqual({
      status: 200, body: { merchantId: merchant.merchantId, wallet: merchant.wallet },
    });
    expect((await call(actor, "/authenticate", { apiKey: `${merchant.apiKey}wrong` })).status).toBe(401);
  });

  it("rejects replay even after actor restart and concurrent registration retries", async () => {
    const pending = await challenge(actor);
    const results = await Promise.all([
      call(actor, "/register", proof(pending)),
      call(actor, "/register", proof(pending)),
    ]);
    expect(results.map(result => result.status).sort()).toEqual([200, 401]);
    actor = new PaymentMerchants({ storage } as unknown as DurableObjectState, env);
    expect((await call(actor, "/register", proof(pending))).body.error.code).toBe("INVALID_MERCHANT_CHALLENGE");
  });

  it("rejects expired and unknown challenges", async () => {
    const pending = await challenge(actor);
    expect((await call(actor, "/register", { ...proof(pending), challengeId: crypto.randomUUID() })).status).toBe(401);
    vi.advanceTimersByTime(300_000);
    const expired = await call(actor, "/register", proof(pending));
    expect(expired.body.error.code).toBe("INVALID_MERCHANT_CHALLENGE");
    expect(await storage.get("merchant")).toBeUndefined();
  });

  it("rejects a different wallet, forged signature, changed origin and noncanonical signature encoding", async () => {
    const pending = await challenge(actor);
    const mismatched = await call(actor, "/register", { ...proof(pending), wallet: otherWallet.publicKey.toBase58() });
    expect(mismatched.body.error.code).toBe("INVALID_MERCHANT_CHALLENGE");
    for (const invalid of [
      signature(pending.message, otherWallet),
      signature(pending.message.replace("payments.merchant.test", "phishing.test")),
      `${signature(pending.message)}\n`,
      Buffer.alloc(64).toString("base64"),
    ]) {
      const result = await call(actor, "/register", { ...proof(pending), signature: invalid });
      expect(result.body.error.code).toBe("INVALID_MERCHANT_SIGNATURE");
    }
    // Invalid attempts do not consume the legitimate wallet's challenge.
    expect((await call(actor, "/register", proof(pending))).status).toBe(200);
  });

  it("requires explicit wallet-signed rotation, invalidates the old key and preserves merchant identity", async () => {
    const merchant = await register(actor);
    const originalRecord = await storage.get<{ createdAt: string }>("merchant");
    const duplicate = await challenge(actor);
    expect((await call(actor, "/register", proof(duplicate))).body.error.code).toBe("MERCHANT_EXISTS");
    const recovery = await challenge(actor, "rotate-key");
    expect(recovery.message).toContain("merchant rotate-key");
    expect(recovery.id).not.toBe(duplicate.id);
    expect((await call(actor, "/rotate-key", proof(duplicate))).status).toBe(401);
    expect((await call(actor, "/register", proof(recovery))).status).toBe(401);
    vi.advanceTimersByTime(1_000);
    const rotated = await call(actor, "/rotate-key", proof(recovery));
    expect(rotated.status).toBe(200);
    expect(rotated.body.merchantId).toBe(merchant.merchantId);
    expect(rotated.body.apiKey).not.toBe(merchant.apiKey);
    expect((await storage.get<{ createdAt: string }>("merchant"))?.createdAt).toBe(originalRecord?.createdAt);
    expect((await call(actor, "/authenticate", { apiKey: merchant.apiKey })).status).toBe(401);
    expect((await call(actor, "/authenticate", { apiKey: rotated.body.apiKey })).status).toBe(200);
    expect((await call(actor, "/rotate-key", proof(recovery))).status).toBe(401);
    expect(JSON.stringify([...storage.records])).not.toContain(rotated.body.apiKey);
  });

  it("does not register a missing merchant through key recovery", async () => {
    const recovery = await challenge(actor, "rotate-key");
    expect((await call(actor, "/rotate-key", proof(recovery))).body.error.code).toBe("MERCHANT_NOT_FOUND");
    expect((await call(actor, "/authenticate", { apiKey: "mb_unknown" })).status).toBe(401);
  });

  it("creates a public offer once and rejects changed terms for the same link reference", async () => {
    const merchant = await register(actor);
    const link: PaymentLink = {
      id: digest("credit-bundle"),
      merchantId: merchant.merchantId,
      recipient: merchant.wallet,
      externalReference: "credit-bundle",
      amount: "1000000",
      cluster: "devnet-private",
      description: "100 credits",
      expiresInSeconds: 3600,
      createdAt: new Date().toISOString(),
    };
    const creationHash = digest(JSON.stringify(link));
    const [first, repeated] = await Promise.all([
      call(actor, "/create-link", { link, creationHash }),
      call(actor, "/create-link", { link, creationHash }),
    ]);
    expect(first).toEqual({ status: 200, body: link });
    expect(repeated).toEqual(first);
    const changed = { ...link, amount: "2000000" };
    const conflict = await call(actor, "/create-link", { link: changed, creationHash: digest(JSON.stringify(changed)) });
    expect(conflict.body.error.code).toBe("PAYMENT_LINK_CONFLICT");
    expect((await call(actor, "/get-link", { id: link.id })).body).toEqual(link);
    expect((await call(actor, "/get-link", { id: digest("missing") })).body.error.code).toBe("PAYMENT_LINK_NOT_FOUND");
  });
});
