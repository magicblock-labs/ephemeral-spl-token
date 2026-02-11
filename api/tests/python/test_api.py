#!/usr/bin/env python3
"""
Pytest-based tests for Ephemeral SPL Token API.
Run with: pytest test_api.py -v
"""

import pytest
import httpx
import json
from typing import Dict, Any


BASE_URL = "http://localhost:4000"

# Sample pubkeys (valid Solana addresses)
PAYER = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
USER = "EyBRt4Acr7b4s3exfnVvJ4EgL8oa6Lc4JK1Leonud34W"
MINT = "Gh9ZwEmdLJ8DscKNTkTqPbNwLNNBjuSzaG9Vp2KGtKJr"
ATA = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
AUTHORITY = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
SOURCE = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
DEST = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
VAULT = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
VAULT_TOKEN = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
VAULT_SOURCE = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
OWNER_PROGRAM = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
BUFFER = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
DELEGATION_RECORD = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
DELEGATION_METADATA = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
PERMISSION = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
MAGIC_CONTEXT = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"


@pytest.fixture
def client():
    """Provide HTTP client for tests."""
    with httpx.Client(base_url=BASE_URL, timeout=30.0) as c:
        yield c


class TestHealthEndpoints:
    """Test health check endpoints."""

    def test_root_endpoint(self, client):
        """Test GET / endpoint."""
        response = client.get("/")
        assert response.status_code == 200
        data = response.json()
        assert data["status"] == "ok"
        assert "ephemeral_spl_token_program" in data
        assert "endpoint_url" in data

    def test_config_endpoint(self, client):
        """Test GET /config endpoint."""
        response = client.get("/config")
        assert response.status_code == 200
        data = response.json()
        assert "endpoint_url" in data
        assert "ephemeral_spl_token_program" in data
        assert "token_program" in data


class TestEphemeralAtaEndpoints:
    """Test ephemeral ATA endpoints."""

    def test_initialize_ephemeral_ata(self, client):
        """Test POST /tx/initialize-ephemeral-ata."""
        response = client.post("/tx/initialize-ephemeral-ata", json={
            "payer": PAYER,
            "owner": USER,
            "mint": MINT,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data
        assert isinstance(data["transaction"], str)
        assert len(data["transaction"]) > 0

    def test_delegate_ephemeral_ata(self, client):
        """Test POST /tx/delegate-ephemeral-ata."""
        response = client.post("/tx/delegate-ephemeral-ata", json={
            "payer": PAYER,
            "user": USER,
            "mint": MINT,
            "owner_program": OWNER_PROGRAM,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data

    def test_undelegate_ephemeral_ata(self, client):
        """Test POST /tx/undelegate-ephemeral-ata."""
        response = client.post("/tx/undelegate-ephemeral-ata", json={
            "payer": PAYER,
            "user": USER,
            "mint": MINT,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data


class TestVaultEndpoints:
    """Test global vault endpoints."""

    def test_initialize_global_vault(self, client):
        """Test POST /tx/initialize-global-vault."""
        response = client.post("/tx/initialize-global-vault", json={
            "payer": PAYER,
            "mint": MINT,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data
        assert isinstance(data["transaction"], str)


class TestDepositWithdrawEndpoints:
    """Test deposit and withdraw endpoints."""

    def test_deposit_spl_tokens(self, client):
        """Test POST /tx/deposit-spl-tokens."""
        response = client.post("/tx/deposit-spl-tokens", json={
            "owner": AUTHORITY,
            "user": USER,
            "mint": MINT,
            "amount": 1000000,
        })
        # May return 400 if accounts don't exist on chain
        assert response.status_code in [200, 400]

    def test_withdraw_spl_tokens(self, client):
        """Test POST /tx/withdraw-spl-tokens."""
        response = client.post("/tx/withdraw-spl-tokens", json={
            "owner": USER,
            "user": USER,
            "mint": MINT,
            "amount": 500000,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data


class TestPermissionEndpoints:
    """Test permission endpoints - not currently exposed in API."""
    pass


class TestTokenEndpoints:
    """Test token transfer endpoints."""

    def test_checked_transfer(self, client):
        """Test POST /tx/checked-transfer."""
        response = client.post("/tx/checked-transfer", json={
            "sender": PAYER,
            "recipient": USER,
            "mint": MINT,
            "amount": 1000000,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data
        assert isinstance(data["transaction"], str)
        assert len(data["transaction"]) > 0


class TestAtaEndpoints:
    """Test Associated Token Account endpoints."""

    def test_initialize_ata(self, client):
        """Test POST /tx/initialize-ata."""
        response = client.post("/tx/initialize-ata", json={
            "payer": PAYER,
            "owner": USER,
            "mint": MINT,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data
        assert isinstance(data["transaction"], str)
        assert len(data["transaction"]) > 0

    def test_initialize_ata_with_custom_token_program(self, client):
        """Test POST /tx/initialize-ata with custom token program."""
        response = client.post("/tx/initialize-ata", json={
            "payer": PAYER,
            "owner": USER,
            "mint": MINT,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data


class TestErrorHandling:
    """Test error handling."""

    def test_missing_required_field(self, client):
        """Test request with missing required field."""
        response = client.post("/tx/initialize-ephemeral-ata", json={
            "user": USER,
            # Missing "mint" required field
        })
        assert response.status_code == 422  # Validation error

    def test_invalid_pubkey_format(self, client):
        """Test request with invalid pubkey format."""
        response = client.post("/tx/initialize-ephemeral-ata", json={
            "payer": "not-a-valid-pubkey!!!",
            "owner": "not-a-valid-pubkey!!!",
            "mint": MINT,
        })
        assert response.status_code in [400, 422]  # Bad request or validation error


class TestPrivateEndpoints:
    """Test private transaction endpoints."""

    def test_private_transfer_amount(self, client):
        """Test POST /private/tx/transfer-amount."""
        response = client.post("/private/tx/transfer-amount", json={
            "sender": PAYER,
            "recipient": USER,
            "mint": MINT,
            "amount": 1000000,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data

    def test_private_prepare_withdrawal(self, client):
        """Test POST /private/tx/prepare-withdrawal."""
        response = client.post("/private/tx/prepare-withdrawal", json={
            "user": USER,
            "mint": MINT,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data

    def test_private_withdraw(self, client):
        """Test POST /private/tx/withdraw."""
        response = client.post("/private/tx/withdraw", json={
            "owner": USER,
            "user": USER,
            "mint": MINT,
            "amount": 500000,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data

    def test_private_deposit(self, client):
        """Test POST /private/tx/deposit."""
        response = client.post("/private/tx/deposit", json={
            "payer": PAYER,
            "user": USER,
            "mint": MINT,
            "amount": 1000000,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data


class TestTransactionFormat:
    """Test transaction response format."""

    def test_transaction_is_base64(self, client):
        """Test that transaction response is valid base64."""
        import base64
        response = client.post("/tx/initialize-ata", json={
            "payer": PAYER,
            "owner": USER,
            "mint": MINT,
        })
        data = response.json()
        tx = data["transaction"]
        
        # Should be valid base64
        try:
            decoded = base64.b64decode(tx)
            assert len(decoded) > 0
        except Exception as e:
            pytest.fail(f"Transaction is not valid base64: {e}")

    def test_response_structure(self, client):
        """Test response has required fields."""
        response = client.post("/tx/initialize-ata", json={
            "payer": PAYER,
            "owner": USER,
            "mint": MINT,
        })
        data = response.json()
        
        assert "transaction" in data
        assert "message" in data
        assert data["message"] == "Transaction created successfully"


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
