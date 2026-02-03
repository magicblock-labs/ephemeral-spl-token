"""
Snapshot tests for transaction serialization determinism.

These tests ensure that API refactoring doesn't change transaction output.
The blockhash is mocked to a fixed value for deterministic comparison.

Usage:
    # Run tests
    pytest test_snapshots.py -v

    # Regenerate snapshots after intentional changes
    python generate_snapshots.py
"""

import json
import pytest
from pathlib import Path
from unittest.mock import patch, AsyncMock

# Fixed blockhash for deterministic tests (32 zero bytes)
FIXED_BLOCKHASH = b'\x00' * 32

# Load snapshots
SNAPSHOTS_FILE = Path(__file__).parent / "snapshots.json"


def load_snapshots() -> dict:
    """Load expected transaction snapshots from JSON file."""
    if not SNAPSHOTS_FILE.exists():
        return {}
    with open(SNAPSHOTS_FILE) as f:
        return json.load(f)


SNAPSHOTS = load_snapshots()


@pytest.fixture
def mock_blockhash():
    """Mock the blockhash to be deterministic."""
    with patch('src.builder._get_latest_blockhash', new_callable=AsyncMock) as mock:
        mock.return_value = FIXED_BLOCKHASH
        yield mock


@pytest.fixture
def client(mock_blockhash):
    """Create test client with mocked blockhash."""
    from fastapi.testclient import TestClient
    from src.main import create_app
    app = create_app()
    return TestClient(app)


# Test fixtures - these inputs should produce stable outputs
TEST_INPUTS = {
    "initialize_ephemeral_ata": {
        "endpoint": "/tx/initialize-ephemeral-ata",
        "payload": {
            "payer": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "user": "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
            "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "ephemeral_ata": "So11111111111111111111111111111111111111112",
            "ephemeral_ata_bump": 255,
        },
    },
    "initialize_global_vault": {
        "endpoint": "/tx/initialize-global-vault",
        "payload": {
            "payer": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "vault": "So11111111111111111111111111111111111111112",
            "vault_bump": 254,
        },
    },
    "deposit_spl_tokens": {
        "endpoint": "/tx/deposit-spl-tokens",
        "payload": {
            "authority": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "user": "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
            "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "source_token": "So11111111111111111111111111111111111111112",
            "vault_token": "11111111111111111111111111111111",
            "amount": 1000000,
            "ephemeral_ata": "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
            "vault": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        },
    },
    "withdraw_spl_tokens": {
        "endpoint": "/tx/withdraw-spl-tokens",
        "payload": {
            "owner": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "vault_source": "So11111111111111111111111111111111111111112",
            "user_dest": "11111111111111111111111111111111",
            "amount": 500000,
            "ephemeral_ata": "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
            "vault": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "vault_bump": 253,
        },
    },
    "delegate_ephemeral_ata": {
        "endpoint": "/tx/delegate-ephemeral-ata",
        "payload": {
            "payer": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "user": "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
            "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "owner_program": "So11111111111111111111111111111111111111112",
            "buffer": "11111111111111111111111111111111",
            "delegation_record": "ComputeBudget111111111111111111111111111111",
            "delegation_metadata": "Vote111111111111111111111111111111111111111",
            "ephemeral_ata": "Stake11111111111111111111111111111111111111",
            "ephemeral_ata_bump": 252,
        },
    },
    "delegate_ephemeral_ata_with_validator": {
        "endpoint": "/tx/delegate-ephemeral-ata",
        "payload": {
            "payer": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "user": "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
            "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "owner_program": "So11111111111111111111111111111111111111112",
            "buffer": "11111111111111111111111111111111",
            "delegation_record": "ComputeBudget111111111111111111111111111111",
            "delegation_metadata": "Vote111111111111111111111111111111111111111",
            "ephemeral_ata": "Stake11111111111111111111111111111111111111",
            "ephemeral_ata_bump": 252,
            "validator": "Config1111111111111111111111111111111111111",
        },
    },
    "undelegate_ephemeral_ata": {
        "endpoint": "/tx/undelegate-ephemeral-ata",
        "payload": {
            "payer": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "user": "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
            "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "ata": "So11111111111111111111111111111111111111112",
            "magic_context": "11111111111111111111111111111111",
            "ephemeral_ata": "ComputeBudget111111111111111111111111111111",
        },
    },
    "create_ephemeral_ata_permission": {
        "endpoint": "/tx/create-ephemeral-ata-permission",
        "payload": {
            "payer": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "user": "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
            "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "flags": 3,
            "ephemeral_ata": "So11111111111111111111111111111111111111112",
            "ephemeral_ata_bump": 251,
            "permission": "11111111111111111111111111111111",
        },
    },
    "delegate_ephemeral_ata_permission": {
        "endpoint": "/tx/delegate-ephemeral-ata-permission",
        "payload": {
            "payer": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "user": "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
            "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "buffer": "So11111111111111111111111111111111111111112",
            "record": "11111111111111111111111111111111",
            "metadata": "ComputeBudget111111111111111111111111111111",
            "validator": "Vote111111111111111111111111111111111111111",
            "ephemeral_ata": "Stake11111111111111111111111111111111111111",
            "ephemeral_ata_bump": 250,
            "permission": "Config1111111111111111111111111111111111111",
        },
    },
    "undelegate_ephemeral_ata_permission": {
        "endpoint": "/tx/undelegate-ephemeral-ata-permission",
        "payload": {
            "payer": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "user": "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
            "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "magic_context": "So11111111111111111111111111111111111111112",
            "ephemeral_ata": "11111111111111111111111111111111",
            "permission": "ComputeBudget111111111111111111111111111111",
        },
    },
    "reset_ephemeral_ata_permission": {
        "endpoint": "/tx/reset-ephemeral-ata-permission",
        "payload": {
            "owner": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "user": "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
            "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "flags": 7,
            "ephemeral_ata": "So11111111111111111111111111111111111111112",
            "ephemeral_ata_bump": 249,
            "permission": "11111111111111111111111111111111",
        },
    },
    "checked_transfer": {
        "endpoint": "/tx/checked-transfer",
        "payload": {
            "source": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "destination": "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
            "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "amount": 1000000,
            "decimals": 6,
            "authority": "So11111111111111111111111111111111111111112",
        },
    },
    "initialize_ata": {
        "endpoint": "/tx/initialize-ata",
        "payload": {
            "payer": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "user": "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
            "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "ata": "So11111111111111111111111111111111111111112",
        },
    },
}


class TestTransactionDeterminism:
    """Verify transaction serialization is deterministic."""

    @pytest.mark.parametrize("case_name", TEST_INPUTS.keys())
    def test_same_input_same_output(self, client, case_name):
        """Same inputs should produce identical transactions."""
        case = TEST_INPUTS[case_name]
        endpoint = case["endpoint"]
        payload = case["payload"]

        tx1 = client.post(endpoint, json=payload).json()["transaction"]
        tx2 = client.post(endpoint, json=payload).json()["transaction"]

        assert tx1 == tx2, f"Transaction for {case_name} is not deterministic"


class TestTransactionSnapshots:
    """Snapshot tests for refactoring safety."""

    @pytest.mark.parametrize("case_name", TEST_INPUTS.keys())
    def test_transaction_matches_snapshot(self, client, case_name):
        """Transaction output must match recorded snapshot."""
        if case_name not in SNAPSHOTS:
            pytest.skip(f"No snapshot for {case_name} - run generate_snapshots.py")

        case = TEST_INPUTS[case_name]
        endpoint = case["endpoint"]
        payload = case["payload"]

        response = client.post(endpoint, json=payload)
        assert response.status_code == 200, f"Request failed: {response.json()}"

        actual_tx = response.json()["transaction"]
        expected_tx = SNAPSHOTS[case_name]

        if actual_tx != expected_tx:
            # Provide helpful diff info
            import base64
            actual_bytes = base64.b64decode(actual_tx)
            expected_bytes = base64.b64decode(expected_tx)

            pytest.fail(
                f"Transaction mismatch for {case_name}!\n"
                f"Expected length: {len(expected_bytes)} bytes\n"
                f"Actual length:   {len(actual_bytes)} bytes\n"
                f"Expected (first 100): {expected_tx[:100]}...\n"
                f"Actual (first 100):   {actual_tx[:100]}...\n"
                f"\nRun 'python generate_snapshots.py' to update if change is intentional."
            )


class TestTransactionStructure:
    """Validate transaction structure without comparing exact bytes."""

    @pytest.mark.parametrize("case_name", TEST_INPUTS.keys())
    def test_transaction_is_valid_base64(self, client, case_name):
        """Transaction should be valid base64."""
        import base64

        case = TEST_INPUTS[case_name]
        response = client.post(case["endpoint"], json=case["payload"])
        tx = response.json()["transaction"]

        try:
            decoded = base64.b64decode(tx)
            assert len(decoded) > 0
        except Exception as e:
            pytest.fail(f"Invalid base64 for {case_name}: {e}")

    @pytest.mark.parametrize("case_name", TEST_INPUTS.keys())
    def test_transaction_has_signature_placeholder(self, client, case_name):
        """Transaction should have signature placeholder (64 zero bytes per signer)."""
        import base64

        case = TEST_INPUTS[case_name]
        response = client.post(case["endpoint"], json=case["payload"])
        tx = response.json()["transaction"]
        decoded = base64.b64decode(tx)

        # First byte is number of signatures (compact-u16, usually 1 byte for small values)
        num_sigs = decoded[0]
        assert num_sigs >= 1, f"Expected at least 1 signature, got {num_sigs}"

        # Signatures should be zero-filled placeholders
        sig_start = 1
        sig_end = sig_start + (num_sigs * 64)
        signatures = decoded[sig_start:sig_end]

        assert signatures == b'\x00' * (num_sigs * 64), (
            f"Signatures should be zero-filled for unsigned tx"
        )
