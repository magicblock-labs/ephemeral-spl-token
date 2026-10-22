import { createRoute, OpenAPIHono } from "@hono/zod-openapi";
import { openApiDefaultHook } from "../lib/create-app";
import { ApiError } from "../lib/errors";
import { paymentConfig } from "../payments/config";
import { authorization, jsonResponse, ledger, paymentParams, protocolCredential, protocolInput, responses } from "../payments/http";
import type { Env } from "../payments/http";
import { createMppChallenge, encodeMppChallenge, encodeMppReceipt, validateMppCredential } from "../payments/protocols";
import type { PaymentRecord, PaymentView, PreparedPayment } from "../payments/types";

const app = new OpenAPIHono<Env>({ defaultHook: openApiDefaultHook });
const tags = ["MPP"];
const checkoutResponses = {
  ...responses,
  402: { ...jsonResponse, description: "MPP payment required; see WWW-Authenticate: Payment" },
};

app.openapi(createRoute({ method: "post", path: "/v1/mpp/payments/{id}/pay", tags, summary: "MPP checkout: get a challenge, then retry with Payment-Authorization", request: { params: paymentParams }, responses: checkoutResponses }), async (c) => {
  const id = c.req.valid("param").id;
  const auth = await authorization(c);
  const credential = protocolCredential(c.req.raw.headers, "mpp");
  if (!credential) {
    const { payment, prepared } = await ledger<{ payment: PaymentView; prepared: PreparedPayment }>(c, id, "/prepare", auth);
    if (payment.status === "paid" || payment.status === "pending") return c.json(payment, payment.status === "paid" ? 200 : 202);
    const challenge = createMppChallenge(payment, prepared, paymentConfig(c.env).realm);
    c.header("WWW-Authenticate", encodeMppChallenge(challenge));
    return c.json({ paymentId: payment.id, challenge }, 402);
  }
  const record = await ledger<PaymentRecord>(c, id, "/read", auth);
  if (!record.prepared) throw new ApiError(409, "PAYMENT_NOT_PREPARED", "Request a payment challenge first");
  const proof = protocolInput(() => validateMppCredential(credential, record, record.prepared!, paymentConfig(c.env).realm, Date.now(), {
    allowExpired: record.status === "paid" || record.status === "pending" || record.status === "failed",
  }));
  const payment = await ledger<PaymentView>(c, id, "/settle", { ...auth, transactionBase64: proof.transactionBase64 });
  if (payment.status === "paid") c.header("Payment-Receipt", encodeMppReceipt(payment));
  return c.json(payment, payment.status === "paid" ? 200 : payment.status === "pending" ? 202 : 409);
});

export default app;
