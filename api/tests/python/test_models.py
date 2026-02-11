#!/usr/bin/env python3
"""
Unit tests for Pydantic models and request/response schemas.
Tests model validation, serialization, and field constraints.
"""

import pytest

from pydantic import ValidationError
from src.models import (
    TransactionResponse,
    ClusterConfig,
    InitializeEphemeralAtaRequest,
    InitializeGlobalVaultRequest,
    DepositSplTokensRequest,
    WithdrawRequest,
    DelegateEphemeralAtaRequest,
    UndelegateEphemeralAtaRequest,
    CreateEphemeralAtaPermissionRequest,
    DelegateEphemeralAtaPermissionRequest,
    UndelegateEphemeralAtaPermissionRequest,
    ResetEphemeralAtaPermissionRequest,
    TransferAmountRequest,
    InitializeAtaRequest,
    DepositRequest,
    PrepareWithdrawalRequest,
)


# Test fixtures
VALID_PUBKEY = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
VALID_PUBKEY_2 = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
VALID_PUBKEY_3 = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
VALID_TX = "base64encodedtransactiondata=="
VALID_ENDPOINT = "https://rpc.example.com"


class TestTransactionResponse:
    """Test TransactionResponse model."""

    def test_transaction_response_creation(self):
        """Test creating TransactionResponse."""
        response = TransactionResponse(transaction=VALID_TX)
        assert response.transaction == VALID_TX
        assert response.message == "Transaction created successfully"

    def test_transaction_response_custom_message(self):
        """Test TransactionResponse with custom message."""
        response = TransactionResponse(
            transaction=VALID_TX,
            message="Custom message"
        )
        assert response.message == "Custom message"

    def test_transaction_response_required_fields(self):
        """Test that transaction field is required."""
        with pytest.raises(ValidationError):
            TransactionResponse()

    def test_transaction_response_serialization(self):
        """Test TransactionResponse serialization."""
        response = TransactionResponse(transaction=VALID_TX)
        data = response.model_dump()
        assert data["transaction"] == VALID_TX
        assert data["message"] == "Transaction created successfully"

    def test_transaction_response_json_schema(self):
        """Test TransactionResponse has proper JSON schema."""
        schema = TransactionResponse.model_json_schema()
        assert "properties" in schema
        assert "transaction" in schema["properties"]
        assert "message" in schema["properties"]


class TestClusterConfig:
    """Test ClusterConfig model."""

    def test_cluster_config_optional_endpoint(self):
        """Test ClusterConfig with optional endpoint."""
        config = ClusterConfig()
        assert config.endpoint_url is None

    def test_cluster_config_with_endpoint(self):
        """Test ClusterConfig with endpoint."""
        config = ClusterConfig(endpoint_url=VALID_ENDPOINT)
        assert config.endpoint_url == VALID_ENDPOINT

    def test_cluster_config_serialization(self):
        """Test ClusterConfig serialization."""
        config = ClusterConfig(endpoint_url=VALID_ENDPOINT)
        data = config.model_dump()
        assert data["endpoint_url"] == VALID_ENDPOINT


class TestInitializeEphemeralAtaRequest:
    """Test InitializeEphemeralAtaRequest model."""

    def test_initialize_ephemeral_ata_required_fields(self):
        """Test required fields for InitializeEphemeralAtaRequest."""
        req = InitializeEphemeralAtaRequest(
            payer=VALID_PUBKEY,
            owner=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
        )
        assert req.payer == VALID_PUBKEY
        assert req.owner == VALID_PUBKEY_2
        assert req.mint == VALID_PUBKEY_3

    def test_initialize_ephemeral_ata_optional_endpoint(self):
        """Test optional endpoint in request."""
        req = InitializeEphemeralAtaRequest(
            payer=VALID_PUBKEY,
            owner=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            endpoint_url=VALID_ENDPOINT,
        )
        assert req.endpoint_url == VALID_ENDPOINT

    def test_initialize_ephemeral_ata_missing_required(self):
        """Test validation with missing required field."""
        with pytest.raises(ValidationError):
            InitializeEphemeralAtaRequest(
                payer=VALID_PUBKEY,
                owner=VALID_PUBKEY_2,
                # mint is required
            )

    def test_initialize_ephemeral_ata_serialization(self):
        """Test serialization of request."""
        req = InitializeEphemeralAtaRequest(
            payer=VALID_PUBKEY,
            owner=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
        )
        data = req.model_dump()
        assert data["payer"] == VALID_PUBKEY


class TestInitializeGlobalVaultRequest:
    """Test InitializeGlobalVaultRequest model."""

    def test_initialize_global_vault_required_fields(self):
        """Test required fields."""
        req = InitializeGlobalVaultRequest(
            payer=VALID_PUBKEY,
            mint=VALID_PUBKEY_2,
        )
        assert req.payer == VALID_PUBKEY
        assert req.mint == VALID_PUBKEY_2

    def test_initialize_global_vault_missing_payer(self):
        """Test validation with missing payer."""
        with pytest.raises(ValidationError):
            InitializeGlobalVaultRequest(mint=VALID_PUBKEY_2)

    def test_initialize_global_vault_missing_mint(self):
        """Test validation with missing mint."""
        with pytest.raises(ValidationError):
            InitializeGlobalVaultRequest(payer=VALID_PUBKEY)


class TestDepositSplTokensRequest:
    """Test DepositSplTokensRequest model."""

    def test_deposit_required_fields(self):
        """Test required fields."""
        req = DepositSplTokensRequest(
            owner=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            amount=1000000,
        )
        assert req.owner == VALID_PUBKEY
        assert req.amount == 1000000

    def test_deposit_zero_amount(self):
        """Test deposit with zero amount."""
        req = DepositSplTokensRequest(
            owner=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            amount=0,
        )
        assert req.amount == 0

    def test_deposit_negative_amount_invalid(self):
        """Test that negative amount is rejected."""
        with pytest.raises(ValidationError):
            DepositSplTokensRequest(
                owner=VALID_PUBKEY,
                user=VALID_PUBKEY_2,
                mint=VALID_PUBKEY_3,
                amount=-1,
            )

    def test_deposit_large_amount(self):
        """Test deposit with large amount."""
        large_amount = 9223372036854775807  # max int64
        req = DepositSplTokensRequest(
            owner=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            amount=large_amount,
        )
        assert req.amount == large_amount


class TestWithdrawRequest:
    """Test WithdrawRequest model."""

    def test_withdraw_required_fields(self):
        """Test required fields."""
        req = WithdrawRequest(
            owner=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            amount=500000,
        )
        assert req.owner == VALID_PUBKEY
        assert req.amount == 500000

    def test_withdraw_zero_amount(self):
        """Test withdraw with zero amount."""
        req = WithdrawRequest(
            owner=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            amount=0,
        )
        assert req.amount == 0

    def test_withdraw_negative_amount_invalid(self):
        """Test that negative amount is rejected."""
        with pytest.raises(ValidationError):
            WithdrawRequest(
                owner=VALID_PUBKEY,
                user=VALID_PUBKEY_2,
                mint=VALID_PUBKEY_3,
                amount=-1,
            )


class TestDelegateEphemeralAtaRequest:
    """Test DelegateEphemeralAtaRequest model."""

    def test_delegate_required_fields(self):
        """Test required fields."""
        req = DelegateEphemeralAtaRequest(
            payer=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            owner_program=VALID_PUBKEY,
        )
        assert req.payer == VALID_PUBKEY
        assert req.owner_program == VALID_PUBKEY

    def test_delegate_optional_validator(self):
        """Test optional validator field."""
        req = DelegateEphemeralAtaRequest(
            payer=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            owner_program=VALID_PUBKEY,
            validator=VALID_PUBKEY_2,
        )
        assert req.validator == VALID_PUBKEY_2

    def test_delegate_validator_none_by_default(self):
        """Test validator is None by default."""
        req = DelegateEphemeralAtaRequest(
            payer=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            owner_program=VALID_PUBKEY,
        )
        assert req.validator is None


class TestCreateEphemeralAtaPermissionRequest:
    """Test CreateEphemeralAtaPermissionRequest model."""

    def test_create_permission_required_fields(self):
        """Test required fields."""
        req = CreateEphemeralAtaPermissionRequest(
            payer=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            flags=0,
        )
        assert req.payer == VALID_PUBKEY
        assert req.flags == 0

    def test_create_permission_flags_constraints(self):
        """Test flags field constraints."""
        # Valid flags (0-255)
        req1 = CreateEphemeralAtaPermissionRequest(
            payer=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            flags=255,
        )
        assert req1.flags == 255

    def test_create_permission_flags_too_high(self):
        """Test that flags > 255 is rejected."""
        with pytest.raises(ValidationError):
            CreateEphemeralAtaPermissionRequest(
                payer=VALID_PUBKEY,
                user=VALID_PUBKEY_2,
                mint=VALID_PUBKEY_3,
                flags=256,
            )

    def test_create_permission_flags_negative(self):
        """Test that negative flags are rejected."""
        with pytest.raises(ValidationError):
            CreateEphemeralAtaPermissionRequest(
                payer=VALID_PUBKEY,
                user=VALID_PUBKEY_2,
                mint=VALID_PUBKEY_3,
                flags=-1,
            )


class TestResetEphemeralAtaPermissionRequest:
    """Test ResetEphemeralAtaPermissionRequest model."""

    def test_reset_permission_required_fields(self):
        """Test required fields."""
        req = ResetEphemeralAtaPermissionRequest(
            owner=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            flags=0,
        )
        assert req.owner == VALID_PUBKEY
        assert req.flags == 0

    def test_reset_permission_flags_valid(self):
        """Test valid flags values."""
        for flags in [0, 1, 127, 255]:
            req = ResetEphemeralAtaPermissionRequest(
                owner=VALID_PUBKEY,
                user=VALID_PUBKEY_2,
                mint=VALID_PUBKEY_3,
                flags=flags,
            )
            assert req.flags == flags


class TestTransferAmountRequest:
    """Test TransferAmountRequest model."""

    def test_transfer_required_fields(self):
        """Test required fields."""
        req = TransferAmountRequest(
            sender=VALID_PUBKEY,
            recipient=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            amount=1000000,
        )
        assert req.sender == VALID_PUBKEY
        assert req.amount == 1000000

    def test_transfer_zero_amount(self):
        """Test transfer with zero amount."""
        req = TransferAmountRequest(
            sender=VALID_PUBKEY,
            recipient=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            amount=0,
        )
        assert req.amount == 0

    def test_transfer_negative_amount_invalid(self):
        """Test that negative amount is rejected."""
        with pytest.raises(ValidationError):
            TransferAmountRequest(
                sender=VALID_PUBKEY,
                recipient=VALID_PUBKEY_2,
                mint=VALID_PUBKEY_3,
                amount=-1,
            )


class TestInitializeAtaRequest:
    """Test InitializeAtaRequest model."""

    def test_initialize_ata_required_fields(self):
        """Test required fields."""
        req = InitializeAtaRequest(
            payer=VALID_PUBKEY,
            owner=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
        )
        assert req.payer == VALID_PUBKEY
        assert req.owner == VALID_PUBKEY_2
        assert req.mint == VALID_PUBKEY_3

    def test_initialize_ata_serialization(self):
        """Test serialization."""
        req = InitializeAtaRequest(
            payer=VALID_PUBKEY,
            owner=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
        )
        data = req.model_dump()
        assert data["payer"] == VALID_PUBKEY


class TestDepositRequest:
    """Test DepositRequest model."""

    def test_deposit_request_required_fields(self):
        """Test required fields."""
        req = DepositRequest(
            payer=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
        )
        assert req.payer == VALID_PUBKEY
        assert req.user == VALID_PUBKEY_2
        assert req.mint == VALID_PUBKEY_3
        assert req.amount == 0  # Default

    def test_deposit_request_with_amount(self):
        """Test DepositRequest with amount."""
        req = DepositRequest(
            payer=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            amount=1000000,
        )
        assert req.payer == VALID_PUBKEY
        assert req.amount == 1000000

    def test_deposit_request_default_amount(self):
        """Test that amount defaults to 0."""
        req = DepositRequest(
            payer=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
        )
        assert req.amount == 0

    def test_deposit_request_negative_amount_invalid(self):
        """Test that negative amount is rejected."""
        with pytest.raises(ValidationError):
            DepositRequest(
                payer=VALID_PUBKEY,
                user=VALID_PUBKEY_2,
                mint=VALID_PUBKEY_3,
                amount=-1,
            )

    def test_deposit_request_missing_payer(self):
        """Test validation with missing payer."""
        with pytest.raises(ValidationError):
            DepositRequest(
                user=VALID_PUBKEY,
                mint=VALID_PUBKEY_2,
            )

    def test_deposit_request_with_validator(self):
        """Test DepositRequest with optional validator."""
        req = DepositRequest(
            payer=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            validator=VALID_PUBKEY,
        )
        assert req.validator == VALID_PUBKEY

    def test_deposit_request_with_endpoint(self):
        """Test DepositRequest with optional endpoint."""
        req = DepositRequest(
            payer=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            endpoint_url=VALID_ENDPOINT,
        )
        assert req.endpoint_url == VALID_ENDPOINT


class TestPrepareWithdrawalRequest:
    """Test PrepareWithdrawalRequest model."""

    def test_prepare_withdrawal_required_fields(self):
        """Test required fields."""
        req = PrepareWithdrawalRequest(
            user=VALID_PUBKEY,
            mint=VALID_PUBKEY_2,
        )
        assert req.user == VALID_PUBKEY
        assert req.mint == VALID_PUBKEY_2

    def test_prepare_withdrawal_optional_endpoint(self):
        """Test optional endpoint field."""
        req = PrepareWithdrawalRequest(
            user=VALID_PUBKEY,
            mint=VALID_PUBKEY_2,
            endpoint_url=VALID_ENDPOINT,
        )
        assert req.endpoint_url == VALID_ENDPOINT


class TestDelegateEphemeralAtaPermissionRequest:
    """Test DelegateEphemeralAtaPermissionRequest model."""

    def test_delegate_permission_required_fields(self):
        """Test required fields."""
        req = DelegateEphemeralAtaPermissionRequest(
            payer=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
            validator=VALID_PUBKEY,
        )
        assert req.payer == VALID_PUBKEY
        assert req.validator == VALID_PUBKEY

    def test_delegate_permission_missing_validator(self):
        """Test validation with missing validator."""
        with pytest.raises(ValidationError):
            DelegateEphemeralAtaPermissionRequest(
                payer=VALID_PUBKEY,
                user=VALID_PUBKEY_2,
                mint=VALID_PUBKEY_3,
                # validator is required
            )


class TestUndelegateEphemeralAtaPermissionRequest:
    """Test UndelegateEphemeralAtaPermissionRequest model."""

    def test_undelegate_permission_required_fields(self):
        """Test required fields."""
        req = UndelegateEphemeralAtaPermissionRequest(
            payer=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
        )
        assert req.payer == VALID_PUBKEY


class TestUndelegateEphemeralAtaRequest:
    """Test UndelegateEphemeralAtaRequest model."""

    def test_undelegate_ata_required_fields(self):
        """Test required fields."""
        req = UndelegateEphemeralAtaRequest(
            payer=VALID_PUBKEY,
            user=VALID_PUBKEY_2,
            mint=VALID_PUBKEY_3,
        )
        assert req.payer == VALID_PUBKEY


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
