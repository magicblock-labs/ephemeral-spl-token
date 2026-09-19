import { Buffer } from "buffer";
import { Keypair, PublicKey, Transaction, TransactionInstruction } from "@solana/web3.js";
import nacl from "tweetnacl";
import { beforeEach, describe, expect, it, vi } from "vitest";
import app from "../app";
import type { AppBindings } from "../env";
import { getPaymentStatus, preparePayment, submitPayment } from "./chain";
import { createPaymentCredential } from "./client";
import { PaymentLedger } from "./ledger";
import { PaymentMerchants } from "./merchants";
import { decodePaymentJson, decodeX402Payload, encodePaymentJson } from "./protocols";
import type { PaymentTerms, PaymentView, PreparedPayment } from "./types";

// Exercise actual HTTP, authentication, ledger and signature validation; only RPC is mocked.
vi.mock("./chain", async original => ({
  ...await original<typeof import("./chain")>(),
  preparePayment: vi.fn(), submitPayment: vi.fn(), getPaymentStatus: vi.fn(),
}));

class Storage {
  records = new Map<string, unknown>();
  alarmTime: number | null = null;
  writes = 0;
  async get<T>(key: string): Promise<T | undefined> { return structuredClone(this.records.get(key)) as T | undefined; }
  async put(key: string | Record<string, unknown>, value?: unknown) {
    this.writes++;
    const entries: [string, unknown][] = typeof key === "string" ? [[key, value]] : Object.entries(key);
    for (const [name, entry] of entries) {
      this.records.set(name, structuredClone(entry));
    }
  }

  async delete(key: string) { return this.records.delete(key); }
  async setAlarm(time: number) { this.alarmTime = time; }
  async deleteAlarm() { this.alarmTime = null; }
  async transaction<T>(callback: (storage: Storage) => Promise<T>): Promise<T> {
    const records = structuredClone(this.records);
    const alarm = this.alarmTime;
    try {
      return await callback(this);
    } catch (error) {
      this.records = records;
      this.alarmTime = alarm;
      throw error;
    }
  }
}

function namespace(create: (storage: Storage) => { fetch(request: Request): Promise<Response> }) {
  const actors = new Map<string, { actor: ReturnType<typeof create>; storage: Storage }>();
  return {
    actors,
    binding: {
      idFromName: (name: string) => ({ name }),
      get: (id: { name: string }) => ({
        fetch: (url: string, init: RequestInit) => {
          let item = actors.get(id.name);
          if (!item) {
            const storage = new Storage();
            item = { actor: create(storage), storage };
            actors.set(id.name, item);
          }
          return item.actor.fetch(new Request(url, init));
        },
      }),
    } as unknown as DurableObjectNamespace,
  };
}

const validator = Keypair.generate().publicKey.toBase58();
const rpcSecret = "rpc-key-never-in-response";
const tokenProgram = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const associatedTokenProgram = new PublicKey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

function preparedTransaction(terms: PaymentTerms): PreparedPayment {
  const payer = new PublicKey(terms.payer);
  const mint = new PublicKey(terms.mint);
  const ata = (owner: string) => PublicKey.findProgramAddressSync([
    new PublicKey(owner).toBuffer(), tokenProgram.toBuffer(), mint.toBuffer(),
  ], associatedTokenProgram)[0];
  const data = Buffer.alloc(9);
  data[0] = 3;
  data.writeBigUInt64LE(BigInt(terms.amount), 1);
  const transaction = new Transaction({ feePayer: payer, recentBlockhash: Keypair.generate().publicKey.toBase58() }).add(
    new TransactionInstruction({
      programId: tokenProgram,
      keys: [
        { pubkey: ata(terms.payer), isSigner: false, isWritable: true },
        { pubkey: ata(terms.recipient), isSigner: false, isWritable: true },
        { pubkey: payer, isSigner: true, isWritable: false },
      ],
      data,
    }),
    new TransactionInstruction({ programId: new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"), keys: [], data: Buffer.from(`payment:${terms.id}`) }),
  );
  return {
    transactionBase64: transaction.serialize({ requireAllSignatures: false }).toString("base64"),
    messageBase64: transaction.serializeMessage().toString("base64"),
    recentBlockhash: transaction.recentBlockhash!, lastValidBlockHeight: 100, validator,
    rpcEndpoint: `https://private-er.test?key=${rpcSecret}`,
  };
}

type Merchant = { merchantId: string; apiKey: string; wallet: string };
type Checkout = { payment: PaymentView; accessToken: string; checkoutUrl: string; paymentUrls: { x402: string; mpp: string } };

function fixture() {
  const env: AppBindings = {
    BASE_RPC_URL: "https://base.test", EPHEMERAL_RPC_URL: "https://er.test",
    BASE_DEVNET_RPC_URL: "https://devnet-base.test", EPHEMERAL_DEVNET_RPC_URL: "https://devnet-er.test",
    EPHEMERAL_DEVNET_TEE_RPC_URL: `https://private-er.test?key=${rpcSecret}`,
    PAYMENTS_PUBLIC_URL: "https://payments.test", PAYMENTS_SECRET: "test-checkout-secret-longer-than-thirty-two-characters",
    PAYMENTS_RPC_AUTH_SECRET_KEY: JSON.stringify(Array.from(Keypair.generate().secretKey)),
  };
  const merchants = namespace(storage => new PaymentMerchants({ storage } as unknown as DurableObjectState, env));
  const ledger = namespace(storage => new PaymentLedger({ storage } as unknown as DurableObjectState, env));
  env.PAYMENT_MERCHANTS = merchants.binding;
  env.PAYMENT_LEDGER = ledger.binding;
  const request = (path: string, method = "GET", body?: unknown, headers: HeadersInit = {}) => {
    const requestHeaders = new Headers(headers);
    requestHeaders.set("Content-Type", "application/json");
    return app.request(path, {
      method, headers: requestHeaders,
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    }, env);
  };
  const merchantHeaders = (merchant: Merchant) => ({ "Authorization": `Bearer ${merchant.apiKey}`, "X-Merchant-Id": merchant.merchantId });
  const buyerHeaders = (checkout: Checkout) => ({ Authorization: `Bearer ${checkout.accessToken}` });
  const register = async (wallet = Keypair.generate()): Promise<Merchant> => {
    const response = await request("/v1/merchants/challenge", "POST", { wallet: wallet.publicKey.toBase58() });
    expect(response.status).toBe(200);
    const challenge = await response.json() as { id: string; message: string };
    const result = await request("/v1/merchants", "POST", {
      wallet: wallet.publicKey.toBase58(), challengeId: challenge.id,
      signature: Buffer.from(nacl.sign.detached(new TextEncoder().encode(challenge.message), wallet.secretKey)).toString("base64"),
    });
    expect(result.status).toBe(200);
    return result.json() as Promise<Merchant>;
  };
  const create = async (merchant: Merchant, buyer: Keypair, extra: Record<string, unknown> = {}): Promise<Checkout> => {
    const response = await request("/v1/payments", "POST", {
      payer: buyer.publicKey.toBase58(), amount: "1000000", cluster: "devnet-private", externalReference: "order_123",
      resource: "https://merchant.test/credits", requestHash: "a".repeat(64), ...extra,
    }, merchantHeaders(merchant));
    expect(response.status).toBe(200);
    return response.json() as Promise<Checkout>;
  };
  const proof = async (checkout: Checkout, buyer: Keypair, protocol: "x402" | "mpp") => {
    const challenge = await request(checkout.paymentUrls[protocol], "POST", undefined, buyerHeaders(checkout));
    expect(challenge.status).toBe(402);
    if (protocol === "x402") {
      expect(challenge.headers.get("PAYMENT-REQUIRED")).toBeTruthy();
      expect(challenge.headers.get("WWW-Authenticate")).toBeNull();
      expect(await challenge.clone().json()).toMatchObject({ x402Version: 2 });
    } else {
      expect(challenge.headers.get("WWW-Authenticate")).toMatch(/^Payment /);
      expect(challenge.headers.get("PAYMENT-REQUIRED")).toBeNull();
      expect(await challenge.clone().json()).toMatchObject({ paymentId: checkout.payment.id, challenge: { method: "magicblock", intent: "charge" } });
    }
    const credential = await createPaymentCredential({
      protocol, headers: challenge.headers, payment: checkout.payment, validator, realm: "payments.test",
      signTransaction: async (transaction) => {
        transaction.sign(buyer);
        return transaction;
      },
    });
    return { credential, headers: { ...buyerHeaders(checkout), [credential.headerName]: credential.headerValue }, challenge };
  };
  return { env, merchants, ledger, request, merchantHeaders, buyerHeaders, register, create, proof };
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(preparePayment).mockImplementation(async (_env, terms) => preparedTransaction(terms));
  vi.mocked(submitPayment).mockResolvedValue();
  vi.mocked(getPaymentStatus).mockResolvedValue({ state: "paid", slot: 42 });
});

describe("payment HTTP integration", () => {
  it.each(["x402", "mpp"] as const)("registers a merchant, prepares and pays with %s using a real wallet signature", async (protocol) => {
    const f = fixture();
    const merchant = await f.register();
    const buyer = Keypair.generate();
    const checkout = await f.create(merchant, buyer);
    const preparation = await f.request(`${checkout.checkoutUrl}/prepare`, "POST", undefined, f.buyerHeaders(checkout));
    expect(preparation.status).toBe(200);
    const details = await preparation.json();
    expect(details).toMatchObject({ validator, fees: { lamports: "0", tokens: "0" } });
    expect(JSON.stringify(details)).not.toContain(rpcSecret);
    expect(details).not.toHaveProperty("rpcEndpoint");
    const { headers, challenge } = await f.proof(checkout, buyer, protocol);
    expect(challenge.headers.get("Cache-Control")).toBe("no-store");
    const paid = await f.request(checkout.paymentUrls[protocol], "POST", undefined, headers);
    expect(paid.status).toBe(200);
    const result = await paid.json();
    expect(result).toMatchObject({ id: checkout.payment.id, status: "paid", settlement: "ephemeral-rollup", slot: 42 });
    expect(JSON.stringify(result)).not.toContain(rpcSecret);
    const receipt = paid.headers.get(protocol === "x402" ? "PAYMENT-RESPONSE" : "Payment-Receipt");
    expect(receipt).toBeTruthy();
    expect(paid.headers.get(protocol === "x402" ? "Payment-Receipt" : "PAYMENT-RESPONSE")).toBeNull();
    expect(paid.headers.get("Cache-Control")).toBe("no-store");
    expect(decodePaymentJson(receipt!, protocol === "mpp")).toMatchObject(protocol === "x402" ? { success: true } : { status: "success" });
    expect(preparePayment).toHaveBeenCalledOnce();
    expect(submitPayment).toHaveBeenCalledOnce();
    const retry = await f.request(checkout.paymentUrls[protocol], "POST", undefined, headers);
    expect(await retry.json()).toEqual(result);
    expect(submitPayment).toHaveBeenCalledOnce();
  });

  it("shares one settlement across x402 and MPP retries for the same order", async () => {
    const f = fixture();
    const merchant = await f.register();
    const buyer = Keypair.generate();
    const checkout = await f.create(merchant, buyer);
    const x402 = await f.proof(checkout, buyer, "x402");
    const mpp = await f.proof(checkout, buyer, "mpp");
    const first = await f.request(checkout.paymentUrls.x402, "POST", undefined, x402.headers);
    const second = await f.request(checkout.paymentUrls.mpp, "POST", undefined, mpp.headers);
    expect(first.status).toBe(200);
    expect(second.status).toBe(200);
    expect(await first.json()).toEqual(await second.json());
    expect(submitPayment).toHaveBeenCalledOnce();
  });

  it("rejects mixed protocols, mismatched orders and unapproved requirements before broadcast", async () => {
    const f = fixture();
    const merchant = await f.register();
    const buyer = Keypair.generate();
    const checkout = await f.create(merchant, buyer);
    const x402 = await f.proof(checkout, buyer, "x402");
    const mpp = await f.proof(checkout, buyer, "mpp");
    for (const protocol of ["x402", "mpp"] as const) {
      const oppositeHeaders = protocol === "x402" ? mpp.headers : x402.headers;
      const opposite = await f.request(checkout.paymentUrls[protocol], "POST", undefined, oppositeHeaders);
      expect(opposite.status).toBe(400);
      expect(opposite.headers.get("Cache-Control")).toBe("no-store");
      expect((await f.request(checkout.paymentUrls[protocol], "POST", undefined, { ...x402.headers, ...mpp.headers })).status).toBe(400);
    }
    const other = await f.create(merchant, buyer, { externalReference: "order_other" });
    await f.request(`${other.checkoutUrl}/prepare`, "POST", undefined, f.buyerHeaders(other));
    expect((await f.request(other.paymentUrls.x402, "POST", undefined, { ...f.buyerHeaders(other), "PAYMENT-SIGNATURE": x402.credential.headerValue })).status).toBe(400);
    const payload = decodeX402Payload(x402.credential.headerValue).payload;
    payload.accepted.amount = "1";
    expect((await f.request(checkout.paymentUrls.x402, "POST", undefined, { ...f.buyerHeaders(checkout), "PAYMENT-SIGNATURE": encodePaymentJson(payload) })).status).toBe(400);
    expect(submitPayment).not.toHaveBeenCalled();
  });

  it("rejects cross-merchant access, missing or wrong capabilities and credentials from another checkout", async () => {
    const f = fixture();
    const merchant = await f.register();
    const otherMerchant = await f.register();
    const buyer = Keypair.generate();
    const checkout = await f.create(merchant, buyer);
    const invalidHeaders: HeadersInit[] = [{}, { Authorization: "Bearer wrong" }, f.merchantHeaders(otherMerchant)];
    for (const headers of invalidHeaders) {
      expect((await f.request(checkout.checkoutUrl, "GET", undefined, headers)).status).toBe(401);
      for (const url of Object.values(checkout.paymentUrls)) {
        const denied = await f.request(url, "POST", undefined, headers);
        expect(denied.status).toBe(401);
        expect(denied.headers.get("Cache-Control")).toBe("no-store");
        expect(denied.headers.get("WWW-Authenticate")).toBeNull();
        expect(denied.headers.get("PAYMENT-REQUIRED")).toBeNull();
      }
    }
    const other = await f.create(merchant, buyer, { externalReference: "other" });
    expect((await f.request(checkout.checkoutUrl, "GET", undefined, f.buyerHeaders(other))).status).toBe(401);
    expect((await f.request(checkout.checkoutUrl, "GET", undefined, f.merchantHeaders(merchant))).status).toBe(200);
    expect(preparePayment).not.toHaveBeenCalled();
  });

  it.each(["x402", "mpp"] as const)("returns 202 without a success receipt on %s when private RPC confirmation is unavailable", async (protocol) => {
    const f = fixture();
    const merchant = await f.register();
    const buyer = Keypair.generate();
    const checkout = await f.create(merchant, buyer);
    const { headers } = await f.proof(checkout, buyer, protocol);
    vi.mocked(getPaymentStatus).mockRejectedValue(new Error("Private RPC offline"));
    const pending = await f.request(checkout.paymentUrls[protocol], "POST", undefined, headers);
    expect(pending.status).toBe(202);
    expect(await pending.json()).toMatchObject({ status: "pending" });
    expect(pending.headers.get("Payment-Receipt")).toBeNull();
    if (protocol === "x402") {
      expect(decodePaymentJson(pending.headers.get("PAYMENT-RESPONSE")!)).toMatchObject({ success: false, errorReason: "settlement_pending" });
    } else {
      expect(pending.headers.get("PAYMENT-RESPONSE")).toBeNull();
    }
    vi.mocked(getPaymentStatus).mockResolvedValue({ state: "paid", slot: 100 });
    expect(await (await f.request(checkout.checkoutUrl, "GET", undefined, f.merchantHeaders(merchant))).json()).toMatchObject({ status: "paid", slot: 100 });
    expect(submitPayment).toHaveBeenCalledOnce();
  });

  it("returns a terminal conflict without a success receipt when the private transfer failed", async () => {
    const f = fixture();
    const merchant = await f.register();
    const buyer = Keypair.generate();
    const checkout = await f.create(merchant, buyer);
    const { headers } = await f.proof(checkout, buyer, "mpp");
    vi.mocked(getPaymentStatus).mockResolvedValue({ state: "failed", failure: "InsufficientFunds", slot: 42 });
    const failed = await f.request(checkout.paymentUrls.mpp, "POST", undefined, headers);
    expect(failed.status).toBe(409);
    expect(failed.headers.get("Payment-Receipt")).toBeNull();
    expect(await (await f.request(checkout.checkoutUrl, "GET", undefined, f.merchantHeaders(merchant))).json()).toMatchObject({ status: "failed" });
  });

  it("facilitator verification is read-only; settlement binds the same merchant and exact requirements", async () => {
    const f = fixture();
    const merchant = await f.register();
    const buyer = Keypair.generate();
    const checkout = await f.create(merchant, buyer);
    const { credential } = await f.proof(checkout, buyer, "x402");
    const payload = decodeX402Payload(credential.headerValue).payload;
    const body = { x402Version: 2, paymentPayload: payload, paymentRequirements: payload.accepted };
    const storage = f.ledger.actors.get(checkout.payment.id)!.storage;
    const writes = storage.writes;
    const verified = await f.request("/v1/x402/verify", "POST", body, f.merchantHeaders(merchant));
    expect(await verified.json()).toEqual({ isValid: true, payer: buyer.publicKey.toBase58() });
    expect(storage.writes).toBe(writes);
    expect(submitPayment).not.toHaveBeenCalled();
    expect(getPaymentStatus).not.toHaveBeenCalled();
    const unsignedPayload = { ...payload, payload: { ...payload.payload, transaction: payload.accepted.extra.transaction } };
    const invalid = await f.request("/v1/x402/verify", "POST", { ...body, paymentPayload: unsignedPayload }, f.merchantHeaders(merchant));
    expect(invalid.status).toBe(200);
    expect(await invalid.json()).toMatchObject({ isValid: false });
    expect(storage.writes).toBe(writes);
    expect((await f.request("/v1/x402/settle", "POST", { ...body, paymentRequirements: { ...payload.accepted, amount: "1" } }, f.merchantHeaders(merchant))).status).toBe(400);
    const otherMerchant = await f.register();
    expect((await f.request("/v1/x402/settle", "POST", body, f.merchantHeaders(otherMerchant))).status).toBe(401);
    const settled = await f.request("/v1/x402/settle", "POST", body, f.merchantHeaders(merchant));
    expect(await settled.json()).toMatchObject({ success: true, amount: "1000000", payer: buyer.publicKey.toBase58(), extensions: { magicblock: { info: { settlement: "ephemeral-rollup" } } } });
    expect(submitPayment).toHaveBeenCalledOnce();
  });

  it("creates arbitrary reusable offers and idempotent buyer-specific checkouts without exposing payment state", async () => {
    const f = fixture();
    const merchant = await f.register();
    const offer = { amount: "2500000", cluster: "devnet-private", externalReference: "download-123", description: "Product download" };
    const first = await f.request("/v1/payment-links", "POST", offer, f.merchantHeaders(merchant));
    const link = await first.json() as { link: { id: string }; url: string };
    expect(first.status).toBe(200);
    expect(await (await f.request("/v1/payment-links", "POST", offer, f.merchantHeaders(merchant))).json()).toEqual(link);
    expect((await f.request("/v1/payment-links", "POST", { ...offer, amount: "1" }, f.merchantHeaders(merchant))).status).toBe(409);
    const publicOffer = await (await f.request(link.url)).json();
    expect(publicOffer).toMatchObject({ amount: offer.amount, description: offer.description });
    expect(publicOffer).not.toHaveProperty("status");
    expect(publicOffer).not.toHaveProperty("payer");
    const payer = Keypair.generate().publicKey.toBase58();
    const headers = { "Idempotency-Key": crypto.randomUUID() };
    const checkout = await (await f.request(`${link.url}/checkouts`, "POST", { payer }, headers)).json() as Checkout;
    expect(checkout.payment.amount).toBe(offer.amount);
    expect(checkout.payment.resource).toBe(checkout.checkoutUrl);
    expect(checkout.paymentUrls).toEqual({
      x402: `https://payments.test/v1/x402/payments/${checkout.payment.id}/pay`,
      mpp: `https://payments.test/v1/mpp/payments/${checkout.payment.id}/pay`,
    });
    expect(checkout).not.toHaveProperty("paymentUrl");
    expect(checkout.payment.externalReference).toMatch(new RegExp(`^link:${link.link.id}:[a-f0-9]{32}$`));
    expect(await (await f.request(`${link.url}/checkouts`, "POST", { payer }, headers)).json()).toEqual(checkout);
    const other = await (await f.request(`${link.url}/checkouts`, "POST", { payer }, { "Idempotency-Key": crypto.randomUUID() })).json() as Checkout;
    expect(other.payment.id).not.toBe(checkout.payment.id);
    expect(other.accessToken).not.toBe(checkout.accessToken);
    expect((await f.request(`${link.url}/checkouts`, "POST", { payer, amount: "1" }, headers)).status).toBe(422);
    expect((await f.request(`${link.url}/checkouts`, "POST", { payer })).status).toBe(422);
    expect((await f.request(`${link.url}/checkouts`, "POST", { payer }, { "Idempotency-Key": "predictable-key-12345" })).status).toBe(422);
  });

  it("enforces checkout order idempotency, immutable terms, and payment-only fields", async () => {
    const f = fixture();
    const merchant = await f.register();
    const buyer = Keypair.generate();
    const first = await f.create(merchant, buyer);
    expect(await f.create(merchant, buyer)).toEqual(first);
    const invalid = await f.request("/v1/payments", "POST", {
      payer: buyer.publicKey.toBase58(), amount: "2", cluster: "devnet-private", externalReference: "order_123",
    }, f.merchantHeaders(merchant));
    expect(invalid.status).toBe(409);
    const credits = await f.request("/v1/payments", "POST", {
      payer: buyer.publicKey.toBase58(), amount: "1", externalReference: "new", credits: 100,
    }, f.merchantHeaders(merchant));
    expect(credits.status).toBe(422);
  });

  it("documents separate protocol APIs and removes the mixed checkout endpoint", async () => {
    const f = fixture();
    const response = await f.request("/doc");
    expect(response.status).toBe(200);
    const document = await response.json() as { paths: Record<string, Record<string, { tags?: string[] }>> };
    for (const protocol of ["x402", "mpp"] as const) {
      const path = `/v1/${protocol}/payments/{id}/pay`;
      const tags = document.paths[path].post.tags?.map(tag => tag.toLowerCase());
      expect(tags).toContain(protocol);
      expect(tags).not.toContain(protocol === "x402" ? "mpp" : "x402");
    }
    for (const [path, method] of [["/v1/x402/supported", "get"], ["/v1/x402/verify", "post"], ["/v1/x402/settle", "post"]]) {
      expect(document.paths[path][method].tags?.map(tag => tag.toLowerCase())).toContain("x402");
    }
    expect(document.paths["/v1/payments/{id}/prepare"].post.tags).toContain("Payments");
    expect(document.paths).not.toHaveProperty("/v1/payments/{id}/pay");
    expect((await f.request(`/v1/payments/${"a".repeat(64)}/pay`, "POST")).status).toBe(404);
    expect(submitPayment).not.toHaveBeenCalled();
  });

  it("exposes protocol headers to browser clients, uses no-store and bounds request bodies", async () => {
    const f = fixture();
    const supported = await f.request("/v1/x402/supported", "GET", undefined, { Origin: "https://merchant.test" });
    expect(supported.headers.get("Cache-Control")).toBe("no-store");
    expect(supported.headers.get("Access-Control-Expose-Headers")).toContain("WWW-Authenticate");
    expect(supported.headers.get("Access-Control-Expose-Headers")).toContain("Payment-Receipt");
    expect(await supported.json()).toMatchObject({ kinds: expect.arrayContaining([expect.objectContaining({ scheme: "exact-magicblock", extra: expect.objectContaining({ cluster: "devnet-private" }) })]) });
    const preflight = await f.request("/v1/mpp/payments/id/pay", "OPTIONS", undefined, {
      "Origin": "https://merchant.test", "Access-Control-Request-Method": "POST", "Access-Control-Request-Headers": "authorization,payment-authorization,payment-signature,x-merchant-id",
    });
    expect(preflight.status).toBe(204);
    expect(preflight.headers.get("Access-Control-Allow-Headers")).toContain("payment-authorization");
    const oversized = await f.request("/v1/merchants/challenge", "POST", { wallet: "a".repeat(17_000) });
    expect(oversized.status).toBe(413);
    expect(oversized.headers.get("Cache-Control")).toBe("no-store");
    const dishonestLength = await f.request("/v1/merchants/challenge", "POST", { wallet: "a".repeat(17_000) }, { "Content-Length": "1" });
    expect(dishonestLength.status).toBe(413);
    for (const protocol of ["x402", "mpp"]) {
      const oversizedProof = await f.request(`/v1/${protocol}/payments/${"a".repeat(64)}/pay`, "POST", { padding: "a".repeat(17_000) });
      expect(oversizedProof.status).toBe(413);
      expect(oversizedProof.headers.get("Cache-Control")).toBe("no-store");
    }
    const malformed = await app.request("/v1/merchants/challenge", {
      method: "POST", headers: { "Content-Type": "application/json" }, body: "{",
    }, f.env);
    expect(malformed.status).toBe(400);
    expect(malformed.headers.get("Cache-Control")).toBe("no-store");
  });
});
