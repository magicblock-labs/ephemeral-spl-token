import { createRoute, OpenAPIHono } from "@hono/zod-openapi";
import { getEnv } from "../env";
import { openApiDefaultHook } from "../lib/create-app";
import { ApiError } from "../lib/errors";
import { validatePaymentCluster } from "../payments/chain";
import { paymentConfig } from "../payments/config";
import { authorization, body, jsonResponse, ledger, merchant, paymentParams, protocolCredential, protocolInput, responses } from "../payments/http";
import type { Env } from "../payments/http";
import {
  createX402PaymentRequired, decodeX402Payload, encodePaymentJson, encodeX402PaymentRequired,
  paymentNetwork, validateX402Payload, X402_SCHEME, x402Settlement,
} from "../payments/protocols";
import { facilitatorSchema } from "../payments/schemas";
import type { PaymentRecord, PaymentView, PreparedPayment } from "../payments/types";

const app = new OpenAPIHono<Env>({ defaultHook: openApiDefaultHook });
const tags = ["x402"];
const checkoutResponses = {
  ...responses,
  402: { ...jsonResponse, description: "x402 payment required; see PAYMENT-REQUIRED" },
};

app.openapi(createRoute({ method: "post", path: "/v1/x402/payments/{id}/pay", tags, summary: "x402 checkout: get a challenge, then retry with PAYMENT-SIGNATURE", request: { params: paymentParams }, responses: checkoutResponses }), async (c) => {
  const id = c.req.valid("param").id;
  const auth = await authorization(c);
  const credential = protocolCredential(c.req.raw.headers, "x402");
  if (!credential) {
    const { payment, prepared } = await ledger<{ payment: PaymentView; prepared: PreparedPayment }>(c, id, "/prepare", auth);
    if (payment.status === "paid" || payment.status === "pending") return c.json(payment, payment.status === "paid" ? 200 : 202);
    c.header("PAYMENT-REQUIRED", encodeX402PaymentRequired(payment, prepared));
    return c.json(createX402PaymentRequired(payment, prepared), 402);
  }
  const record = await ledger<PaymentRecord>(c, id, "/read", auth);
  if (!record.prepared) throw new ApiError(409, "PAYMENT_NOT_PREPARED", "Request a payment challenge first");
  const proof = protocolInput(() => validateX402Payload(credential, record, record.prepared!, Date.now(), {
    allowExpired: record.status === "paid" || record.status === "pending" || record.status === "failed",
  }));
  const payment = await ledger<PaymentView>(c, id, "/settle", { ...auth, transactionBase64: proof.transactionBase64 });
  c.header("PAYMENT-RESPONSE", encodePaymentJson(x402Settlement(payment)));
  return c.json(payment, payment.status === "paid" ? 200 : payment.status === "pending" ? 202 : 409);
});

app.openapi(createRoute({ method: "get", path: "/v1/x402/supported", tags, summary: "Supported custom MagicBlock payment schemes", responses }), (c) => {
  paymentConfig(c.env);
  const env = getEnv(c.env);
  const clusters = (["mainnet", "mainnet-private", "devnet", "devnet-private"] as const).filter((cluster) => {
    try {
      validatePaymentCluster(env, cluster);
      return true;
    } catch (error) {
      if (error instanceof ApiError && ["CONFIG_ERROR", "PAYMENT_RPC_AUTH_UNAVAILABLE"].includes(error.code)) return false;
      throw error;
    }
  });
  return c.json({ kinds: clusters.map(cluster => ({ x402Version: 2, scheme: X402_SCHEME, network: paymentNetwork(cluster), extra: { cluster, settlement: "ephemeral-rollup", paymentFlow: "upfront" } })), extensions: ["magicblock"], signers: {} }, 200);
});

for (const action of ["verify", "settle"] as const) {
  app.openapi(createRoute({ method: "post", path: `/v1/x402/${action}`, tags, summary: `x402 facilitator ${action}; requires merchant API credentials`, request: body(facilitatorSchema), responses }), async (c) => {
    const { merchantId } = await merchant(c);
    const input = c.req.valid("json");
    const decoded = protocolInput(() => decodeX402Payload(input.paymentPayload));
    const record = await ledger<PaymentRecord>(c, decoded.paymentId, "/read", { merchantId });
    if (!record.prepared) throw new ApiError(409, "PAYMENT_NOT_PREPARED", "Prepare payment before submitting a proof");
    const auth = { merchantId, transactionBase64: decoded.transactionBase64 };
    try {
      protocolInput(() => validateX402Payload(input.paymentPayload, record, record.prepared!, Date.now(), {
        allowExpired: record.status === "paid" || record.status === "pending" || record.status === "failed",
        paymentRequirements: input.paymentRequirements,
      }));
      if (action === "verify") return c.json(await ledger<{ isValid: boolean; payer: string }>(c, record.id, "/verify", auth), 200);
    } catch (error) {
      if (action === "verify" && error instanceof ApiError && [400, 409].includes(error.status)) {
        return c.json({ isValid: false, invalidReason: error.code, invalidMessage: error.message, payer: record.payer }, 200);
      }
      throw error;
    }
    return c.json(x402Settlement(await ledger<PaymentView>(c, record.id, "/settle", auth)), 200);
  });
}

export default app;
