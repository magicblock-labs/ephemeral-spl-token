#!/usr/bin/env python3
"""
Unit tests for PDA (Program Derived Address) derivation utilities.
Tests account derivation from user and mint addresses.
"""

import pytest

from src.pda import (
    _pubkey_bytes,
    _find_pda,
    derive_accounts,
    DerivedAccounts,
)
from src.config import get_settings


# Test fixtures with valid Solana addresses
VALID_USER = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
VALID_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
VALID_USER_2 = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
VALID_MINT_2 = "So11111111111111111111111111111111111111112"


class TestPubkeyBytesConversion:
    """Test pubkey string to bytes conversion."""

    def test_pubkey_bytes_valid(self):
        """Test conversion of valid pubkey."""
        result = _pubkey_bytes(VALID_USER)
        assert isinstance(result, bytes)
        assert len(result) == 32

    def test_pubkey_bytes_consistency(self):
        """Test that same pubkey always produces same bytes."""
        result1 = _pubkey_bytes(VALID_USER)
        result2 = _pubkey_bytes(VALID_USER)
        assert result1 == result2

    def test_pubkey_bytes_different_keys(self):
        """Test that different pubkeys produce different bytes."""
        result1 = _pubkey_bytes(VALID_USER)
        result2 = _pubkey_bytes(VALID_MINT)
        assert result1 != result2

    def test_pubkey_bytes_invalid_format(self):
        """Test that invalid pubkey raises error."""
        with pytest.raises(Exception):
            _pubkey_bytes("invalid-pubkey-format")


class TestFindPDA:
    """Test PDA finding functionality."""

    def test_find_pda_returns_tuple(self):
        """Test that _find_pda returns (pubkey_str, bump)."""
        settings = get_settings()
        program_id = _pubkey_bytes(settings.ephemeral_spl_token_program)
        user_bytes = _pubkey_bytes(VALID_USER)
        mint_bytes = _pubkey_bytes(VALID_MINT)
        
        result = _find_pda([user_bytes, mint_bytes], program_id)
        assert isinstance(result, tuple)
        assert len(result) == 2
        
        pubkey_str, bump = result
        assert isinstance(pubkey_str, str)
        assert isinstance(bump, int)

    def test_find_pda_bump_in_range(self):
        """Test that bump value is valid (0-255)."""
        settings = get_settings()
        program_id = _pubkey_bytes(settings.ephemeral_spl_token_program)
        user_bytes = _pubkey_bytes(VALID_USER)
        mint_bytes = _pubkey_bytes(VALID_MINT)
        
        _, bump = _find_pda([user_bytes, mint_bytes], program_id)
        assert 0 <= bump <= 255

    def test_find_pda_consistency(self):
        """Test that same seeds always produce same PDA."""
        settings = get_settings()
        program_id = _pubkey_bytes(settings.ephemeral_spl_token_program)
        user_bytes = _pubkey_bytes(VALID_USER)
        mint_bytes = _pubkey_bytes(VALID_MINT)
        
        result1 = _find_pda([user_bytes, mint_bytes], program_id)
        result2 = _find_pda([user_bytes, mint_bytes], program_id)
        assert result1 == result2

    def test_find_pda_different_seeds(self):
        """Test that different seeds produce different PDAs."""
        settings = get_settings()
        program_id = _pubkey_bytes(settings.ephemeral_spl_token_program)
        
        result1 = _find_pda([_pubkey_bytes(VALID_USER)], program_id)
        result2 = _find_pda([_pubkey_bytes(VALID_MINT)], program_id)
        assert result1 != result2

    def test_find_pda_multiple_seeds(self):
        """Test PDA derivation with multiple seeds."""
        settings = get_settings()
        program_id = _pubkey_bytes(settings.permission_program)
        ephemeral_ata = _pubkey_bytes(VALID_USER)
        
        pda_str, bump = _find_pda([b"permission:", ephemeral_ata], program_id)
        assert isinstance(pda_str, str)
        assert len(pda_str) > 0
        assert 0 <= bump <= 255


class TestDerivedAccounts:
    """Test DerivedAccounts namedtuple."""

    def test_derived_accounts_creation(self):
        """Test creating DerivedAccounts namedtuple."""
        accounts = DerivedAccounts(
            vault="vault_addr",
            vault_bump=1,
            vault_ata="vault_ata_addr",
            ephemeral_ata="eata_addr",
            ephemeral_ata_bump=2,
            user_ata="user_ata_addr",
            permission="perm_addr",
            permission_bump=3,
            eata_delegation_record="eata_delrec",
            eata_delegation_metadata="eata_delmeta",
            eata_delegation_buffer="eata_delbuf",
            permission_delegation_buffer="perm_delbuf",
            permission_delegation_record="perm_delrec",
            permission_delegation_metadata="perm_delmeta",
        )
        
        assert accounts.vault == "vault_addr"
        assert accounts.vault_bump == 1
        assert accounts.ephemeral_ata_bump == 2
        assert accounts.permission_bump == 3

    def test_derived_accounts_unpacking(self):
        """Test unpacking DerivedAccounts."""
        accounts = DerivedAccounts(
            vault="v1", vault_bump=1, vault_ata="v2",
            ephemeral_ata="e1", ephemeral_ata_bump=2,
            user_ata="u1", permission="p1", permission_bump=3,
            eata_delegation_record="ed1",
            eata_delegation_metadata="ed2",
            eata_delegation_buffer="ed3",
            permission_delegation_buffer="pd1",
            permission_delegation_record="pd2",
            permission_delegation_metadata="pd3",
        )
        
        vault, vault_bump, vault_ata, eata, eata_bump, user_ata, \
        perm, perm_bump, edr, edm, edb, pdb, pdr, pdm = accounts
        
        assert vault == "v1"
        assert vault_bump == 1


class TestDeriveAccounts:
    """Test account derivation functionality."""

    def test_derive_accounts_basic(self):
        """Test basic account derivation."""
        result = derive_accounts(VALID_USER, VALID_MINT)
        
        assert isinstance(result, DerivedAccounts)
        assert result.vault is not None
        assert result.ephemeral_ata is not None
        assert result.vault_ata is not None
        assert result.user_ata is not None
        assert result.permission is not None

    def test_derive_accounts_returns_valid_pubkeys(self):
        """Test that all derived accounts are valid pubkeys."""
        result = derive_accounts(VALID_USER, VALID_MINT)
        
        # All addresses should be non-empty strings
        assert len(result.vault) > 0
        assert len(result.vault_ata) > 0
        assert len(result.ephemeral_ata) > 0
        assert len(result.user_ata) > 0
        assert len(result.permission) > 0

    def test_derive_accounts_bumps_valid(self):
        """Test that bump values are in valid range."""
        result = derive_accounts(VALID_USER, VALID_MINT)
        
        assert 0 <= result.vault_bump <= 255
        assert 0 <= result.ephemeral_ata_bump <= 255
        assert 0 <= result.permission_bump <= 255

    def test_derive_accounts_consistency(self):
        """Test that same user/mint always produces same accounts."""
        result1 = derive_accounts(VALID_USER, VALID_MINT)
        result2 = derive_accounts(VALID_USER, VALID_MINT)
        
        assert result1 == result2

    def test_derive_accounts_different_user(self):
        """Test that different users produce different ephemeral ATAs."""
        result1 = derive_accounts(VALID_USER, VALID_MINT)
        result2 = derive_accounts(VALID_USER_2, VALID_MINT)
        
        # Ephemeral ATA should be different
        assert result1.ephemeral_ata != result2.ephemeral_ata
        # But vault should be same (derived from mint only)
        assert result1.vault == result2.vault

    def test_derive_accounts_different_mint(self):
        """Test that different mints produce different vaults."""
        result1 = derive_accounts(VALID_USER, VALID_MINT)
        result2 = derive_accounts(VALID_USER, VALID_MINT_2)
        
        # Vault and vault_ata should be different
        assert result1.vault != result2.vault
        assert result1.vault_ata != result2.vault_ata
        # Ephemeral ATA should be different
        assert result1.ephemeral_ata != result2.ephemeral_ata

    def test_derive_accounts_with_custom_programs(self):
        """Test account derivation with custom program IDs."""
        settings = get_settings()
        
        result = derive_accounts(
            VALID_USER,
            VALID_MINT,
            ephemeral_spl_token_program=settings.ephemeral_spl_token_program,
            token_program=settings.token_program,
            permission_program=settings.permission_program,
            delegation_program=settings.delegation_program,
        )
        
        assert isinstance(result, DerivedAccounts)

    def test_derive_accounts_delegation_pdas(self):
        """Test that delegation PDAs are derived."""
        result = derive_accounts(VALID_USER, VALID_MINT)
        
        assert len(result.eata_delegation_record) > 0
        assert len(result.eata_delegation_metadata) > 0
        assert len(result.eata_delegation_buffer) > 0
        assert len(result.permission_delegation_buffer) > 0
        assert len(result.permission_delegation_record) > 0
        assert len(result.permission_delegation_metadata) > 0

    def test_derive_accounts_no_empty_fields(self):
        """Test that no fields in DerivedAccounts are empty."""
        result = derive_accounts(VALID_USER, VALID_MINT)
        
        # Check all string fields are non-empty
        assert result.vault
        assert result.vault_ata
        assert result.ephemeral_ata
        assert result.user_ata
        assert result.permission
        assert result.eata_delegation_record
        assert result.eata_delegation_metadata
        assert result.eata_delegation_buffer
        assert result.permission_delegation_buffer
        assert result.permission_delegation_record
        assert result.permission_delegation_metadata

    def test_vault_same_for_same_mint(self):
        """Test that vault address is same for same mint (different users)."""
        result1 = derive_accounts(VALID_USER, VALID_MINT)
        result2 = derive_accounts(VALID_USER_2, VALID_MINT)
        
        assert result1.vault == result2.vault
        assert result1.vault_bump == result2.vault_bump

    def test_user_ata_derivation(self):
        """Test that user ATA is properly derived."""
        result = derive_accounts(VALID_USER, VALID_MINT)
        
        # User ATA should be derived from user and mint
        assert result.user_ata is not None
        assert len(result.user_ata) > 0

    def test_ephemeral_ata_derivation(self):
        """Test that ephemeral ATA is properly derived."""
        result = derive_accounts(VALID_USER, VALID_MINT)
        
        # Ephemeral ATA should be derived from user and mint
        assert result.ephemeral_ata is not None
        assert len(result.ephemeral_ata) > 0
        # Different from user ATA
        assert result.ephemeral_ata != result.user_ata

    def test_permission_derivation(self):
        """Test that permission PDA is properly derived."""
        result = derive_accounts(VALID_USER, VALID_MINT)
        
        # Permission should be derived from ephemeral_ata
        assert result.permission is not None
        assert len(result.permission) > 0
        # Different from other accounts
        assert result.permission != result.ephemeral_ata
        assert result.permission != result.user_ata


class TestAccountDerivationEdgeCases:
    """Test edge cases in account derivation."""

    def test_derive_accounts_with_none_programs(self):
        """Test derivation with None program overrides (should use defaults)."""
        result = derive_accounts(
            VALID_USER,
            VALID_MINT,
            ephemeral_spl_token_program=None,
            token_program=None,
            permission_program=None,
            delegation_program=None,
        )
        
        assert isinstance(result, DerivedAccounts)

    def test_derive_accounts_program_override(self):
        """Test that program override affects derivation."""
        settings = get_settings()
        
        result1 = derive_accounts(VALID_USER, VALID_MINT)
        result2 = derive_accounts(
            VALID_USER,
            VALID_MINT,
            ephemeral_spl_token_program=settings.ephemeral_spl_token_program
        )
        
        # Should be the same when using the same program ID
        assert result1 == result2


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
