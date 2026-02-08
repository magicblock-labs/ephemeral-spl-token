#!/usr/bin/env python3
"""
Unit tests for the transaction builder module.
Tests transaction serialization, instruction building, and account metadata.
"""

import pytest
import base64
import struct
from solders.pubkey import Pubkey

# Import modules under test
from src.builder import (
    _pubkey_bytes,
    _encode_length,
    AccountMeta,
    Instruction,
    InstructionBuilder,
    serialize_transaction,
)
from src.config import get_settings


# Test fixtures
VALID_PUBKEY = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
VALID_PUBKEY2 = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
VALID_PUBKEY3 = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"


class TestPubkeyBytesConversion:
    """Test pubkey string to bytes conversion."""

    def test_pubkey_bytes_valid(self):
        """Test conversion of valid pubkey string to bytes."""
        result = _pubkey_bytes(VALID_PUBKEY)
        assert isinstance(result, bytes)
        assert len(result) == 32

    def test_pubkey_bytes_consistency(self):
        """Test that same pubkey always produces same bytes."""
        result1 = _pubkey_bytes(VALID_PUBKEY)
        result2 = _pubkey_bytes(VALID_PUBKEY)
        assert result1 == result2

    def test_pubkey_bytes_different_keys(self):
        """Test that different pubkeys produce different bytes."""
        result1 = _pubkey_bytes(VALID_PUBKEY)
        result2 = _pubkey_bytes(VALID_PUBKEY2)
        assert result1 != result2

    def test_pubkey_bytes_invalid_format(self):
        """Test that invalid pubkey raises error."""
        with pytest.raises(Exception):
            _pubkey_bytes("not-a-valid-pubkey")

    def test_pubkey_bytes_roundtrip(self):
        """Test that we can convert back to pubkey."""
        pubkey_bytes = _pubkey_bytes(VALID_PUBKEY)
        pubkey_obj = Pubkey(pubkey_bytes)
        assert str(pubkey_obj) == VALID_PUBKEY


class TestEncodeLength:
    """Test shortvec length encoding."""

    def test_encode_single_byte(self):
        """Test encoding values that fit in single byte."""
        result = _encode_length(0)
        assert result == b'\x00'
        
        result = _encode_length(127)
        assert result == b'\x7f'

    def test_encode_multi_byte(self):
        """Test encoding values requiring multiple bytes."""
        result = _encode_length(128)
        assert len(result) == 2
        
        result = _encode_length(255)
        assert len(result) == 2

    def test_encode_length_large_values(self):
        """Test encoding larger values."""
        result = _encode_length(16384)
        assert len(result) > 1

    def test_encode_length_consistency(self):
        """Test that same value always produces same encoding."""
        result1 = _encode_length(1000)
        result2 = _encode_length(1000)
        assert result1 == result2

    def test_encode_length_monotonic(self):
        """Test that encoded size doesn't decrease with larger values."""
        result1 = _encode_length(1)
        result2 = _encode_length(128)
        assert len(result2) >= len(result1)


class TestAccountMeta:
    """Test AccountMeta dataclass."""

    def test_account_meta_creation(self):
        """Test creating AccountMeta."""
        pubkey = _pubkey_bytes(VALID_PUBKEY)
        meta = AccountMeta(pubkey, is_signer=True, is_writable=False)
        assert meta.pubkey == pubkey
        assert meta.is_signer is True
        assert meta.is_writable is False

    def test_account_meta_frozen(self):
        """Test that AccountMeta is immutable."""
        pubkey = _pubkey_bytes(VALID_PUBKEY)
        meta = AccountMeta(pubkey, is_signer=True, is_writable=False)
        with pytest.raises(AttributeError):
            meta.is_signer = False

    def test_account_meta_equality(self):
        """Test AccountMeta equality."""
        pubkey1 = _pubkey_bytes(VALID_PUBKEY)
        meta1 = AccountMeta(pubkey1, is_signer=True, is_writable=False)
        meta2 = AccountMeta(pubkey1, is_signer=True, is_writable=False)
        assert meta1 == meta2

    def test_account_meta_different_flags(self):
        """Test AccountMeta with different flags."""
        pubkey = _pubkey_bytes(VALID_PUBKEY)
        meta1 = AccountMeta(pubkey, is_signer=True, is_writable=False)
        meta2 = AccountMeta(pubkey, is_signer=False, is_writable=True)
        assert meta1 != meta2


class TestInstruction:
    """Test Instruction dataclass."""

    def test_instruction_creation(self):
        """Test creating Instruction."""
        program_id = _pubkey_bytes(VALID_PUBKEY)
        data = b'\x00\x01\x02'
        accounts = [AccountMeta(_pubkey_bytes(VALID_PUBKEY2), True, False)]
        
        instr = Instruction(program_id, data, accounts)
        assert instr.program_id == program_id
        assert instr.data == data
        assert len(instr.accounts) == 1

    def test_instruction_frozen(self):
        """Test that Instruction is immutable."""
        program_id = _pubkey_bytes(VALID_PUBKEY)
        instr = Instruction(program_id, b'', [])
        with pytest.raises(AttributeError):
            instr.data = b'new'

    def test_instruction_multiple_accounts(self):
        """Test Instruction with multiple accounts."""
        program_id = _pubkey_bytes(VALID_PUBKEY)
        accounts = [
            AccountMeta(_pubkey_bytes(VALID_PUBKEY), True, True),
            AccountMeta(_pubkey_bytes(VALID_PUBKEY2), False, False),
            AccountMeta(_pubkey_bytes(VALID_PUBKEY3), True, False),
        ]
        instr = Instruction(program_id, b'', accounts)
        assert len(instr.accounts) == 3


class TestInstructionBuilder:
    """Test InstructionBuilder class."""

    def test_builder_initialization(self):
        """Test InstructionBuilder initialization."""
        builder = InstructionBuilder()
        assert builder.program_id is not None
        assert builder.system_program is not None
        assert builder.token_program is not None

    def test_initialize_ephemeral_ata(self):
        """Test building initialize_ephemeral_ata instruction."""
        builder = InstructionBuilder()
        payer = VALID_PUBKEY
        user = VALID_PUBKEY2
        mint = MINT
        ephemeral_ata = VALID_PUBKEY3
        bump = 42
        
        ix = builder.initialize_ephemeral_ata(payer, user, mint, ephemeral_ata, bump)
        
        assert isinstance(ix, Instruction)
        assert len(ix.accounts) == 5
        assert ix.data[0] == 0  # discriminator
        assert ix.data[1] == bump

    def test_initialize_global_vault(self):
        """Test building initialize_global_vault instruction."""
        builder = InstructionBuilder()
        payer = VALID_PUBKEY
        mint = MINT
        vault = VALID_PUBKEY3
        bump = 123
        
        ix = builder.initialize_global_vault(payer, mint, vault, bump)
        
        assert isinstance(ix, Instruction)
        assert len(ix.accounts) == 4
        assert ix.data[0] == 1  # discriminator
        assert ix.data[1] == bump

    def test_deposit_spl_tokens(self):
        """Test building deposit_spl_tokens instruction."""
        builder = InstructionBuilder()
        owner = VALID_PUBKEY
        source = VALID_PUBKEY2
        vault_token = VALID_PUBKEY3
        amount = 1000000
        ephemeral_ata = VALID_PUBKEY
        vault = VALID_PUBKEY2
        mint = MINT
        
        ix = builder.deposit_spl_tokens(
            owner, source, vault_token, amount,
            ephemeral_ata, vault, mint
        )
        
        assert isinstance(ix, Instruction)
        assert len(ix.accounts) == 7
        assert ix.data[0] == 2  # discriminator
        # Check amount encoding
        assert struct.unpack("<Q", ix.data[1:9])[0] == amount

    def test_withdraw_spl_tokens(self):
        """Test building withdraw_spl_tokens instruction."""
        builder = InstructionBuilder()
        owner = VALID_PUBKEY
        mint = MINT
        vault_source = VALID_PUBKEY2
        user_dest = VALID_PUBKEY3
        amount = 500000
        ephemeral_ata = VALID_PUBKEY
        vault = VALID_PUBKEY2
        bump = 200
        
        ix = builder.withdraw_spl_tokens(
            owner, mint, vault_source, user_dest, amount,
            ephemeral_ata, vault, bump
        )
        
        assert isinstance(ix, Instruction)
        assert len(ix.accounts) == 7
        assert ix.data[0] == 3  # discriminator

    def test_delegate_ephemeral_ata_without_validator(self):
        """Test building delegate_ephemeral_ata without validator."""
        builder = InstructionBuilder()
        payer = VALID_PUBKEY
        owner_program = VALID_PUBKEY3
        buffer = VALID_PUBKEY
        record = VALID_PUBKEY2
        metadata = VALID_PUBKEY3
        ephemeral_ata = VALID_PUBKEY
        bump = 50
        
        ix = builder.delegate_ephemeral_ata(
            payer, owner_program, buffer,
            record, metadata, ephemeral_ata, bump
        )
        
        assert isinstance(ix, Instruction)
        assert ix.data[0] == 4  # discriminator
        assert ix.data[2] == 0  # no validator

    def test_delegate_ephemeral_ata_with_validator(self):
        """Test building delegate_ephemeral_ata with validator."""
        builder = InstructionBuilder()
        payer = VALID_PUBKEY
        owner_program = VALID_PUBKEY3
        buffer = VALID_PUBKEY
        record = VALID_PUBKEY2
        metadata = VALID_PUBKEY3
        ephemeral_ata = VALID_PUBKEY
        bump = 50
        validator = VALID_PUBKEY
        
        ix = builder.delegate_ephemeral_ata(
            payer, owner_program, buffer,
            record, metadata, ephemeral_ata, bump, validator
        )
        
        assert isinstance(ix, Instruction)
        assert ix.data[0] == 4  # discriminator
        assert ix.data[2] == 1  # has validator

    def test_checked_transfer(self):
        """Test building checked_transfer instruction."""
        builder = InstructionBuilder()
        source = VALID_PUBKEY
        destination = VALID_PUBKEY2
        mint = MINT
        amount = 1000
        decimals = 6
        authority = VALID_PUBKEY3
        
        ix = builder.checked_transfer(
            source, destination, mint, amount, decimals, authority
        )
        
        assert isinstance(ix, Instruction)
        assert len(ix.accounts) == 4
        assert ix.data[0] == 12  # discriminator
        
        # Check amount and decimals encoding
        unpacked_amount = struct.unpack("<Q", ix.data[1:9])[0]
        unpacked_decimals = ix.data[9]
        assert unpacked_amount == amount
        assert unpacked_decimals == decimals

    def test_initialize_ata(self):
        """Test building initialize_ata instruction."""
        builder = InstructionBuilder()
        payer = VALID_PUBKEY
        owner = VALID_PUBKEY2
        mint = MINT
        ata = VALID_PUBKEY3
        
        ix = builder.initialize_ata(payer, owner, mint, ata)
        
        assert isinstance(ix, Instruction)
        assert len(ix.accounts) == 6
        assert ix.data[0] == 1  # ATA Initialize discriminator


class TestTransactionSerialization:
    """Test transaction serialization (requires async support)."""

    @pytest.mark.asyncio
    async def test_serialize_single_instruction(self):
        """Test serializing a single instruction."""
        builder = InstructionBuilder()
        ix = builder.initialize_ephemeral_ata(
            VALID_PUBKEY, VALID_PUBKEY2, MINT, VALID_PUBKEY3, 42
        )
        
        # This will need a valid endpoint or mock
        # For now, just test the function accepts the instruction
        assert isinstance(ix, Instruction)

    def test_instruction_data_encoding(self):
        """Test that instruction data is properly encoded."""
        builder = InstructionBuilder()
        amount = 123456789
        
        ix = builder.deposit_spl_tokens(
            VALID_PUBKEY, VALID_PUBKEY2, VALID_PUBKEY3, amount,
            VALID_PUBKEY, VALID_PUBKEY2, MINT
        )
        
        # Check that amount is properly encoded in little-endian
        encoded_amount = struct.unpack("<Q", ix.data[1:9])[0]
        assert encoded_amount == amount


class TestBuilderEdgeCases:
    """Test edge cases and error conditions."""

    def test_builder_with_custom_token_program(self):
        """Test deposit with custom token program."""
        builder = InstructionBuilder()
        custom_token_program = VALID_PUBKEY3
        
        ix = builder.deposit_spl_tokens(
            VALID_PUBKEY, VALID_PUBKEY2, VALID_PUBKEY3, 1000,
            VALID_PUBKEY, VALID_PUBKEY2, MINT,
            token_program=custom_token_program
        )
        
        assert isinstance(ix, Instruction)

    def test_zero_amount_deposit(self):
        """Test deposit with zero amount."""
        builder = InstructionBuilder()
        ix = builder.deposit_spl_tokens(
            VALID_PUBKEY, VALID_PUBKEY2, VALID_PUBKEY3, 0,
            VALID_PUBKEY, VALID_PUBKEY2, MINT
        )
        
        assert isinstance(ix, Instruction)

    def test_large_amount_deposit(self):
        """Test deposit with very large amount."""
        builder = InstructionBuilder()
        large_amount = 2**63 - 1  # Max int64
        
        ix = builder.deposit_spl_tokens(
            VALID_PUBKEY, VALID_PUBKEY2, VALID_PUBKEY3, large_amount,
            VALID_PUBKEY, VALID_PUBKEY2, MINT
        )
        
        assert isinstance(ix, Instruction)

    def test_all_permission_instructions(self):
        """Test all permission-related instructions."""
        builder = InstructionBuilder()
        
        # Create permission
        ix1 = builder.create_ephemeral_ata_permission(
            VALID_PUBKEY, MINT, 0, VALID_PUBKEY2, 10, VALID_PUBKEY3
        )
        assert ix1.data[0] == 6
        
        # Delegate permission
        ix2 = builder.delegate_ephemeral_ata_permission(
            VALID_PUBKEY, VALID_PUBKEY2, VALID_PUBKEY3,
            VALID_PUBKEY, VALID_PUBKEY2, VALID_PUBKEY3, 10, VALID_PUBKEY
        )
        assert ix2.data[0] == 7
        
        # Undelegate permission
        ix3 = builder.undelegate_ephemeral_ata_permission(
            VALID_PUBKEY, VALID_PUBKEY2, VALID_PUBKEY3
        )
        assert ix3.data[0] == 8
        
        # Reset permission
        ix4 = builder.reset_ephemeral_ata_permission(
            VALID_PUBKEY, MINT, 255, VALID_PUBKEY2, 10, VALID_PUBKEY3
        )
        assert ix4.data[0] == 9


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
