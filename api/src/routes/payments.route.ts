import { createRoute, OpenAPIHono, z } from "@hono/zod-openapi";
import type { Context } from "hono";
import { getEnv } from "../env";
import { openApiDefaultHook } from "../lib/create-app";
import { ApiError } from "../lib/errors";
import { validatePaymentCluster } from "../payments/chain";
import { callObject, checkoutToken, digest, paymentConfig } from "../payments/config";
import { canonicalJson } from "../payments/protocols";
import { authorization, body, ledger, merchant, paymentParams, responses } from "../payments/http";
import type { Env } from "../payments/http";
import x402 from "./x402.route";
import mpp from "./mpp.route";
import {
  challengeSchema, checkoutSchema, idSchema, linkCheckoutSchema,
  linkSchema, registrationSchema, settleSchema, walletSchema,
} from "../payments/schemas";
import type { CheckoutInput, PaymentLink } from "../payments/schemas";
import type { PaymentRecord, PaymentView, PreparedPayment } from "../payments/types";

const app = new OpenAPIHono<Env>({ defaultHook: openApiDefaultHook });
const tags = ["Payments"];

// Credentials, unsigned transactions and private order information must not enter shared caches.
for (const path of ["/v1/merchants/*", "/v1/merchants", "/v1/payment-links/*", "/v1/payment-links", "/v1/payments/*", "/v1/payments", "/v1/x402/*", "/v1/mpp/*"]) {
  app.use(path, async (c, next) => {
    c.header("Cache-Control", "no-store");
    await next();
  });
  app.use(path, async (c, next) => {
    // Bound the actual stream before JSON validation, including chunked requests.
    const reader = c.req.raw.body?.getReader();
    if (reader) {
      const chunks: Uint8Array[] = [];
      let size = 0;
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        size += value.byteLength;
        if (size > 16_384) {
          void reader.cancel().catch(() => undefined);
          return c.json({ error: { code: "REQUEST_TOO_LARGE", message: "Payment request exceeds 16 KiB" } }, 413);
        }
        chunks.push(value);
      }
      const bytes = new Uint8Array(size);
      let offset = 0;
      for (const chunk of chunks) {
        bytes.set(chunk, offset);
        offset += chunk.byteLength;
      }
      if (size && c.req.header("Content-Type")?.includes("json")) {
        try {
          JSON.parse(new TextDecoder().decode(bytes));
        } catch {
          return c.json({ error: { code: "INVALID_REQUEST", message: "Malformed JSON request" } }, 400);
        }
      }
      c.req.raw = new Request(c.req.raw, { body: bytes });
    }
    await next();
  });
}

async function createCheckout(c: Context<Env>, merchantId: string, input: CheckoutInput) {
  const config = paymentConfig(c.env);
  validatePaymentCluster(getEnv(c.env), input.cluster);
  if (input.payer === merchantId) {
    throw new ApiError(400, "INVALID_PAYMENT_ACCOUNTS", "Payment requires a signer wallet and a distinct recipient");
  }
  const id = digest(`payment:${canonicalJson([merchantId, input.externalReference])}`);
  const accessToken = checkoutToken(c.env, id);
  const record: PaymentRecord = {
    id, merchantId, payer: input.payer, recipient: merchantId,
    amount: input.amount, cluster: input.cluster,
    mint: input.cluster.startsWith("devnet") ? "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU" : "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    externalReference: input.externalReference,
    resource: input.resource ?? `${config.origin}/v1/payments/${id}`,
    ...(input.description ? { description: input.description } : {}),
    ...(input.requestHash ? { requestHash: input.requestHash } : {}),
    expiresAt: new Date(Date.now() + input.expiresInSeconds * 1000).toISOString(),
    createdAt: new Date().toISOString(), status: "created", accessTokenHash: digest(accessToken),
  };
  const payment = await ledger<PaymentView>(c, id, "/initialize", { record, creationHash: digest(canonicalJson(input)) });
  return {
    payment, accessToken, checkoutUrl: `${config.origin}/v1/payments/${id}`,
    paymentUrls: {
      x402: `${config.origin}/v1/x402/payments/${id}/pay`,
      mpp: `${config.origin}/v1/mpp/payments/${id}/pay`,
    },
  };
}

function publicPreparation(payment: PaymentView, prepared: PreparedPayment) {
  return {
    payment,
    transactionBase64: prepared.transactionBase64,
    recentBlockhash: prepared.recentBlockhash,
    lastValidBlockHeight: prepared.lastValidBlockHeight,
    validator: prepared.validator,
    fees: { lamports: "0", tokens: "0" },
  };
}

app.openapi(createRoute({ method: "post", path: "/v1/merchants/challenge", tags, summary: "Request a wallet-signed merchant registration or key-rotation challenge", request: body(challengeSchema), responses }), async (c) => {
  const input = c.req.valid("json");
  const challenge = await callObject<Record<string, unknown>>(paymentConfig(c.env).merchants, input.wallet, "/challenge", input);
  return c.json(challenge, 200);
});

app.openapi(createRoute({ method: "post", path: "/v1/merchants", tags, summary: "Register a merchant; signature is base64 Ed25519 over the exact challenge message", request: body(registrationSchema), responses }), async (c) => {
  const input = c.req.valid("json");
  return c.json(await callObject<Record<string, unknown>>(paymentConfig(c.env).merchants, input.wallet, "/register", input), 200);
});

app.openapi(createRoute({ method: "post", path: "/v1/merchants/rotate-key", tags, summary: "Rotate a merchant API key using a fresh wallet-signed rotate-key challenge", request: body(registrationSchema), responses }), async (c) => {
  const input = c.req.valid("json");
  return c.json(await callObject<Record<string, unknown>>(paymentConfig(c.env).merchants, input.wallet, "/rotate-key", input), 200);
});

app.openapi(createRoute({ method: "post", path: "/v1/payments", tags, summary: "Create a checkout; externalReference is unique per merchant and retries return the same payment", request: body(checkoutSchema), responses }), async (c) => {
  const identity = await merchant(c);
  return c.json(await createCheckout(c, identity.merchantId, c.req.valid("json")), 200);
});

app.openapi(createRoute({ method: "post", path: "/v1/payment-links", tags, summary: "Create a reusable offer; checkout sessions are created separately for each buyer", request: body(linkSchema), responses }), async (c) => {
  const identity = await merchant(c);
  const input = c.req.valid("json");
  const config = paymentConfig(c.env);
  validatePaymentCluster(getEnv(c.env), input.cluster);
  const id = digest(`link:${canonicalJson([identity.merchantId, input.externalReference])}`);
  const link = await callObject<PaymentLink>(config.merchants, identity.merchantId, "/create-link", {
    link: { ...input, id, merchantId: identity.merchantId, recipient: identity.wallet, createdAt: new Date().toISOString() },
    creationHash: digest(canonicalJson(input)),
  });
  return c.json({ link, url: `${config.origin}/v1/payment-links/${identity.merchantId}/${id}` }, 200);
});

const linkParams = z.object({ merchantId: walletSchema, linkId: idSchema });
app.openapi(createRoute({ method: "get", path: "/v1/payment-links/{merchantId}/{linkId}", tags, summary: "Read a public offer; does not reveal buyer or payment status", request: { params: linkParams }, responses }), async (c) => {
  const { merchantId, linkId } = c.req.valid("param");
  return c.json(await callObject<PaymentLink>(paymentConfig(c.env).merchants, merchantId, "/get-link", { id: linkId }), 200);
});

app.openapi(createRoute({ method: "post", path: "/v1/payment-links/{merchantId}/{linkId}/checkouts", tags, summary: "Create a buyer checkout; keep the random UUIDv4 Idempotency-Key secret for recovery", request: { ...body(linkCheckoutSchema), params: linkParams, headers: z.object({ "idempotency-key": z.uuid({ version: "v4" }) }) }, responses }), async (c) => {
  const { merchantId, linkId } = c.req.valid("param");
  const link = await callObject<PaymentLink>(paymentConfig(c.env).merchants, merchantId, "/get-link", { id: linkId });
  const { payer } = c.req.valid("json");
  const reference = digest(canonicalJson([payer, c.req.valid("header")["idempotency-key"]])).slice(0, 32);
  const { amount, cluster, description, resource, requestHash, expiresInSeconds } = link;
  const input = checkoutSchema.parse({
    amount, cluster, expiresInSeconds, payer, externalReference: `link:${linkId}:${reference}`,
    ...(description !== undefined ? { description } : {}),
    ...(resource !== undefined ? { resource } : {}),
    ...(requestHash !== undefined ? { requestHash } : {}),
  });
  return c.json(await createCheckout(c, merchantId, input), 200);
});

app.openapi(createRoute({ method: "get", path: "/v1/payments/{id}", tags, summary: "Read/reconcile a payment using its merchant key or checkout access token", request: { params: paymentParams }, responses }), async (c) => {
  return c.json(await ledger<PaymentView>(c, c.req.valid("param").id, "/status", await authorization(c)), 200);
});

app.openapi(createRoute({ method: "post", path: "/v1/payments/{id}/prepare", tags, summary: "Prepare one fee-free ER transfer for signing; the prepared message is immutable", request: { params: paymentParams }, responses }), async (c) => {
  const result = await ledger<{ payment: PaymentView; prepared: PreparedPayment }>(c, c.req.valid("param").id, "/prepare", await authorization(c));
  return c.json(publicPreparation(result.payment, result.prepared), 200);
});

app.openapi(createRoute({ method: "post", path: "/v1/payments/{id}/settle", tags, summary: "Submit an exact signed payment; pending responses are not proof of payment", request: { ...body(settleSchema), params: paymentParams }, responses }), async (c) => {
  const payment = await ledger<PaymentView>(c, c.req.valid("param").id, "/settle", { ...await authorization(c), ...c.req.valid("json") });
  return c.json(payment, payment.status === "paid" ? 200 : payment.status === "pending" ? 202 : 409);
});

app.route("/", x402);
app.route("/", mpp);

export default app;
