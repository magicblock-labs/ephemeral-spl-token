import { Buffer } from "buffer";
import { describe, expect, it } from "vitest";
import {
  canonicalJson,
  createMppChallenge,
  createX402PaymentRequired,
  decodeMppChallenge,
  decodeMppCredential,
  decodePaymentJson,
  decodeX402Payload,
  encodeMppChallenge,
  encodeMppCredential,
  encodeMppReceipt,
  encodePaymentJson,
  encodeX402PaymentRequired,
  paymentCredential,
  paymentNetwork,
  validateMppCredential,
  validateX402Payload,
  x402Settlement,
} from "./protocols";
import type { PaymentTerms, PaymentView, PreparedPayment } from "./types";

const terms: PaymentTerms = {
  id: "pay_123", merchantId: "mer_123", payer: "buyer", recipient: "merchant",
  amount: "1000000", mint: "usdc", cluster: "devnet-private", externalReference: "order_456",
  resource: "https://merchant.test/credits", requestHash: "a".repeat(64),
  description: "100 credits", expiresAt: "2026-09-19T13:00:00.000Z",
};
const prepared: PreparedPayment = {
  transactionBase64: Buffer.from("unsigned").toString("base64"), messageBase64: "bWVzc2FnZQ==",
  recentBlockhash: "blockhash", lastValidBlockHeight: 100, validator: "validator",
  rpcEndpoint: "https://private.rpc.test/?token=secret",
};
const now = Date.parse("2026-09-19T12:00:00.000Z");
const signed = Buffer.from("signed").toString("base64");
const realm = "payments.test";

function x402Payload() {
  const required = createX402PaymentRequired(terms, prepared);
  return { x402Version: 2, resource: required.resource, accepted: required.accepts[0], payload: { paymentId: terms.id, transaction: signed } };
}

describe("payment protocol envelopes", () => {
  it("uses base-layer CAIP-2 IDs and explicit private ER metadata without exposing RPC credentials", () => {
    const required = createX402PaymentRequired(terms, prepared);
    expect(required.accepts[0]).toMatchObject({
      scheme: "exact-magicblock", network: "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
      extra: { paymentFlow: "upfront", cluster: "devnet-private", settlement: "ephemeral-rollup", validator: "validator" },
    });
    expect(paymentNetwork("mainnet-private")).toBe("solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
    expect(JSON.stringify(required)).not.toContain("secret");
    expect(decodePaymentJson(encodeX402PaymentRequired(terms, prepared))).toEqual(required);
  });

  it("round-trips x402 v2 credentials and validates against stored requirements", () => {
    const payload = x402Payload();
    const header = encodePaymentJson(payload);
    expect(decodeX402Payload(header).paymentId).toBe(terms.id);
    expect(validateX402Payload(header, terms, prepared, now).transactionBase64).toBe(signed);
    expect(validateX402Payload(payload, terms, prepared, now, { paymentRequirements: payload.accepted }).paymentId).toBe(terms.id);
  });

  it.each(["amount", "asset", "payTo", "network", "scheme"])("rejects altered x402 %s", (field) => {
    const payload = x402Payload();
    Object.assign(payload.accepted, { [field]: "changed" });
    expect(() => validateX402Payload(payload, terms, prepared, now)).toThrow();
  });

  it("rejects changed facilitator requirements even when payload requirements are valid", () => {
    const payload = x402Payload();
    expect(() => validateX402Payload(payload, terms, prepared, now, {
      paymentRequirements: { ...payload.accepted, amount: "1" },
    })).toThrow("Facilitator requirements");
  });

  it("binds x402 order, resource, request body, validator, expiry and canonical unsigned transaction", () => {
    for (const altered of [
      { ...terms, id: "other" }, { ...terms, resource: "https://merchant.test/other" },
      { ...terms, requestHash: "b".repeat(64) }, { ...terms, expiresAt: "2026-09-19T14:00:00.000Z" },
    ]) expect(() => validateX402Payload(x402Payload(), altered, prepared, now)).toThrow();
    expect(() => validateX402Payload(x402Payload(), terms, { ...prepared, validator: "other" }, now)).toThrow();
    expect(() => validateX402Payload(x402Payload(), terms, { ...prepared, transactionBase64: signed }, now)).toThrow();
    const payload = x402Payload();
    payload.payload.paymentId = "other";
    expect(() => validateX402Payload(payload, terms, prepared, now)).toThrow("Payment ID");
  });

  it("creates standard MPP auth-params, JCS requests, credentials and successful receipts", () => {
    const challenge = createMppChallenge(terms, prepared, realm);
    expect(decodeMppChallenge(encodeMppChallenge(challenge))).toEqual(challenge);
    expect(challenge.digest).toBe(`sha-256=:${Buffer.from(terms.requestHash!, "hex").toString("base64")}:`);
    const request = decodePaymentJson(challenge.request, true);
    expect(request).toMatchObject({ amount: terms.amount, currency: terms.mint, methodDetails: { paymentId: terms.id } });
    expect(request).not.toHaveProperty("expires");
    expect(Buffer.from(challenge.request, "base64url").toString()).toBe(canonicalJson(request));
    expect(JSON.stringify(challenge)).not.toContain("secret");
    const header = encodeMppCredential(challenge, terms.id, signed);
    expect(decodeMppCredential(header).credential.challenge.request).toBe(challenge.request);
    expect(validateMppCredential(header, terms, prepared, realm, now).transactionBase64).toBe(signed);
    const paid: PaymentView = { ...terms, status: "paid", settlement: "ephemeral-rollup", signature: "signature", confirmedAt: "2026-09-19T12:01:00.000Z" };
    expect(decodePaymentJson(encodeMppReceipt(paid), true)).toMatchObject({
      status: "success", method: "magicblock", reference: "signature", paymentId: terms.id, settlement: "ephemeral-rollup",
    });
    expect(x402Settlement(paid)).toMatchObject({ success: true, transaction: "signature", amount: terms.amount });
    expect(() => encodeMppReceipt({ ...paid, status: "pending" })).toThrow("confirmed settlement");
    expect(x402Settlement({ ...paid, status: "pending" })).toMatchObject({ success: false, errorReason: "settlement_pending" });
  });

  it.each(["id", "realm", "method", "intent", "request", "expires", "header", "digest"])("rejects altered MPP %s", (field) => {
    const challenge = { ...createMppChallenge(terms, prepared, realm), [field]: "changed" };
    const header = `Payment ${encodePaymentJson({ challenge, payload: { paymentId: terms.id, transaction: signed } }, true)}`;
    expect(() => validateMppCredential(header, terms, prepared, realm, now)).toThrow();
  });

  it("rejects expired challenges while allowing explicit reconciliation of previously submitted payments", () => {
    const expiredNow = Date.parse(terms.expiresAt);
    const mpp = encodeMppCredential(createMppChallenge(terms, prepared, realm), terms.id, signed);
    expect(() => validateX402Payload(x402Payload(), terms, prepared, expiredNow)).toThrow("expired");
    expect(() => validateMppCredential(mpp, terms, prepared, realm, expiredNow)).toThrow("expired");
    expect(validateX402Payload(x402Payload(), terms, prepared, expiredNow, { allowExpired: true }).paymentId).toBe(terms.id);
    expect(validateMppCredential(mpp, terms, prepared, realm, expiredNow, { allowExpired: true }).paymentId).toBe(terms.id);
  });

  it("preserves bearer authentication and rejects ambiguous or misplaced payment credentials", () => {
    const x402 = encodePaymentJson(x402Payload());
    const mpp = encodeMppCredential(createMppChallenge(terms, prepared, realm), terms.id, signed);
    const headers = new Headers({ "Authorization": "Bearer merchant-api-key", "Payment-Authorization": mpp });
    expect(paymentCredential(headers)).toEqual({ protocol: "mpp", value: mpp });
    expect(headers.get("Authorization")).toBe("Bearer merchant-api-key");
    headers.set("PAYMENT-SIGNATURE", x402);
    expect(() => paymentCredential(headers)).toThrow("only one");
    expect(() => paymentCredential(new Headers({ Authorization: mpp }))).toThrow("advertised");
    expect(() => paymentCredential(new Headers({ "PAYMENT-SIGNATURE": mpp }))).toThrow();
    expect(() => paymentCredential(new Headers({ "Payment-Authorization": x402 }))).toThrow();
    expect(paymentCredential(new Headers())).toBeUndefined();
  });

  it("rejects malformed/oversized/non-canonical header encodings and duplicated challenges", () => {
    expect(() => decodePaymentJson("e30=", true)).toThrow();
    expect(() => decodePaymentJson("e30")).toThrow();
    expect(() => decodePaymentJson("A".repeat(16_385))).toThrow();
    expect(() => decodePaymentJson(Buffer.from([0xff]).toString("base64"))).toThrow();
    const challenge = encodeMppChallenge(createMppChallenge(terms, prepared, realm));
    expect(() => decodeMppChallenge(`${challenge}, id="other"`)).toThrow("duplicate");
    expect(() => decodeMppChallenge(`${challenge}, `)).toThrow();
    expect(() => canonicalJson(Number.NaN)).toThrow();
    expect(() => canonicalJson("\ud800")).toThrow("Unicode");
    expect(canonicalJson("\ud83d\ude00")).toBe("\"\ud83d\ude00\"");
  });
});
