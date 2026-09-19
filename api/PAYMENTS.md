# Payment checkouts

This API supports [x402 v2](https://x402.org/) and [MPP (Machine Payments Protocol)](https://mpp.dev/) for selling API access, credits and other products using already-delegated USDC on MagicBlock Ephemeral Rollups. Start with the [x402 integration](#x402) or [MPP integration](#mpp): each has its own routes and examples, with shared checkout and payment status.

The integrations use custom MagicBlock payment methods: x402 scheme `exact-magicblock` and MPP method `magicblock` with intent `charge`. The service registers merchants, creates orders and reusable offers, prepares transfers, verifies buyer signatures, and tracks settlement. Products, customers, credits, subscriptions, refunds and fulfillment remain in the merchant's service. Checkout is API-only, with no hosted UI or dashboard.

Each payment contains one ordinary SPL transfer between projected associated token accounts plus a `payment:<paymentId>` memo. Both eATAs must already be delegated to the selected validator. The buyer signs and is the transaction fee payer; preparation and submission require `getFeeForMessage` to return exactly zero. No sponsorship, relay fee, account creation, deposit or withdrawal occurs during checkout.

## Configure the Worker

Provision these settings before deploying the payment service:

| Setting | Purpose |
| --- | --- |
| `PAYMENTS_PUBLIC_URL` | Canonical HTTPS origin, such as `https://payments.example.com`; no path, query, credentials or fragment. HTTP is allowed for `localhost` and `127.0.0.1` development. |
| `PAYMENTS_SECRET` | Random server secret, at least 32 characters. Generate at least 32 random bytes and encode them as hex or base64url. Derives per-checkout bearer capabilities using HMAC-SHA256. |
| `PAYMENTS_RPC_AUTH_SECRET_KEY` | Required for private clusters only: a dedicated Solana service keypair as a JSON array of 64 secret-key bytes. Authenticates RPC requests; never signs payments or sponsors fees. |
| `PAYMENT_MERCHANTS` | Durable Object binding for merchant registration, credential hashes and reusable links. |
| `PAYMENT_LEDGER` | Durable Object binding for payment state and recovery alarms, sharded per payment. |

Use Worker secrets for both secrets; never send them to merchants or buyers. Keep `PAYMENTS_SECRET` stable: checkout tokens are derived from it and their hashes are stored with orders. Rotating it requires a capability-migration plan; simply changing it prevents repeated creation requests from recovering the original token.

[`wrangler.jsonc`](./wrangler.jsonc) declares both bindings and the `v1-payment-checkouts` SQLite Durable Object migration. [`src/index.ts`](./src/index.ts) exports their classes. Preserve deployed migration history and provision the new configuration in every deployment environment. Payment routes return `503` if payment configuration is incomplete. Existing SPL routes do not require payment configuration.

RPCs come exclusively from Worker configuration:

The default checkout target is the mainnet TEE. Configure:

```dotenv
EPHEMERAL_TEE_RPC_URL=https://mainnet-tee.magicblock.app
```

Use `cluster: "mainnet-private"` (the checkout default) for both public and private accounts on this validator. The cluster selects the TEE endpoint and service authentication; account permissions determine which data is private. The public node can execute payments too, but it does not preserve payment privacy.

| `cluster` | Base RPC | Payment RPC |
| --- | --- | --- |
| `mainnet` | `BASE_RPC_URL` | `EPHEMERAL_RPC_URL` |
| `mainnet-private` | `BASE_RPC_URL` | `EPHEMERAL_TEE_RPC_URL` |
| `devnet` | `BASE_DEVNET_RPC_URL` | `EPHEMERAL_DEVNET_RPC_URL` |
| `devnet-private` | `BASE_DEVNET_RPC_URL` | `EPHEMERAL_DEVNET_TEE_RPC_URL` |

The existing environment schema also requires `BASE_RPC_URL` and `EPHEMERAL_RPC_URL`. Payment APIs accept only the four named clusters; callers cannot supply RPC URLs. Use `cluster: "devnet-private"` explicitly for devnet testing. Mainnet USDC is `EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v`; devnet USDC is `4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU`. Both use six decimals.

## Register and recover merchant access

1. `POST /v1/merchants/challenge` with `{ "wallet": "<merchant-wallet>", "purpose": "register" }`.
2. Sign the returned `message` as exact UTF-8 bytes using the merchant wallet's Ed25519 message signer.
3. `POST /v1/merchants` with `{ "wallet": "<merchant-wallet>", "challengeId": "<id>", "signature": "<base64-signature>" }`.

The response contains `merchantId`, `wallet`, and `apiKey`. The merchant ID and receiving wallet are the registration wallet in v1. Store the API key on the merchant server; the API stores only its hash. Registration is off-chain.

```ts
import { Buffer } from "buffer";

const challengeResponse = await fetch(`${paymentsOrigin}/v1/merchants/challenge`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ wallet: merchantWalletAddress, purpose: "register" }),
});
if (!challengeResponse.ok) throw new Error("Challenge request failed");
const challenge = await challengeResponse.json();
const signatureBytes = await wallet.signMessage(new TextEncoder().encode(challenge.message));
const registration = await fetch(`${paymentsOrigin}/v1/merchants`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    wallet: merchantWalletAddress,
    challengeId: challenge.id,
    signature: Buffer.from(signatureBytes).toString("base64"),
  }),
});
if (!registration.ok) throw new Error("Registration failed");
const merchant = await registration.json(); // Deliver apiKey securely to your server.
```

Challenges bind the configured origin, wallet, purpose, random nonce and five-minute expiry. They are consumed on success. Retrying an unexpired challenge request returns the same challenge. Signatures use standard padded base64, not base58 or a transaction signature.

To recover a lost key or rotate it, request a challenge with `purpose: "rotate-key"`, sign its exact message, then submit the same proof shape to `POST /v1/merchants/rotate-key`. The new key immediately invalidates the previous key. Re-registering an existing wallet does not reveal or reset its key.

Merchant-authenticated requests use both headers:

```http
Authorization: Bearer <merchant-api-key>
X-Merchant-Id: <merchant-wallet>
```

## Create an order checkout

The merchant server chooses the price and creates a checkout:

```http
POST /v1/payments
Authorization: Bearer <merchant-api-key>
X-Merchant-Id: <merchant-wallet>
Content-Type: application/json

{
  "payer": "<buyer-wallet>",
  "externalReference": "order_123",
  "amount": "1000000",
  "cluster": "mainnet-private",
  "description": "100 credits",
  "resource": "https://merchant.example/credits/purchase",
  "expiresInSeconds": 300
}
```

`amount` is a positive integer string in USDC base units: `"1000000"` means 1 USDC. The merchant's registered wallet receives the payment. `description`, `resource`, and the SHA-256 hex `requestHash` are optional. `requestHash` binds a merchant-defined request body digest; the merchant must compute and check it against its own request. `expiresInSeconds` accepts 60–86,400 and defaults to 3,600.

The response contains `payment`, `accessToken`, `checkoutUrl` and separate `paymentUrls.x402` and `paymentUrls.mpp` URLs. Return only the appropriate checkout details to that buyer. Its `accessToken` grants payment read, preparation and settlement access; use it as `Authorization: Bearer <accessToken>`, without `X-Merchant-Id`. Keep it out of URLs, analytics and logs. Knowing a payment ID or checkout URL alone does not authorize access.

`externalReference` is unique per merchant. Repeating creation with identical inputs returns the same payment and token, including its original expiry. Changing the payer, amount or other terms under the same reference returns `409 PAYMENT_ORDER_CONFLICT`. Persist the order-to-payment mapping on your server before fulfilling anything.

## Prepare, sign and settle

The direct JSON flow is:

1. `POST /v1/payments/{id}/prepare` using buyer or merchant credentials. The response contains `payment`, `transactionBase64`, `recentBlockhash`, `lastValidBlockHeight`, `validator`, and zero-fee metadata.
2. Deserialize the legacy transaction, inspect its recipient, amount, mint-derived accounts and memo, then add the buyer's signature without changing the message.
3. `POST /v1/payments/{id}/settle` with `{ "transactionBase64": "<signed-base64>" }` using the same credentials.
4. If status is pending, poll `GET /v1/payments/{id}`; do not create another payment or sign a replacement.

The API verifies the exact stored message and every signature before submitting. It durably stores the signed bytes and signature before broadcast. Concurrent retries, protocol switches and recovery alarms use the same transaction. The configured endpoint and validator identity must still match the prepared payment.

## Choose a protocol

Registration, checkouts, links, preparation and payment status are shared. Each protocol has its own endpoint and returns only its own challenges and receipts. The API reference places these routes after the core API in **Merchant checkout · x402 / MPP**, with **Merchants & checkouts**, **x402** and **MPP** sections. Sidebar sections start collapsed by default; expand a section to browse its endpoints. The underlying OpenAPI tags remain `Payments`, `x402` and `MPP`.

| Group | Endpoint | Purpose |
| --- | --- | --- |
| Payments | `POST /v1/payments` | Create the shared checkout. |
| Payments | `POST /v1/payments/{id}/prepare` | Obtain the immutable transaction and validator. |
| Payments | `POST /v1/payments/{id}/settle` | Optional direct JSON settlement. |
| Payments | `GET /v1/payments/{id}` | Shared authenticated payment status. |
| x402 | `POST /v1/x402/payments/{id}/pay` | x402 challenge and credential retry; `paymentUrls.x402`. |
| x402 | `GET /v1/x402/supported` | Advertise supported schemes. |
| x402 | `POST /v1/x402/verify` | Merchant-authenticated proof verification. |
| x402 | `POST /v1/x402/settle` | Merchant-authenticated facilitator settlement. |
| MPP | `POST /v1/mpp/payments/{id}/pay` | MPP challenge and credential retry; `paymentUrls.mpp`. |

Both protocols reference the same payment ID and ledger, so switching protocols cannot redeem the checkout twice. Each endpoint rejects the other protocol's credential headers. These are custom MagicBlock methods: stock Solana x402/MPP clients do not automatically understand delegated accounts, ER routing or ER settlement.

[`src/payments/client.ts`](./src/payments/client.ts) provides `createPaymentCredential` as a repository helper; it is not a published client package. It checks the envelope and transfer against approved checkout terms, calls the wallet, and rejects a wallet-modified message. The two examples below share this setup after your merchant service has supplied trusted `checkout` details and the user has approved the price:

```ts
import { createPaymentCredential } from "./src/payments/client";

const authorization = { Authorization: `Bearer ${checkout.accessToken}` };
const preparedResponse = await fetch(`${checkout.checkoutUrl}/prepare`, {
  method: "POST", headers: authorization,
});
if (!preparedResponse.ok) throw new Error("Payment preparation failed");
const prepared = await preparedResponse.json();
```

Use your configured Payments API origin when accepting checkout URLs. The helper does not choose a merchant or approve a price for the user.

## x402

Use `POST /v1/x402/payments/{id}/pay` with the checkout token or merchant credentials. An unpaid checkout returns `402` with a `PAYMENT-REQUIRED` header and x402 v2 JSON body advertising scheme `exact-magicblock`.

| Step | Header |
| --- | --- |
| API access on both requests | `Authorization: Bearer <checkout-token>` |
| `402` challenge | `PAYMENT-REQUIRED: <base64-payment-requirements>` |
| Signed retry | `PAYMENT-SIGNATURE: <base64-payment-payload>` |
| Settlement result | `PAYMENT-RESPONSE: <base64-settlement-result>` |

Using the shared setup above:

```ts
const x402Url = checkout.paymentUrls.x402;
const challenge = await fetch(x402Url, { method: "POST", headers: authorization });

if (challenge.status === 402) {
  const credential = await createPaymentCredential({
    protocol: "x402",
    headers: challenge.headers,
    payment: checkout.payment,
    validator: prepared.validator,
    realm: new URL(paymentsOrigin).host,
    signTransaction: transaction => wallet.signTransaction(transaction),
  });
  const response = await fetch(x402Url, {
    method: "POST",
    headers: { ...authorization, "PAYMENT-SIGNATURE": credential.headerValue },
  });
  if (![200, 202].includes(response.status)) throw new Error("x402 payment rejected");
  const payment = await response.json();
  // A pending result is not a successful payment. Poll checkout.checkoutUrl.
} else if (![200, 202].includes(challenge.status)) {
  throw new Error("x402 challenge failed");
}
```

For merchant-hosted resource middleware, `GET /v1/x402/supported` advertises configured methods. Merchant-authenticated `POST /v1/x402/verify` and `/v1/x402/settle` accept `{ "x402Version": 2, "paymentPayload": <decoded-proof>, "paymentRequirements": <accepted-requirement> }`. Verification only validates the prepared proof; it never establishes payment or grants access. Fulfill only after settlement reports success and the authenticated payment record is `paid`.

## MPP

Use `POST /v1/mpp/payments/{id}/pay` with the checkout token or merchant credentials. An unpaid checkout returns `402` with `WWW-Authenticate: Payment …`, advertising method `magicblock` and intent `charge`.

| Step | Header |
| --- | --- |
| API access on both requests | `Authorization: Bearer <checkout-token>` |
| `402` challenge | `WWW-Authenticate: Payment …` |
| Signed retry | `Payment-Authorization: Payment <base64url-credential>` |
| Confirmed success only | `Payment-Receipt: <base64url-receipt>` |

The challenge explicitly advertises `Payment-Authorization` so the payment credential does not replace the API's bearer credential. Do not send the MPP credential in `Authorization` or use x402 headers on this endpoint.

Using the same shared checkout and preparation setup, choose MPP as follows:

```ts
const mppUrl = checkout.paymentUrls.mpp;
const challenge = await fetch(mppUrl, { method: "POST", headers: authorization });

if (challenge.status === 402) {
  const credential = await createPaymentCredential({
    protocol: "mpp",
    headers: challenge.headers,
    payment: checkout.payment,
    validator: prepared.validator,
    realm: new URL(paymentsOrigin).host,
    signTransaction: transaction => wallet.signTransaction(transaction),
  });
  const response = await fetch(mppUrl, {
    method: "POST",
    headers: { ...authorization, "Payment-Authorization": credential.headerValue },
  });
  if (![200, 202].includes(response.status)) throw new Error("MPP payment rejected");
  const payment = await response.json();
  // Payment-Receipt is present only after paid; poll checkout.checkoutUrl if pending.
} else if (![200, 202].includes(challenge.status)) {
  throw new Error("MPP challenge failed");
}
```

MPP has no separate facilitator endpoints in v1. Your merchant server uses the shared `GET /v1/payments/{id}` endpoint to verify payment before fulfillment, exactly as with x402.

## Reusable public links

The merchant creates an offer with `POST /v1/payment-links` using the order fields above except `payer`. Here `externalReference` identifies the reusable offer, such as `credits-100`. Creation retries with the same terms return the same link; changes conflict. The returned `url` serves public JSON, not a checkout page. Offer metadata is public; do not put customer details or secrets there.

Anyone can read `GET /v1/payment-links/{merchantId}/{linkId}` and create an individual buyer checkout:

```http
POST /v1/payment-links/{merchantId}/{linkId}/checkouts
Idempotency-Key: <random-UUIDv4>
Content-Type: application/json

{ "payer": "<buyer-wallet>" }
```

This public endpoint returns the ordinary checkout response. Generate a random UUIDv4 with `crypto.randomUUID()` and persist it as the required `Idempotency-Key` once per purchase; reuse it on retries. The link, payer and key together identify the checkout and allow recovery of its bearer token. Treat the UUID as a secret capability: anyone knowing it and the public payer/link can recover that checkout. A new key creates a new purchase. Link checkout `externalReference` has the format `link:<linkId>:<32hex purchase hash>`, so fulfillment can recover the source offer from `linkId`. Track customer/order association in your service.

## Status, fulfillment and private payments

| Status | Meaning |
| --- | --- |
| `created` | Awaiting an accepted signed transaction. |
| `pending` | Submission or reconciliation is unresolved. No fulfillment yet. |
| `paid` | The exact validated transfer succeeded with confirmed/finalized status on the selected ER. |
| `failed` | The ER returned a confirmed/finalized execution error. |
| `expired` | The checkout expired before the API accepted a signed payment. |

A `200` response alone is not proof of payment. The merchant retrieves `GET /v1/payments/{id}` with its own credentials, verifies the payment's amount, payer and order reference, then atomically inserts a fulfillment record with a unique `payment.id` and grants the product in its own database. A retry after payment but before delivery must complete that same fulfillment once. There are no webhooks or product/credit balances in this API.

`paid` explicitly means **ER settlement**, not L1 commitment or withdrawal. The buyer and merchant need prepared accounts on the same zero-fee validator. Private clusters additionally require appropriate account permissions on a TEE-backed private ER; choosing a private string is not account onboarding.

The service reads public base-chain delegation records during preparation, then uses its own authenticated private RPC session to submit the buyer-signed bytes and poll `getSignatureStatuses`. It does not read private token balances, fetch full transactions, retain buyer login tokens or need the merchant to inspect the buyer's account. The service still sees the sender, recipient and amount in the transaction it processes. Payment status is restricted to the merchant or checkout-token holder.

The local private-gateway source allows signature-status queries without buyer account-read access, and the implementation is tested with mocked RPC responses. **A real private transfer using a separate service identity, including a zero-SOL buyer, remains a deployment acceptance check.** Local tests do not establish the deployed gateway's behavior.

Preparation fixes one blockhash/message for the entire checkout. Prepare only when the buyer is ready to sign; checkout expiry does not extend the blockhash's validity. The API does not regenerate an issued transaction, replace a pending charge, or infer failure from a missing signature. Timeouts, RPC access/history failures and unknown outcomes stay `pending`; background alarms retry the exact bytes and reconcile with bounded backoff. Broadcast stops after `lastValidBlockHeight`, while signature-status reconciliation continues because the original transaction may already have landed. An unavailable original endpoint or a changed validator identity also prevents automatic settlement confirmation.

An expired blockhash or lost transaction history can therefore require operational reconciliation. Do not automatically issue a replacement purchase while the original outcome is unknown. Checkout expiry is an API acceptance deadline, not an on-chain cancellation instruction; buyers should submit through the API within that window.

Run `yarn typecheck`, `yarn lint` and `yarn test` for local validation. Before enabling production fulfillment, verify successful public/private settlement, zero-SOL operation, interrupted-request recovery and exactly-once delivery against the configured deployed services.
