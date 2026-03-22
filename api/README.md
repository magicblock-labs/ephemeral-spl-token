# SPL Tokens API

Minimal REST API for building unsigned SPL token transactions for the local Ephemeral Rollups SDK.

This project is designed to:

- run on a Cloudflare Worker
- be easy to test locally with `wrangler dev`
- expose a small, documented REST surface
- return serialized unsigned transactions that a client can complete, sign, and send

The API uses the published SDK package:

- `@magicblock-labs/ephemeral-rollups-sdk@0.10.1`

## What The API Does

The API exposes:

- `POST /v1/spl/deposit`
- `POST /v1/spl/withdraw`
- `POST /v1/spl/transfer`
- `GET /v1/spl/balance`
- `GET /v1/spl/private-balance`
- `GET /mcp`
- `POST /mcp`
- `GET /.well-known/mcp.json`

Transaction endpoints return an unsigned serialized legacy Solana transaction as base64 plus metadata such as:

- `sendTo`: where the client should submit the signed transaction, `"base"` or `"ephemeral"`
- `recentBlockhash`
- `lastValidBlockHeight`
- `requiredSigners`

Important behavior:

- `deposit` is a REST wrapper around the SDK `delegateSpl(...)` flow
- `deposit` defaults `validator` to `MAS1Dt9qreoRMQ14YQuhg8UTZMMzDdKhmkZMECCzk57` on mainnet
- `deposit` generates `shuttleId` server-side and always uses `escrowIndex = 0`
- `withdraw` uses `withdrawSpl(...)`
- `transfer` uses `transferSpl(...)`
- every SPL endpoint accepts an optional `cluster` parameter
- `cluster=mainnet` uses `BASE_RPC_URL` and `EPHEMERAL_RPC_URL`
- `cluster=devnet` uses `BASE_DEVNET_RPC_URL` and `EPHEMERAL_DEVNET_RPC_URL`
- any other valid `cluster` value is treated as a custom RPC URL and used for both base and ephemeral RPCs
- the API fetches the recent blockhash itself
- if `fromBalance` is `"base"`, the blockhash is fetched from the base RPC
- if `fromBalance` is `"ephemeral"`, the blockhash is fetched from the ephemeral RPC
- `getBalance` reads the owner ATA on the base RPC
- `getPrivateBalance` reads the owner ATA on the ephemeral RPC

## Stack

- [Hono](https://hono.dev/)
- `@hono/zod-openapi`
- Scalar API Reference
- Cloudflare Workers via Wrangler
- `@solana/web3.js`
- `@magicblock-labs/ephemeral-rollups-sdk`

## Project Layout

- [src/app.ts](./src/app.ts): app composition
- [src/lib/solana.ts](./src/lib/solana.ts): SDK integration, transaction building, RPC logic
- [src/routes/mcp.route.ts](./src/routes/mcp.route.ts): MCP server endpoint and tool registration
- [src/routes/spl/spl.routes.ts](./src/routes/spl/spl.routes.ts): OpenAPI route definitions
- [src/routes/spl/spl.schemas.ts](./src/routes/spl/spl.schemas.ts): request and response schemas
- [src/routes/spl/spl.handlers.ts](./src/routes/spl/spl.handlers.ts): route handlers
- [src/app.test.ts](./src/app.test.ts): local tests

## Prerequisites

- Node.js 18+ recommended
- Yarn 1.x available

## Environment Variables

Copy the example file:

```bash
cp .dev.vars.example .dev.vars
```

Variables:

- `BASE_RPC_URL`: mainnet base Solana RPC used when `cluster` is omitted or set to `mainnet`
- `EPHEMERAL_RPC_URL`: mainnet ephemeral RPC used when `cluster` is omitted or set to `mainnet`
- `BASE_DEVNET_RPC_URL`: devnet base Solana RPC used when `cluster=devnet`
- `EPHEMERAL_DEVNET_RPC_URL`: devnet ephemeral RPC used when `cluster=devnet`
- `CORS_ORIGIN`: CORS origin, `*` by default
- `VALIDATOR_PUBKEY`: optional validator pubkey override for routes that do not pin their own default

If `VALIDATOR_PUBKEY` is omitted, non-deposit flows resolve it from the ephemeral RPC using `getIdentity`.

Example:

```dotenv
BASE_RPC_URL=https://rpc.magicblock.app/mainnet
EPHEMERAL_RPC_URL=https://mainnet.magicblock.app
BASE_DEVNET_RPC_URL=https://rpc.magicblock.app/devnet
EPHEMERAL_DEVNET_RPC_URL=https://devnet.magicblock.app
CORS_ORIGIN=*
VALIDATOR_PUBKEY=
```

## Run Locally

Install dependencies:

```bash
yarn install
```

Create local env:

```bash
cp .dev.vars.example .dev.vars
```

If you see `CONFIG_ERROR`, check that `.dev.vars` exists in this directory and contains `BASE_RPC_URL` and `EPHEMERAL_RPC_URL`. If you use `cluster=devnet`, also set `BASE_DEVNET_RPC_URL` and `EPHEMERAL_DEVNET_RPC_URL`.

Start the worker locally:

```bash
yarn dev
```

Wrangler will start the worker locally, typically on:

- `http://127.0.0.1:8787`

Useful routes:

- `GET /health`
- `GET /doc`
- `GET /reference`
- `GET /mcp`
- `POST /mcp`
- `GET /.well-known/mcp.json`

Examples:

```bash
curl http://127.0.0.1:8787/health
curl http://127.0.0.1:8787/doc
curl http://127.0.0.1:8787/reference
```

## Scripts

- `yarn dev`: start local worker development server
- `yarn build`: TypeScript check
- `yarn typecheck`: TypeScript check
- `yarn test`: run Vitest suite
- `yarn deploy`: deploy with Wrangler, uploading Worker secrets from `.dev.vars`

## API Documentation

When the app is running locally:

- OpenAPI JSON: `http://127.0.0.1:8787/doc`
- Scalar reference UI: `http://127.0.0.1:8787/reference`
- MCP info: `http://127.0.0.1:8787/mcp`
- MCP endpoint: `POST http://127.0.0.1:8787/mcp`
- MCP discovery: `http://127.0.0.1:8787/.well-known/mcp.json`

## MCP Server

The worker also exposes a stateless MCP server at:

- `GET /mcp`: human-friendly info document
- `POST /mcp`: MCP Streamable HTTP endpoint
- `GET /.well-known/mcp.json`: discovery document

Exposed MCP tools:

- `spl.deposit`
- `spl.withdraw`
- `spl.transfer`
- `spl.getBalance`
- `spl.getPrivateBalance`

The MCP tools reuse the same transaction and balance builders as the REST API, so the behavior and validation stay aligned.

Implementation details:

- each `POST /mcp` request creates a fresh `McpServer`, connects it to `WebStandardStreamableHTTPServerTransport`, handles the JSON-RPC exchange, then closes it
- the transport is configured with `sessionIdGenerator: undefined` and `enableJsonResponse: true`
- this server is stateless, so it does not issue or require `mcp-session-id` headers
- normal browsers can open `GET /mcp` to see how to connect
- MCP clients should use `POST /mcp`
- for compatibility with Streamable HTTP clients, `GET /mcp` returns `405` when the request asks for `text/event-stream`
- `POST /mcp` works with `Accept: application/json` and `Content-Type: application/json`

`GET /mcp` example:

```bash
curl http://127.0.0.1:8787/mcp
```

```json
{
  "name": "spl-private-payments-api",
  "version": "0.1.0",
  "transport": "streamable-http",
  "mode": "stateless-json-response",
  "endpoint": "http://127.0.0.1:8787/mcp",
  "discovery": "http://127.0.0.1:8787/.well-known/mcp.json",
  "methods": [
    "POST"
  ],
  "inspector": {
    "transport": "Streamable HTTP",
    "url": "http://127.0.0.1:8787/mcp"
  },
  "tools": [
    {
      "name": "spl.deposit",
      "description": "Build an unsigned base-chain deposit transaction using delegateSpl(...)."
    },
    {
      "name": "spl.withdraw",
      "description": "Withdraw SPL tokens from an ephemeral rollup back to Solana."
    },
    {
      "name": "spl.transfer",
      "description": "Build an unsigned SPL transfer transaction using transferSpl(...)."
    },
    {
      "name": "spl.getBalance",
      "description": "Read the owner ATA balance on the base RPC."
    },
    {
      "name": "spl.getPrivateBalance",
      "description": "Read the owner ATA balance on the ephemeral RPC."
    }
  ]
}
```

Example with the official TypeScript MCP client:

```ts
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";

const client = new Client({
  name: "local-example",
  version: "1.0.0",
});

const transport = new StreamableHTTPClientTransport(
  new URL("http://127.0.0.1:8787/mcp"),
);

await client.connect(transport);

const tools = await client.listTools();
const result = await client.callTool({
  name: "spl.getBalance",
  arguments: {
    address: "Bt9oNR5cCtnfuMmXgWELd6q5i974PdEMQDUE55nBC57L",
    mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
  },
});
```

Manual HTTP flow for this implementation:

1. Initialize:

```bash
curl -X POST http://127.0.0.1:8787/mcp \
  -H 'content-type: application/json' \
  -H 'accept: application/json' \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": {
      "protocolVersion": "2025-11-25",
      "capabilities": {},
      "clientInfo": {
        "name": "curl-example",
        "version": "1.0.0"
      }
    }
  }'
```

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocolVersion": "2025-11-25",
    "capabilities": {
      "tools": {
        "listChanged": true
      }
    },
    "serverInfo": {
      "name": "spl-private-payments-api",
      "version": "0.1.0"
    }
  }
}
```

2. Send `notifications/initialized`:

```bash
curl -i -X POST http://127.0.0.1:8787/mcp \
  -H 'content-type: application/json' \
  -H 'accept: application/json' \
  -H 'mcp-protocol-version: 2025-11-25' \
  -d '{
    "jsonrpc": "2.0",
    "method": "notifications/initialized"
  }'
```

This returns `HTTP 202` with no body.

3. Call a tool:

```bash
curl -X POST http://127.0.0.1:8787/mcp \
  -H 'content-type: application/json' \
  -H 'accept: application/json' \
  -H 'mcp-protocol-version: 2025-11-25' \
  -d '{
    "jsonrpc": "2.0",
    "id": 3,
    "method": "tools/call",
    "params": {
      "name": "spl.deposit",
      "arguments": {
        "owner": "3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE",
        "amount": 1,
        "initIfMissing": true,
        "initAtasIfMissing": true,
        "initVaultIfMissing": true,
        "idempotent": true
      }
    }
  }'
```

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": {
    "content": [
      {
        "type": "text",
        "text": "Built an unsigned SPL deposit transaction."
      }
    ],
    "structuredContent": {
      "kind": "deposit",
      "version": "legacy",
      "transactionBase64": "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAIDKmcfsS5XfSOLaLlaBHJry50iH2Ufk2TMz4STC2fHzIcFKkerg3q2DD3Yn8TISmGeKoxSLz+BiP7iQ4pYqXYXsgu8D8C7R8ovdMQRLpSrE8+jxjTl3BfqywPNGiPNfnh8eS+smowIxqKDcCjw5liNXQkkCbBSDCBDFwtrgCKqoQ0DAgEBBAECAwQCAQEEAgIDBAIBAQQDAgME",
      "sendTo": "base",
      "recentBlockhash": "9A4VhP8M8fQZxP4h7rB6mP6eM8w2pJkYh7QdZk7V4r2x",
      "lastValidBlockHeight": 284512337,
      "instructionCount": 3,
      "requiredSigners": [
        "3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE"
      ],
      "validator": "MAS1Dt9qreoRMQ14YQuhg8UTZMMzDdKhmkZMECCzk57"
    }
  }
}
```

### MCP Inspector

You can test this server with the official MCP Inspector.

1. Start the worker locally:

```bash
yarn dev
```

2. In another terminal, start Inspector:

```bash
npx @modelcontextprotocol/inspector
```

3. Open the Inspector UI, usually at:

```text
http://127.0.0.1:6274
```

4. Use these connection settings:

- Transport: `Streamable HTTP`
- URL: `http://127.0.0.1:8787/mcp`

5. Connect, then use `tools/list` or call:

- `spl.deposit`
- `spl.withdraw`
- `spl.transfer`
- `spl.getBalance`
- `spl.getPrivateBalance`

## Request Conventions

- Public keys are base58 strings
- `cluster` is optional:
  - omit it or use `mainnet` to use `BASE_RPC_URL` and `EPHEMERAL_RPC_URL`
  - use `devnet` to use `BASE_DEVNET_RPC_URL` and `EPHEMERAL_DEVNET_RPC_URL`
  - use any other valid http(s) URL to use that URL for both base and ephemeral RPCs
- amount encoding depends on the route:
  - deposit, withdraw, and transfer: integer JSON values with minimum `1`, for example `1` or `1000000`
- do not send UI-decimal token strings like `"1.5"`
- blockhash is not part of the request

## Response Contract

Transaction routes return:

```json
{
  "kind": "transfer",
  "version": "legacy",
  "transactionBase64": "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA...",
  "sendTo": "ephemeral",
  "recentBlockhash": "9A4VhP8M8fQZxP4h7rB6mP6eM8w2pJkYh7QdZk7V4r2x",
  "lastValidBlockHeight": 123456,
  "instructionCount": 2,
  "requiredSigners": [
    "Bt9oNR5cCtnfuMmXgWELd6q5i974PdEMQDUE55nBC57L"
  ],
  "validator": "11111111111111111111111111111111"
}
```

The expected client flow is:

1. call the API
2. decode `transactionBase64`
3. optionally adjust the transaction if your client needs to
4. sign with the required wallet(s)
5. send to the RPC indicated by `sendTo`

## Endpoints

### `POST /v1/spl/deposit`

Builds an unsigned base-chain deposit transaction.

This wraps the SDK `delegateSpl(...)` flow.

Example:

```bash
curl -X POST http://127.0.0.1:8787/v1/spl/deposit \
  -H 'content-type: application/json' \
  -d '{
    "owner": "3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE",
    "amount": 1,
    "initIfMissing": true,
    "initAtasIfMissing": true,
    "initVaultIfMissing": true,
    "idempotent": true
  }'
```

Notes:

- if `cluster` is omitted, the API uses `mainnet`
- if `mint` is omitted, the API uses Solana USDC: `EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v`
- `amount` is an integer JSON value with minimum `1`, not a string
- if `validator` is omitted, the API uses `MAS1Dt9qreoRMQ14YQuhg8UTZMMzDdKhmkZMECCzk57` on mainnet; on devnet or a custom RPC URL it uses `VALIDATOR_PUBKEY` when set, otherwise the selected ephemeral RPC identity
- `shuttleId` is generated internally
- `escrowIndex` is fixed to `0` and is not part of the public request body

Relevant fields:

- `owner`
- `cluster`
- `mint`
- `amount`
- `validator`
- `initIfMissing`
- `initVaultIfMissing`
- `initAtasIfMissing`
- `idempotent`

### `POST /v1/spl/withdraw`

Withdraws SPL tokens from an ephemeral rollup back to Solana. `amount` is an integer JSON value with minimum `1`. If `cluster` is omitted, the API uses `mainnet`.

Example:

```bash
curl -X POST http://127.0.0.1:8787/v1/spl/withdraw \
  -H 'content-type: application/json' \
  -d '{
    "owner": "OWNER_PUBKEY",
    "mint": "MINT_PUBKEY",
    "amount": 1000000,
    "idempotent": true
  }'
```

Relevant fields:

- `owner`
- `cluster`
- `mint`
- `amount`
- `validator`
- `initIfMissing`
- `initAtasIfMissing`
- `shuttleId`
- `escrowIndex`
- `idempotent`

### `POST /v1/spl/transfer`

Builds an unsigned transfer transaction using `transferSpl(...)`. `amount` is an integer JSON value with minimum `1`. If `cluster` is omitted, the API uses `mainnet`.

The API automatically decides:

- which blockhash to use
- whether the client should send to the base RPC or ephemeral RPC

The returned `sendTo` value is:

- `"base"` when `fromBalance` is `"base"`
- `"ephemeral"` when `fromBalance` is `"ephemeral"`

Example:

```bash
curl -X POST http://127.0.0.1:8787/v1/spl/transfer \
  -H 'content-type: application/json' \
  -d '{
    "from": "FROM_OWNER_PUBKEY",
    "to": "TO_OWNER_PUBKEY",
    "mint": "MINT_PUBKEY",
    "amount": 1000000,
    "visibility": "private",
    "fromBalance": "base",
    "toBalance": "base",
    "initIfMissing": true,
    "initAtasIfMissing": true,
    "initVaultIfMissing": true,
    "minDelayMs": "0",
    "maxDelayMs": "0",
    "split": 1
  }'
```

Supported transfer combinations currently exposed by the SDK path used here:

- `base -> base` with `public`
- `base -> base` with `private`
- `base -> ephemeral` with `private`
- `ephemeral -> ephemeral` with `public`
- `ephemeral -> ephemeral` with `private`
- `ephemeral -> base` with `private`

Unsupported combinations return `400` with:

- `code: "UNSUPPORTED_TRANSFER_ROUTE"`

Private transfer validation:

- `minDelayMs` defaults to `"0"`
- `maxDelayMs` defaults to `"0"` and follows `minDelayMs` when only `minDelayMs` is set
- `split` defaults to `1`
- `minDelayMs` must be an integer string
- `maxDelayMs` must be an integer string
- `split` must be a positive integer
- `split` cannot exceed `amount`
- if both delays are present, `maxDelayMs >= minDelayMs`

Relevant fields:

- `from`
- `to`
- `cluster`
- `mint`
- `amount`
- `visibility`
- `fromBalance`
- `toBalance`
- `validator`
- `initIfMissing`
- `initAtasIfMissing`
- `initVaultIfMissing`
- `shuttleId`
- `minDelayMs`
- `maxDelayMs`
- `split`

### `GET /v1/spl/balance`

Returns the owner ATA balance from the base RPC. If `cluster` is omitted, the API uses `mainnet`.

Example:

```bash
curl "http://127.0.0.1:8787/v1/spl/balance?address=Bt9oNR5cCtnfuMmXgWELd6q5i974PdEMQDUE55nBC57L&mint=EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
```

Response shape:

```json
{
  "address": "Bt9oNR5cCtnfuMmXgWELd6q5i974PdEMQDUE55nBC57L",
  "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
  "ata": "3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE",
  "location": "base",
  "balance": "1000000"
}
```

### `GET /v1/spl/private-balance`

Returns the owner ATA balance from the ephemeral RPC. If `cluster` is omitted, the API uses `mainnet`.

Example:

```bash
curl "http://127.0.0.1:8787/v1/spl/private-balance?address=Bt9oNR5cCtnfuMmXgWELd6q5i974PdEMQDUE55nBC57L&mint=EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
```

Note:

- this currently reads the owner ATA on the ephemeral RPC
- it does not decode or return a delegation PDA balance directly

## Error Handling

The API uses JSON error responses. Typical cases:

- `400`: bad input, unsupported route combination, invalid key, invalid amount
- `404`: route not found
- `422`: schema validation error
- `500`: internal error
- `502`: upstream RPC error

Validation error example:

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Request validation failed",
    "issues": [
      {
        "code": "invalid_format",
        "message": "Must be a base-unit integer string",
        "path": ["amount"]
      }
    ]
  }
}
```

## Local Verification

Current local verification commands:

```bash
yarn build
yarn test
```

The included test suite verifies:

- OpenAPI document exposure
- deposit transaction generation
- ephemeral blockhash selection for ephemeral transfers
- unsupported transfer route handling
- base vs private balance routing

## Notes

- This API returns unsigned transactions only
- The client is responsible for signing and submitting them
- The API currently serializes legacy `Transaction`, not versioned transactions
- `transfer` prepends a noop instruction to preserve the same behavior as the current app flow
