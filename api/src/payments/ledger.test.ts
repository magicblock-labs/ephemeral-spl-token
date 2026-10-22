import { Buffer } from "buffer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AppBindings } from "../env";
import { ApiError } from "../lib/errors";
import { getPaymentStatus, preparePayment, submitPayment, validateSignedPayment } from "./chain";
import { PaymentLedger } from "./ledger";
import type { PaymentRecord, PreparedPayment } from "./types";

vi.mock("./chain", () => ({
  getPaymentStatus: vi.fn(), preparePayment: vi.fn(), submitPayment: vi.fn(), validateSignedPayment: vi.fn(),
}));

class MemoryStorage {
  records = new Map<string, unknown>();
  alarmTime: number | null = null;
  writes = 0;
  failNextWrite = false;

  async get<T>(key: string): Promise<T | undefined> { return structuredClone(this.records.get(key)) as T | undefined; }
  async put(key: string | Record<string, unknown>, value?: unknown) {
    if (this.failNextWrite) {
      this.failNextWrite = false;
      throw new Error("Storage unavailable");
    }
    this.writes++;
    if (typeof key === "string") this.records.set(key, structuredClone(value));
    else for (const [name, entry] of Object.entries(key)) this.records.set(name, structuredClone(entry));
  }

  async setAlarm(time: number) { this.alarmTime = time; }
  async deleteAlarm() { this.alarmTime = null; }
  async transaction<T>(operation: (storage: MemoryStorage) => Promise<T>): Promise<T> {
    const snapshot = structuredClone(this.records);
    const alarm = this.alarmTime;
    try {
      return await operation(this);
    } catch (error) {
      this.records = snapshot;
      this.alarmTime = alarm;
      throw error;
    }
  }
}

const env: AppBindings = { BASE_RPC_URL: "https://base.test", EPHEMERAL_RPC_URL: "https://er.test" };
const prepared: PreparedPayment = {
  transactionBase64: "unsigned", messageBase64: "message", recentBlockhash: "blockhash",
  lastValidBlockHeight: 100, validator: "validator", rpcEndpoint: "https://rpc.test?key=secret",
};
const merchantId = "merchant";
const accessToken = "checkout-secret";

async function fixture() {
  const storage = new MemoryStorage();
  const state = { storage } as unknown as DurableObjectState;
  const ledger = new PaymentLedger(state, env);
  const record: PaymentRecord = {
    id: "pay_123", merchantId, payer: "buyer", recipient: "merchant-wallet", amount: "1000000", mint: "usdc",
    cluster: "devnet-private", externalReference: "order_456", resource: "https://merchant.test/credits",
    expiresAt: new Date(Date.now() + 60_000).toISOString(), createdAt: new Date().toISOString(), status: "created",
    accessTokenHash: Buffer.from(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(accessToken))).toString("hex"),
  };
  await call(ledger, "/initialize", { record, creationHash: "hash" });
  return { storage, state, ledger, record };
}

function call(ledger: PaymentLedger, path: string, body: unknown = { merchantId }) {
  return ledger.fetch(new Request(`https://ledger.test${path}`, { method: "POST", body: JSON.stringify(body) }));
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-09-19T12:00:00.000Z"));
  vi.mocked(preparePayment).mockResolvedValue(prepared);
  vi.mocked(submitPayment).mockResolvedValue();
  vi.mocked(getPaymentStatus).mockResolvedValue({ state: "pending" });
  vi.mocked(validateSignedPayment).mockImplementation((_terms, _prepared, transactionBase64) => {
    if (transactionBase64 === "invalid") throw new ApiError(400, "INVALID_PAYMENT_TRANSACTION", "Invalid transaction");
    return { transactionBase64, signature: `signature-${transactionBase64}` };
  });
});
afterEach(() => vi.useRealTimers());

describe("durable payment ledger", () => {
  it("creates once, returns the original order on retry and rejects changed terms", async () => {
    const { ledger, storage, record } = await fixture();
    const retry = await call(ledger, "/initialize", { record: { ...record, createdAt: "different" }, creationHash: "hash" });
    expect(retry.status).toBe(200);
    expect(await storage.get("payment")).toEqual(record);
    expect((await call(ledger, "/initialize", { record, creationHash: "different" })).status).toBe(409);
  });

  it("authorizes the merchant or hashed checkout capability and keeps private state out of status responses", async () => {
    const { ledger, storage } = await fixture();
    expect((await call(ledger, "/status", { merchantId: "other" })).status).toBe(401);
    expect((await call(ledger, "/read", { accessToken: "wrong" })).status).toBe(401);
    expect((await call(ledger, "/status", { accessToken })).status).toBe(200);
    await call(ledger, "/prepare");
    const payment = await (await call(ledger, "/status")).json();
    expect(payment).not.toHaveProperty("prepared");
    expect(payment).not.toHaveProperty("accessTokenHash");
    expect(JSON.stringify(payment)).not.toContain("secret");
    expect(await (await call(ledger, "/read")).json()).toEqual(await storage.get("payment"));
  });

  it("prepares one canonical transaction across concurrent retries", async () => {
    const { ledger } = await fixture();
    const responses = await Promise.all([call(ledger, "/prepare"), call(ledger, "/prepare")]);
    expect(preparePayment).toHaveBeenCalledOnce();
    for (const response of responses) expect(await response.json()).toMatchObject({ prepared });
  });

  it("verify is read-only and never broadcasts, including expired or invalid proofs", async () => {
    const { ledger, storage } = await fixture();
    await call(ledger, "/prepare");
    const writes = storage.writes;
    expect(await (await call(ledger, "/verify", { merchantId, transactionBase64: "signed" })).json()).toEqual({ isValid: true, payer: "buyer" });
    expect((await call(ledger, "/verify", { merchantId, transactionBase64: "invalid" })).status).toBe(400);
    vi.advanceTimersByTime(60_001);
    expect((await call(ledger, "/verify", { merchantId, transactionBase64: "signed" })).status).toBe(409);
    expect(storage.writes).toBe(writes);
    expect(submitPayment).not.toHaveBeenCalled();
    expect(getPaymentStatus).not.toHaveBeenCalled();
  });

  it("durably binds the signature and recovery alarm before broadcasting", async () => {
    const { ledger, storage } = await fixture();
    await call(ledger, "/prepare");
    vi.mocked(submitPayment).mockImplementation(async () => {
      expect(await storage.get("payment")).toMatchObject({ status: "pending", signedTransactionBase64: "signed", signature: "signature-signed" });
      expect(storage.alarmTime).toBe(Date.now() + 1_000);
    });
    vi.mocked(getPaymentStatus).mockResolvedValue({ state: "paid", slot: 42 });
    const response = await call(ledger, "/settle", { merchantId, transactionBase64: "signed" });
    expect(await response.json()).toMatchObject({ status: "paid", slot: 42, signature: "signature-signed", settlement: "ephemeral-rollup" });
    expect(storage.alarmTime).toBeNull();
    expect(await storage.get("payment")).toMatchObject({ status: "paid", confirmedAt: new Date().toISOString() });
  });

  it("does not broadcast if durable persistence fails", async () => {
    const { ledger, storage } = await fixture();
    await call(ledger, "/prepare");
    storage.failNextWrite = true;
    expect((await call(ledger, "/settle", { merchantId, transactionBase64: "signed" })).status).toBe(500);
    expect(submitPayment).not.toHaveBeenCalled();
    expect(await storage.get("payment")).toMatchObject({ status: "created" });
  });

  it("serializes concurrent settlement and cross-protocol retries into one payment", async () => {
    const { ledger } = await fixture();
    await call(ledger, "/prepare");
    vi.mocked(getPaymentStatus).mockResolvedValue({ state: "paid", slot: 42 });
    const [first, second] = await Promise.all([
      call(ledger, "/settle", { merchantId, transactionBase64: "signed" }),
      call(ledger, "/settle", { accessToken, transactionBase64: "signed" }),
    ]);
    expect(await first.json()).toEqual(await second.json());
    expect(submitPayment).toHaveBeenCalledOnce();
    expect((await call(ledger, "/settle", { merchantId, transactionBase64: "different" })).status).toBe(409);
  });

  it("keeps ambiguous submission and RPC failures pending, and retries only identical bytes after restart", async () => {
    const { ledger, storage, state } = await fixture();
    await call(ledger, "/prepare");
    vi.mocked(submitPayment).mockRejectedValue(new Error("Request timeout"));
    vi.mocked(getPaymentStatus).mockRejectedValue(new Error("RPC auth expired"));
    expect(await (await call(ledger, "/settle", { merchantId, transactionBase64: "signed" })).json()).toMatchObject({ status: "pending" });
    vi.advanceTimersByTime(120_000);
    const restarted = new PaymentLedger(state, env);
    await restarted.alarm();
    expect(submitPayment).toHaveBeenCalledTimes(2);
    expect(vi.mocked(submitPayment).mock.calls.every(args => args[3] === "signed")).toBe(true);
    expect(await storage.get("payment")).toMatchObject({ status: "pending", reconcileAttempts: 1 });
    expect(storage.alarmTime).toBe(Date.now() + 2_000);
    vi.mocked(getPaymentStatus).mockResolvedValue({ state: "paid", slot: 55 });
    await restarted.alarm();
    expect(await storage.get("payment")).toMatchObject({ status: "paid", slot: 55 });
    expect(submitPayment).toHaveBeenCalledTimes(2);
    expect(storage.alarmTime).toBeNull();
  });

  it("recovers a crash after durable persistence but before the first broadcast", async () => {
    const { ledger, storage, state } = await fixture();
    await call(ledger, "/prepare");
    const record = (await storage.get<PaymentRecord>("payment"))!;
    await storage.put("payment", { ...record, status: "pending", signature: "signature-signed", signedTransactionBase64: "signed" });
    await new PaymentLedger(state, env).alarm();
    expect(submitPayment).toHaveBeenCalledOnce();
    expect(vi.mocked(submitPayment).mock.calls[0][3]).toBe("signed");
    expect(storage.alarmTime).not.toBeNull();
  });

  it("retains unknown outcomes indefinitely with bounded backoff and never expires submitted payments", async () => {
    const { ledger, storage } = await fixture();
    await call(ledger, "/prepare");
    await call(ledger, "/settle", { merchantId, transactionBase64: "signed" });
    vi.advanceTimersByTime(3_600_000);
    for (let i = 0; i < 10; i++) await ledger.alarm();
    expect(storage.alarmTime).toBe(Date.now() + 60_000);
    const alarm = storage.alarmTime;
    expect(await (await call(ledger, "/status")).json()).toMatchObject({ status: "pending" });
    expect(storage.alarmTime).toBe(alarm);
    expect((await call(ledger, "/settle", { merchantId, transactionBase64: "signed" })).status).toBe(200);
    expect(preparePayment).toHaveBeenCalledOnce();
  });

  it("records only authoritative execution failures and stops reconciliation", async () => {
    const { ledger, storage } = await fixture();
    await call(ledger, "/prepare");
    vi.mocked(getPaymentStatus).mockResolvedValue({ state: "failed", slot: 8, failure: "InsufficientFunds" });
    const result = await (await call(ledger, "/settle", { merchantId, transactionBase64: "signed" })).json();
    expect(result).toMatchObject({ status: "failed", failure: "InsufficientFunds", slot: 8 });
    expect(storage.alarmTime).toBeNull();
    await ledger.alarm();
    expect(getPaymentStatus).toHaveBeenCalledOnce();
    expect((await call(ledger, "/verify", { merchantId, transactionBase64: "signed" })).status).toBe(409);
  });

  it("expires unsubmitted checkouts without creating or refreshing a transaction", async () => {
    const { ledger, storage } = await fixture();
    vi.advanceTimersByTime(60_001);
    expect(await (await call(ledger, "/status")).json()).toMatchObject({ status: "expired" });
    expect((await call(ledger, "/prepare")).status).toBe(409);
    expect(preparePayment).not.toHaveBeenCalled();
    expect(await storage.get("payment")).toMatchObject({ status: "expired" });
  });

  it("keeps a paid order recoverable after merchant delivery fails", async () => {
    const { ledger, state } = await fixture();
    await call(ledger, "/prepare");
    vi.mocked(getPaymentStatus).mockResolvedValue({ state: "paid", slot: 42 });
    await call(ledger, "/settle", { merchantId, transactionBase64: "signed" });
    vi.advanceTimersByTime(3_600_000);
    const restarted = new PaymentLedger(state, env);
    expect(await (await call(restarted, "/status")).json()).toMatchObject({ status: "paid" });
    expect(await (await call(restarted, "/settle", { merchantId, transactionBase64: "signed" })).json()).toMatchObject({ status: "paid" });
    expect(submitPayment).toHaveBeenCalledOnce();
  });

  it("does not reveal whether an unauthorized payment exists", async () => {
    const { ledger } = await fixture();
    const empty = new PaymentLedger({ storage: new MemoryStorage() } as unknown as DurableObjectState, env);
    for (const path of ["/read", "/status", "/prepare", "/verify", "/settle"]) {
      const unauthorized = await call(ledger, path, { accessToken: "wrong" });
      const missing = await call(empty, path, { accessToken: "wrong" });
      expect(unauthorized.status).toBe(401);
      expect(missing.status).toBe(401);
      expect(await missing.json()).toEqual(await unauthorized.json());
    }
    expect((await call(ledger, "/missing")).status).toBe(404);
    expect((await call(empty, "/missing")).status).toBe(404);
    expect((await empty.fetch(new Request("https://ledger.test/status"))).status).toBe(405);
  });
});
