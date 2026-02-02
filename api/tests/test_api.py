#!/usr/bin/env python3
"""
Pytest-based tests for Ephemeral SPL Token API.
Run with: pytest test_api.py -v
"""

import pytest
import httpx
import json
from typing import Dict, Any


BASE_URL = "http://localhost:8000"

# Sample pubkeys (valid Solana addresses)
PAYER = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
USER = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
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
        assert "program_id" in data
        assert "cluster_url" in data

    def test_config_endpoint(self, client):
        """Test GET /config endpoint."""
        response = client.get("/config")
        assert response.status_code == 200
        data = response.json()
        assert "cluster_url" in data
        assert "program_id" in data
        assert "token_program" in data


class TestEphemeralAtaEndpoints:
    """Test ephemeral ATA endpoints."""

    def test_initialize_ephemeral_ata(self, client):
        """Test POST /tx/initialize-ephemeral-ata."""
        response = client.post("/tx/initialize-ephemeral-ata", json={
            "payer": PAYER,
            "user": USER,
            "mint": MINT,
            "ephemeral_ata": ATA,
            "ephemeral_ata_bump": 255,
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
            "buffer": BUFFER,
            "delegation_record": DELEGATION_RECORD,
            "delegation_metadata": DELEGATION_METADATA,
            "ephemeral_ata": ATA,
            "ephemeral_ata_bump": 255,
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
            "ata": ATA,
            "magic_context": MAGIC_CONTEXT,
            "ephemeral_ata": ATA,
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
            "vault": VAULT,
            "vault_bump": 255,
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
            "authority": AUTHORITY,
            "user": USER,
            "mint": MINT,
            "source_token": SOURCE,
            "vault_token": VAULT_TOKEN,
            "amount": 1000000,
            "ephemeral_ata": ATA,
            "vault": VAULT,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data

    def test_withdraw_spl_tokens(self, client):
        """Test POST /tx/withdraw-spl-tokens."""
        response = client.post("/tx/withdraw-spl-tokens", json={
            "owner": USER,
            "mint": MINT,
            "vault_source": VAULT_SOURCE,
            "user_dest": DEST,
            "amount": 500000,
            "ephemeral_ata": ATA,
            "vault": VAULT,
            "vault_bump": 255,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data


class TestPermissionEndpoints:
    """Test permission endpoints."""

    def test_create_ephemeral_ata_permission(self, client):
        """Test POST /tx/create-ephemeral-ata-permission."""
        response = client.post("/tx/create-ephemeral-ata-permission", json={
            "payer": PAYER,
            "user": USER,
            "mint": MINT,
            "flags": 1,
            "ephemeral_ata": ATA,
            "ephemeral_ata_bump": 255,
            "permission": PERMISSION,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data

    def test_delegate_ephemeral_ata_permission(self, client):
        """Test POST /tx/delegate-ephemeral-ata-permission."""
        response = client.post("/tx/delegate-ephemeral-ata-permission", json={
            "payer": PAYER,
            "user": USER,
            "mint": MINT,
            "buffer": BUFFER,
            "record": DELEGATION_RECORD,
            "metadata": DELEGATION_METADATA,
            "validator": PAYER,
            "ephemeral_ata": ATA,
            "ephemeral_ata_bump": 255,
            "permission": PERMISSION,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data

    def test_undelegate_ephemeral_ata_permission(self, client):
        """Test POST /tx/undelegate-ephemeral-ata-permission."""
        response = client.post("/tx/undelegate-ephemeral-ata-permission", json={
            "payer": PAYER,
            "user": USER,
            "mint": MINT,
            "magic_context": MAGIC_CONTEXT,
            "ephemeral_ata": ATA,
            "permission": PERMISSION,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data

    def test_reset_ephemeral_ata_permission(self, client):
        """Test POST /tx/reset-ephemeral-ata-permission."""
        response = client.post("/tx/reset-ephemeral-ata-permission", json={
            "owner": USER,
            "user": USER,
            "mint": MINT,
            "flags": 0,
            "ephemeral_ata": ATA,
            "ephemeral_ata_bump": 255,
            "permission": PERMISSION,
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data


class TestTokenEndpoints:
    """Test token transfer endpoints."""

    def test_checked_transfer(self, client):
        """Test POST /tx/checked-transfer."""
        response = client.post("/tx/checked-transfer", json={
            "source": SOURCE,
            "destination": DEST,
            "mint": MINT,
            "amount": 1000000,
            "decimals": 6,
            "authority": AUTHORITY,
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
            "user": USER,
            "mint": MINT,
            "ata": ATA,
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
            "user": USER,
            "mint": MINT,
            "ata": ATA,
            "token_program": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        })
        assert response.status_code == 200
        data = response.json()
        assert "transaction" in data


class TestErrorHandling:
    """Test error handling."""

    def test_missing_required_field(self, client):
        """Test request with missing required field."""
        response = client.post("/tx/initialize-ephemeral-ata", json={
            "payer": PAYER,
            "user": USER,
            # Missing "mint" and other required fields
        })
        assert response.status_code == 422  # Validation error

    def test_invalid_pubkey_format(self, client):
        """Test request with invalid pubkey format."""
        response = client.post("/tx/initialize-ephemeral-ata", json={
            "payer": "not-a-valid-pubkey!!!",
            "user": USER,
            "mint": MINT,
            "ephemeral_ata": ATA,
            "ephemeral_ata_bump": 255,
        })
        assert response.status_code == 400  # Bad request

    def test_invalid_bump_seed(self, client):
        """Test request with invalid bump seed."""
        response = client.post("/tx/initialize-ephemeral-ata", json={
            "payer": PAYER,
            "user": USER,
            "mint": MINT,
            "ephemeral_ata": ATA,
            "ephemeral_ata_bump": 256,  # Invalid: > 255
        })
        assert response.status_code == 422  # Validation error


class TestTransactionFormat:
    """Test transaction response format."""

    def test_transaction_is_base64(self, client):
        """Test that transaction response is valid base64."""
        import base64
        response = client.post("/tx/initialize-ata", json={
            "payer": PAYER,
            "user": USER,
            "mint": MINT,
            "ata": ATA,
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
            "user": USER,
            "mint": MINT,
            "ata": ATA,
        })
        data = response.json()
        
        assert "transaction" in data
        assert "message" in data
        assert data["message"] == "Transaction created successfully"


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
