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

### Setup

```bash
# Create virtual environment and install dependencies
cd api
python3 -m venv .venv
source .venv/bin/activate
pip install pytest httpx fastapi pydantic pydantic-settings
```

### Snapshot Tests (Transaction Determinism)

Snapshot tests ensure that API changes don't alter the serialized transaction output. This is useful when refactoring the API (e.g., moving PDA computation server-side) while ensuring byte-for-byte identical transactions.

```bash
# Run snapshot tests
source .venv/bin/activate
pytest tests/test_snapshots.py -v
```

**How it works:**
1. The blockhash is mocked to a fixed value for deterministic output
2. Tests compare the exact base64 transaction string against saved snapshots
3. If the output changes, the test fails with a diff

**Workflow for refactoring:**

```bash
# 1. Update master snapshot BEFORE making changes
python tests/update_master_snapshot.py

# 2. Make your API changes (simplify interface, move computation server-side, etc.)

# 3. Run tests to verify output is unchanged
pytest tests/test_snapshots.py -v

# 4. If tests fail, either fix your code or update the master snapshot if change is intentional
python tests/update_master_snapshot.py
```

### API Endpoint Tests

Run endpoint tests to verify all API functionality:

```bash
# Run all endpoint tests
pytest tests/test_api.py -v

# Run specific test class
pytest tests/test_api.py::TestHealthEndpoints -v
```

### Test Coverage

- **Snapshot tests** (`test_snapshots.py`): Transaction determinism, byte-for-byte comparison
- **API tests** (`test_api.py`): Endpoint functionality, validation, error handling

## Cloudflare Workers Deployment

```bash
# Python Workers with third-party packages currently require pywrangler
uv run pywrangler dev
uv run pywrangler deploy
```
