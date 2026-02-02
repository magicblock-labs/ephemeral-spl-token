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

### Health & Configuration
| Endpoint | Description |
|----------|-------------|
| `GET /` | Health check - returns status and program ID |
| `GET /config` | Get current configuration (cluster URL, program IDs) |

### Ephemeral ATA Operations
| Endpoint | Description |
|----------|-------------|
| `POST /tx/initialize-ephemeral-ata` | Initialize an Ephemeral ATA |
| `POST /tx/delegate-ephemeral-ata` | Delegate ephemeral ATA to DLP |
| `POST /tx/undelegate-ephemeral-ata` | Undelegate ephemeral ATA |

### Global Vault Operations
| Endpoint | Description |
|----------|-------------|
| `POST /tx/initialize-global-vault` | Initialize a Global Vault |
| `POST /tx/deposit-spl-tokens` | Deposit tokens into ephemeral ATA |
| `POST /tx/withdraw-spl-tokens` | Withdraw tokens from ephemeral ATA |

### Permission Management
| Endpoint | Description |
|----------|-------------|
| `POST /tx/create-ephemeral-ata-permission` | Create permission account |
| `POST /tx/delegate-ephemeral-ata-permission` | Delegate permission to DLP |
| `POST /tx/undelegate-ephemeral-ata-permission` | Undelegate permission |
| `POST /tx/reset-ephemeral-ata-permission` | Reset permission flags |

### Token Operations
| Endpoint | Description |
|----------|-------------|
| `POST /tx/checked-transfer` | Transfer SPL tokens with checked mint and decimals |

### Associated Token Account (ATA)
| Endpoint | Description |
|----------|-------------|
| `POST /tx/initialize-ata` | Create an Associated Token Account for a user-mint pair |

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

## Testing

Two test scripts are provided to verify all API endpoints:

### Quick Test Script
Run all endpoint tests with a simple Python script:

```bash
python test_endpoints.py
```

This provides a summary of all tests with pass/fail status.

### Comprehensive Pytest Tests
Run detailed tests with pytest:

```bash
# Install pytest
pip install pytest

# Run all tests
pytest test_api.py -v

# Run specific test class
pytest test_api.py::TestHealthEndpoints -v

# Run with quiet output
pytest test_api.py -q
```

Both test scripts cover:
- Health check endpoints
- All ephemeral ATA operations
- Global vault operations
- Token deposit/withdrawal
- Permission management
- Token transfers
- ATA creation
- Error handling and validation
- Transaction format verification

For detailed testing information, see [TESTING.md](TESTING.md).

## Cloudflare Workers Deployment

```bash
# Python Workers with third-party packages currently require pywrangler
uv run pywrangler dev
uv run pywrangler deploy
```
