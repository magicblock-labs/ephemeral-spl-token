# Ephemeral SPL Token API

FastAPI service for building Solana transactions for the Ephemeral SPL Token program.

## Quick Start

```bash
# Install dependencies
uv sync

# Run locally (app factory)
uv run uvicorn src.main:create_app --factory --reload

# Or with custom port
uv run uvicorn src.main:create_app --factory --reload --port 8080
```

## Configuration

Environment variables (prefix: `EPHEMERAL_`):

| Variable | Default | Description |
|----------|---------|-------------|
| `EPHEMERAL_CLUSTER_URL` | `https://rpc.magicblock.app/mainnet` | Solana cluster URL |

## API Documentation

Once running, visit:
- Swagger UI: http://localhost:8000/docs
- ReDoc: http://localhost:8000/redoc
- OpenAPI JSON: http://localhost:8000/openapi.json

## Endpoints

All transaction endpoints return a base64-encoded serialized transaction ready to be signed.

Note: Cloudflare Python Workers cannot use native Solana packages yet, so PDA addresses
and bump seeds must be provided by the client.

| Endpoint | Description |
|----------|-------------|
| `POST /tx/initialize-ephemeral-ata` | Initialize an Ephemeral ATA |
| `POST /tx/initialize-global-vault` | Initialize a Global Vault |
| `POST /tx/deposit-spl-tokens` | Deposit tokens into ephemeral ATA |
| `POST /tx/withdraw-spl-tokens` | Withdraw tokens from ephemeral ATA |
| `POST /tx/delegate-ephemeral-ata` | Delegate ephemeral ATA to DLP |
| `POST /tx/undelegate-ephemeral-ata` | Undelegate ephemeral ATA |
| `POST /tx/create-ephemeral-ata-permission` | Create permission account |
| `POST /tx/delegate-ephemeral-ata-permission` | Delegate permission to DLP |
| `POST /tx/undelegate-ephemeral-ata-permission` | Undelegate permission |
| `POST /tx/reset-ephemeral-ata-permission` | Reset permission flags |

## Example Usage

```bash
curl -X POST http://localhost:8000/tx/initialize-ephemeral-ata \
  -H "Content-Type: application/json" \
  -d '{
    "payer": "YourPayerPubkey...",
    "user": "UserPubkey...",
    "mint": "MintPubkey..."
  }'
```

Response:
```json
{
  "transaction": "base64EncodedTransaction...",
  "message": "Transaction created successfully"
}
```

## Cloudflare Workers Deployment

```bash
# Python Workers with third-party packages currently require pywrangler
uv run pywrangler dev
uv run pywrangler deploy
```
