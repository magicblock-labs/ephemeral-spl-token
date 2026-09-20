import { Buffer } from "buffer";
import { z } from "zod";
import type { PaymentCluster, PaymentTerms, PaymentView, PreparedPayment } from "./types";

export const X402_SCHEME = "exact-magicblock";
export const MPP_METHOD = "magicblock";
const MAX_HEADER_LENGTH = 16_384;
const transactionSchema = z.string().min(1).max(1_644).regex(/^[A-Za-z0-9+/]+={0,2}$/);
const proofSchema = z.object({ paymentId: z.string().min(1).max(128), transaction: transactionSchema }).strict();
const resourceSchema = z.object({ url: z.string(), description: z.string().optional(), mimeType: z.string() }).strict();
const requirementsSchema = z.object({
  scheme: z.literal(X402_SCHEME),
  network: z.string(),
  amount: z.string(),
  asset: z.string(),
  payTo: z.string(),
  maxTimeoutSeconds: z.number().int().positive(),
  extra: z.record(z.string(), z.unknown()),
}).strict();
const x402PayloadSchema = z.object({
  x402Version: z.literal(2),
  resource: resourceSchema,
  accepted: requirementsSchema,
  payload: proofSchema,
}).strict();
const mppChallengeSchema = z.object({
  id: z.string().min(1).max(128),
  realm: z.string().min(1).max(256),
  method: z.literal(MPP_METHOD),
  intent: z.literal("charge"),
  request: z.string().min(1).max(MAX_HEADER_LENGTH),
  expires: z.string().datetime({ offset: true }),
  header: z.literal("Payment-Authorization"),
  digest: z.string().optional(),
}).strict();
const mppCredentialSchema = z.object({
  challenge: mppChallengeSchema,
  payload: proofSchema,
  source: z.string().optional(),
}).strict();

export type X402Payload = z.infer<typeof x402PayloadSchema>;
export type MppChallenge = z.infer<typeof mppChallengeSchema>;
export type MppCredential = z.infer<typeof mppCredentialSchema>;
type ValidationOptions = { allowExpired?: boolean; paymentRequirements?: unknown };

// RFC 8785: ECMAScript scalar serialization, recursively sorted UTF-16 object keys.
// Values originate in JSON/schema parsing; unsupported values are rejected.
export function canonicalJson(value: unknown): string {
  if (typeof value === "string" && /[\uD800-\uDFFF]/u.test(value)) {
    throw new Error("Payment data contains invalid Unicode");
  }
  if (value === null || typeof value === "boolean" || typeof value === "string") {
    return JSON.stringify(value);
  }
  if (typeof value === "number" && Number.isFinite(value)) return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (typeof value === "object" && value !== null) {
    return `{${Object.keys(value).sort().map(key => `${canonicalJson(key)}:${canonicalJson((value as Record<string, unknown>)[key])}`).join(",")}}`;
  }
  throw new Error("Payment data must contain JSON values");
}

export function encodePaymentJson(value: unknown, urlSafe = false): string {
  return Buffer.from(canonicalJson(value), "utf8").toString(urlSafe ? "base64url" : "base64");
}

export function decodePaymentJson(value: string, urlSafe = false): unknown {
  const pattern = urlSafe ? /^[A-Za-z0-9_-]+$/ : /^[A-Za-z0-9+/]+={0,2}$/;
  if (!value || value.length > MAX_HEADER_LENGTH || !pattern.test(value)) {
    throw new Error("Invalid payment header encoding");
  }
  const encoding = urlSafe ? "base64url" : "base64";
  const bytes = Buffer.from(value, encoding);
  if (bytes.toString(encoding) !== value) throw new Error("Non-canonical payment header encoding");
  return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
}

export function paymentNetwork(cluster: PaymentCluster): string {
  return cluster.startsWith("devnet")
    ? "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1"
    : "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp";
}

function paymentDetails(terms: PaymentTerms, prepared: PreparedPayment) {
  return {
    paymentId: terms.id,
    merchantId: terms.merchantId,
    payer: terms.payer,
    cluster: terms.cluster,
    validator: prepared.validator,
    settlement: "ephemeral-rollup" as const,
    resource: terms.resource,
    ...(terms.requestHash ? { requestHash: terms.requestHash } : {}),
    transaction: prepared.transactionBase64,
    recentBlockhash: prepared.recentBlockhash,
    lastValidBlockHeight: prepared.lastValidBlockHeight,
  };
}

export function createX402PaymentRequired(terms: PaymentTerms, prepared: PreparedPayment) {
  return {
    x402Version: 2 as const,
    resource: {
      url: terms.resource,
      ...(terms.description ? { description: terms.description } : {}),
      mimeType: "application/json",
    },
    accepts: [{
      scheme: X402_SCHEME,
      network: paymentNetwork(terms.cluster),
      amount: terms.amount,
      asset: terms.mint,
      payTo: terms.recipient,
      maxTimeoutSeconds: 60,
      extra: {
        ...paymentDetails(terms, prepared),
        expiresAt: terms.expiresAt,
        assetTransferMethod: "delegated-spl",
        paymentFlow: "upfront",
      },
    }],
  };
}

export function encodeX402PaymentRequired(terms: PaymentTerms, prepared: PreparedPayment): string {
  return encodePaymentJson(createX402PaymentRequired(terms, prepared));
}

/** Decode only to locate trusted payment state. This does not verify a payment. */
export function decodeX402Payload(input: unknown) {
  const payload = x402PayloadSchema.parse(typeof input === "string" ? decodePaymentJson(input) : input);
  return { paymentId: payload.payload.paymentId, transactionBase64: payload.payload.transaction, payload };
}

function checkExpiry(terms: PaymentTerms, now: number, allowExpired = false) {
  if (!allowExpired && Date.parse(terms.expiresAt) <= now) throw new Error("Payment challenge expired");
}

function assertSame(actual: unknown, expected: unknown, message: string) {
  if (canonicalJson(actual) !== canonicalJson(expected)) throw new Error(message);
}

/** Read-only envelope validation. The caller must also verify the transaction/signature. */
export function validateX402Payload(input: unknown, terms: PaymentTerms, prepared: PreparedPayment, now = Date.now(), options: ValidationOptions = {}) {
  const decoded = decodeX402Payload(input);
  const expected = createX402PaymentRequired(terms, prepared);
  assertSame(decoded.payload.accepted, expected.accepts[0], "Payment requirements do not match checkout");
  assertSame(decoded.payload.resource, expected.resource, "Payment resource does not match checkout");
  if (options.paymentRequirements !== undefined) {
    assertSame(options.paymentRequirements, expected.accepts[0], "Facilitator requirements do not match checkout");
  }
  if (decoded.paymentId !== terms.id) throw new Error("Payment ID does not match checkout");
  checkExpiry(terms, now, options.allowExpired);
  return decoded;
}

/** Reconstruct from immutable stored terms; exact comparison provides stateful challenge binding. */
export function createMppChallenge(terms: PaymentTerms, prepared: PreparedPayment, realm: string): MppChallenge {
  return mppChallengeSchema.parse({
    id: terms.id,
    realm,
    method: MPP_METHOD,
    intent: "charge",
    request: encodePaymentJson({
      amount: terms.amount,
      currency: terms.mint,
      recipient: terms.recipient,
      externalId: terms.externalReference,
      ...(terms.description ? { description: terms.description } : {}),
      methodDetails: { ...paymentDetails(terms, prepared), network: paymentNetwork(terms.cluster) },
    }, true),
    expires: terms.expiresAt,
    header: "Payment-Authorization",
    ...(terms.requestHash ? { digest: `sha-256=:${Buffer.from(terms.requestHash, "hex").toString("base64")}:` } : {}),
  });
}

export function encodeMppChallenge(challenge: MppChallenge): string {
  return `Payment ${Object.entries(mppChallengeSchema.parse(challenge)).map(([key, value]) => {
    if (/[\x00-\x1f\x7f]/.test(value)) throw new Error("Invalid payment challenge parameter");
    return `${key}="${value.replace(/\\/g, "\\\\").replace(/"/g, "\\\"")}"`;
  }).join(", ")}`;
}

/** Parse the single MagicBlock challenge emitted by this API, rejecting duplicate parameters. */
export function decodeMppChallenge(header: string): MppChallenge {
  if (header.length > MAX_HEADER_LENGTH || !/^Payment /i.test(header)) throw new Error("Invalid payment challenge");
  const parameters: Record<string, string> = {};
  let remaining = header.slice(8);
  while (remaining) {
    const match = /^\s*([a-zA-Z][a-zA-Z0-9_-]*)\s*=\s*"((?:[^"\\\x00-\x1f\x7f]|\\[\x20-\x7e])*)"\s*(,\s*|$)/.exec(remaining);
    if (!match || Object.hasOwn(parameters, match[1].toLowerCase())) throw new Error("Invalid or duplicate payment challenge parameter");
    parameters[match[1].toLowerCase()] = match[2].replace(/\\(.)/g, "$1");
    remaining = remaining.slice(match[0].length);
    if (!remaining && match[3]) throw new Error("Invalid payment challenge");
  }
  return mppChallengeSchema.parse(parameters);
}

export function encodeMppCredential(challenge: MppChallenge, paymentId: string, transactionBase64: string): string {
  return `Payment ${encodePaymentJson(mppCredentialSchema.parse({ challenge, payload: { paymentId, transaction: transactionBase64 } }), true)}`;
}

/** Decode only to locate trusted payment state. This does not verify a payment. */
export function decodeMppCredential(header: string) {
  if (!/^Payment [A-Za-z0-9_-]+$/i.test(header)) throw new Error("Invalid Payment credential");
  const credential = mppCredentialSchema.parse(decodePaymentJson(header.slice(8), true));
  return { paymentId: credential.payload.paymentId, transactionBase64: credential.payload.transaction, credential };
}

export function validateMppCredential(header: string, terms: PaymentTerms, prepared: PreparedPayment, realm: string, now = Date.now(), options: ValidationOptions = {}) {
  const decoded = decodeMppCredential(header);
  assertSame(decoded.credential.challenge, createMppChallenge(terms, prepared, realm), "Payment challenge does not match checkout");
  if (decoded.paymentId !== terms.id) throw new Error("Payment ID does not match checkout");
  checkExpiry(terms, now, options.allowExpired);
  return decoded;
}

export function paymentCredential(headers: Headers): { protocol: "x402" | "mpp"; value: string } | undefined {
  const x402 = headers.get("PAYMENT-SIGNATURE");
  const mpp = headers.get("Payment-Authorization");
  const authorization = headers.get("Authorization");
  if (authorization && /^Payment\s/i.test(authorization)) {
    throw new Error("Use the advertised Payment-Authorization header for MPP credentials");
  }
  if (x402 !== null && mpp !== null) throw new Error("Supply only one payment protocol credential");
  if (x402 !== null) {
    decodeX402Payload(x402);
    return { protocol: "x402", value: x402 };
  }
  if (mpp !== null) {
    decodeMppCredential(mpp);
    return { protocol: "mpp", value: mpp };
  }
  return undefined;
}

export function x402Settlement(payment: PaymentView) {
  return {
    success: payment.status === "paid",
    transaction: payment.signature ?? "",
    network: paymentNetwork(payment.cluster),
    payer: payment.payer,
    ...(payment.status === "paid" ? { amount: payment.amount } : { errorReason: payment.status === "pending" ? "settlement_pending" : `payment_${payment.status}` }),
    extensions: {
      magicblock: {
        info: { paymentId: payment.id, settlement: payment.settlement },
        schema: {
          type: "object",
          properties: { paymentId: { type: "string" }, settlement: { const: "ephemeral-rollup" } },
          required: ["paymentId", "settlement"],
        },
      },
    },
  };
}

export function encodeMppReceipt(payment: PaymentView): string {
  if (payment.status !== "paid" || !payment.signature || !payment.confirmedAt) {
    throw new Error("A payment receipt requires confirmed settlement");
  }
  return encodePaymentJson({
    status: "success",
    method: MPP_METHOD,
    timestamp: payment.confirmedAt,
    reference: payment.signature,
    paymentId: payment.id,
    settlement: payment.settlement,
  }, true);
}
