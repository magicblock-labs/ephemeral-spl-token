import { Buffer } from "buffer";
import { getEnv, type AppBindings } from "../env";
import { ApiError, errorBody } from "../lib/errors";
import { getPaymentStatus, preparePayment, submitPayment, validateSignedPayment } from "./chain";
import type { PaymentRecord, PaymentView } from "./types";

type LedgerRequest = {
  merchantId?: string;
  accessToken?: string;
  transactionBase64?: string;
  record?: PaymentRecord;
  creationHash?: string;
};

function view(record: PaymentRecord): PaymentView {
  return {
    id: record.id,
    merchantId: record.merchantId,
    payer: record.payer,
    recipient: record.recipient,
    amount: record.amount,
    mint: record.mint,
    cluster: record.cluster,
    externalReference: record.externalReference,
    resource: record.resource,
    ...(record.description ? { description: record.description } : {}),
    ...(record.requestHash ? { requestHash: record.requestHash } : {}),
    expiresAt: record.expiresAt,
    status: record.status,
    settlement: "ephemeral-rollup",
    ...(record.signature ? { signature: record.signature } : {}),
    ...(record.confirmedAt ? { confirmedAt: record.confirmedAt } : {}),
    ...(record.slot !== undefined ? { slot: record.slot } : {}),
    ...(record.failure ? { failure: record.failure } : {}),
  };
}

/** One actor per payment/order. Only the authenticated Worker invokes these internal routes. */
export class PaymentLedger {
  private queue: Promise<unknown> = Promise.resolve();

  constructor(private state: DurableObjectState, private env: AppBindings) {}

  private serial<T>(operation: () => Promise<T>): Promise<T> {
    const result = this.queue.then(operation);
    this.queue = result.catch(() => undefined);
    return result;
  }

  async fetch(request: Request): Promise<Response> {
    try {
      return await this.serial(async () => {
        if (request.method !== "POST") throw new ApiError(405, "METHOD_NOT_ALLOWED", "Use POST for payment operations");
        let input: LedgerRequest;
        try {
          input = await request.json() as LedgerRequest;
          if (!input || typeof input !== "object" || Array.isArray(input)) throw new Error("Invalid body");
        } catch {
          throw new ApiError(400, "INVALID_REQUEST", "Expected a JSON request object");
        }
        return Response.json(await this.handle(new URL(request.url).pathname, input));
      });
    } catch (error) {
      if (error instanceof ApiError) return Response.json(errorBody(error.code, error.message, error.details), { status: error.status });
      return Response.json(errorBody("PAYMENT_ERROR", "Payment operation failed"), { status: 500 });
    }
  }

  private async authorize(record: PaymentRecord, input: LedgerRequest) {
    // merchantId is populated only after the Worker verifies its API credential.
    if (input.merchantId === record.merchantId) return;
    if (typeof input.accessToken === "string" && input.accessToken.length <= 256) {
      const hash = Buffer.from(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(input.accessToken))).toString("hex");
      if (hash === record.accessTokenHash) return;
    }
    throw new ApiError(401, "PAYMENT_UNAUTHORIZED", "Valid merchant credentials or checkout access token required");
  }

  private async handle(path: string, input: LedgerRequest) {
    if (path === "/initialize") {
      if (!input.record || typeof input.creationHash !== "string" || !input.creationHash) {
        throw new ApiError(400, "INVALID_REQUEST", "Payment record and creation hash required");
      }
      return this.state.storage.transaction(async (storage) => {
        const existing = await storage.get<PaymentRecord>("payment");
        if (existing) {
          if (await storage.get("creationHash") !== input.creationHash) {
            throw new ApiError(409, "PAYMENT_ORDER_CONFLICT", "This order already has different payment terms");
          }
          return view(existing);
        }
        await storage.put({ payment: input.record!, creationHash: input.creationHash! });
        return view(input.record!);
      });
    }
    if (!["/read", "/prepare", "/verify", "/settle", "/status"].includes(path)) {
      throw new ApiError(404, "NOT_FOUND", "Unknown payment operation");
    }
    const record = await this.state.storage.get<PaymentRecord>("payment");
    if (!record) throw new ApiError(401, "PAYMENT_UNAUTHORIZED", "Valid merchant credentials or checkout access token required");
    await this.authorize(record, input);
    if (path === "/read") return record;
    if (path === "/verify") {
      if (record.status === "failed") throw new ApiError(409, "PAYMENT_FAILED", "Payment transaction failed execution");
      this.signed(record, input.transactionBase64);
      return { isValid: true, payer: record.payer };
    }
    if (path === "/prepare") {
      await this.expire(record);
      if (record.status === "expired" || record.status === "failed") {
        throw new ApiError(409, "PAYMENT_NOT_PAYABLE", "Payment can no longer be prepared");
      }
      if (!record.prepared) {
        if (record.status !== "created") throw new ApiError(409, "PAYMENT_NOT_PREPARED", "Submitted payment has no prepared transaction");
        const prepared = await preparePayment(getEnv(this.env), record);
        if (Date.parse(record.expiresAt) <= Date.now()) {
          await this.expire(record);
          throw new ApiError(409, "PAYMENT_EXPIRED", "Payment expired during preparation");
        }
        record.prepared = prepared;
        await this.state.storage.put("payment", record);
      }
      return { payment: view(record), prepared: record.prepared };
    }
    if (path === "/settle") {
      const signed = this.signed(record, input.transactionBase64);
      if (record.status === "created") {
        record.status = "pending";
        record.signedTransactionBase64 = signed.transactionBase64;
        record.signature = signed.signature;
        record.reconcileAttempts = 0;
        // Persist the exact bytes and recovery alarm atomically before any broadcast.
        await this.state.storage.transaction(async (storage) => {
          await storage.put("payment", record);
          await storage.setAlarm(Date.now() + 1_000);
        });
        await this.broadcast(record);
      }
      if (record.status === "pending") await this.reconcile(record);
      return view(record);
    }
    await this.expire(record);
    if (record.status === "pending") await this.reconcile(record);
    return view(record);
  }

  private signed(record: PaymentRecord, transactionBase64?: string) {
    if (!record.prepared) throw new ApiError(409, "PAYMENT_NOT_PREPARED", "Prepare the payment before signing");
    if (typeof transactionBase64 !== "string") throw new ApiError(400, "PAYMENT_SIGNATURE_REQUIRED", "Signed payment transaction required");
    if (record.status === "expired" || (record.status === "created" && Date.parse(record.expiresAt) <= Date.now())) {
      throw new ApiError(409, "PAYMENT_EXPIRED", "Payment expired before submission");
    }
    const signed = validateSignedPayment(record, record.prepared, transactionBase64);
    if (record.signedTransactionBase64 && (record.signedTransactionBase64 !== signed.transactionBase64 || record.signature !== signed.signature)) {
      throw new ApiError(409, "PAYMENT_TRANSACTION_CONFLICT", "Payment is already bound to a different signed transaction");
    }
    return signed;
  }

  private async expire(record: PaymentRecord) {
    if (record.status === "created" && Date.parse(record.expiresAt) <= Date.now()) {
      record.status = "expired";
      await this.state.storage.put("payment", record);
    }
  }

  private async broadcast(record: PaymentRecord) {
    if (record.blockhashExpired) return;
    try {
      await submitPayment(getEnv(this.env), record, record.prepared!, record.signedTransactionBase64!);
    } catch (error) {
      // Submission errors are ambiguous: the exact bytes may already have landed.
      if (error instanceof ApiError && error.code === "PAYMENT_BLOCKHASH_EXPIRED") {
        record.blockhashExpired = true;
        await this.state.storage.put("payment", record);
      }
    }
  }

  private async reconcile(record: PaymentRecord) {
    try {
      const status = await getPaymentStatus(getEnv(this.env), record, record.prepared!, record.signature!);
      if (status.state === "pending") return;
      record.status = status.state;
      record.slot = status.slot;
      if (status.state === "paid") record.confirmedAt = new Date().toISOString();
      else record.failure = status.failure ?? "Transaction execution failed";
    } catch {
      // Missing RPC access/history is not evidence of success or failure.
      return;
    }
    await this.state.storage.transaction(async (storage) => {
      await storage.put("payment", record);
      await storage.deleteAlarm();
    });
  }

  async alarm(): Promise<void> {
    return this.serial(async () => {
      const record = await this.state.storage.get<PaymentRecord>("payment");
      if (!record || record.status !== "pending") return;
      await this.reconcile(record);
      if (record.status !== "pending") return;
      // Recover a crash after persist but before broadcast without creating another charge.
      await this.broadcast(record);
      record.reconcileAttempts = Math.min((record.reconcileAttempts ?? 0) + 1, 16);
      const delay = Math.min(1_000 * 2 ** record.reconcileAttempts, 60_000);
      await this.state.storage.transaction(async (storage) => {
        await storage.put("payment", record);
        await storage.setAlarm(Date.now() + delay);
      });
    });
  }
}
