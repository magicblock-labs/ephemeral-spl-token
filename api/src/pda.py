"""PDA derivation utilities for Ephemeral SPL Token program and related programs."""

from typing import NamedTuple

from solders.pubkey import Pubkey

from .config import get_settings


def _pubkey_bytes(value: str) -> bytes:
    """Convert a base58 pubkey string to bytes using solders."""
    pubkey = Pubkey.from_string(value)
    return bytes(pubkey)


def _find_pda(seeds: list[bytes], program_id: bytes) -> tuple[str, int]:
    """
    Find a Program Derived Address (PDA) using solders.
    Matches Solana's findProgramAddressSync behavior.
    
    Args:
        seeds: List of seed bytes
        program_id: Program ID as bytes
    
    Returns:
        Tuple of (pubkey_b58, bump)
    """
    program_pubkey = Pubkey(program_id)
    pda, bump = Pubkey.find_program_address(seeds, program_pubkey)
    pubkey_b58 = str(pda)
    return pubkey_b58, bump


class DerivedAccounts(NamedTuple):
    """Accounts derived from user and mint."""
    vault: str
    vault_bump: int
    vault_ata: str
    ephemeral_ata: str
    ephemeral_ata_bump: int
    user_ata: str
    permission: str
    permission_bump: int
    eata_delegation_record: str
    eata_delegation_metadata: str
    eata_delegation_buffer: str
    permission_delegation_buffer: str
    permission_delegation_record: str
    permission_delegation_metadata: str


def derive_accounts(user: str, mint: str, ephemeral_spl_token_program: str = None, token_program: str = None, permission_program: str = None, delegation_program: str = None) -> DerivedAccounts:
    """
    Derive all necessary accounts from just user and mint addresses.
    
    Args:
        user: User/owner pubkey (base58)
        mint: Token mint pubkey (base58)
        ephemeral_spl_token_program: Ephemeral SPL Token program ID (defaults to config)
        token_program: SPL Token program ID (defaults to config)
        permission_program: Permission program ID (defaults to config)
        delegation_program: Delegation program ID (defaults to config)
    
    Returns:
        DerivedAccounts with all derived account addresses and bumps
    """
    settings = get_settings()
    ephemeral_spl_token_program_bytes = _pubkey_bytes(ephemeral_spl_token_program or settings.ephemeral_spl_token_program)
    permission_program_bytes = _pubkey_bytes(permission_program or settings.permission_program)
    delegation_program_bytes = _pubkey_bytes(delegation_program or settings.delegation_program)
    token_program_bytes = _pubkey_bytes(token_program or settings.token_program)
    ata_program_bytes = _pubkey_bytes(settings.ata_program)
    user_bytes = _pubkey_bytes(user)
    mint_bytes = _pubkey_bytes(mint)
    
    # Derive vault: PDA from [mint]
    vault_pubkey, vault_bump = _find_pda([mint_bytes], ephemeral_spl_token_program_bytes)
    
    # Derive vault ATA: Standard ATA for vault
    vault_bytes = _pubkey_bytes(vault_pubkey)
    vault_ata_pubkey, _ = _find_pda([vault_bytes, token_program_bytes, mint_bytes], ata_program_bytes)

    # Derive user ATA: Standard ATA program derivation
    user_ata_pubkey, _ = _find_pda([user_bytes, token_program_bytes, mint_bytes], ata_program_bytes)
    
    # Derive ephemeral ATA: PDA from [user, mint]
    ephemeral_ata_pubkey, ephemeral_ata_bump = _find_pda([user_bytes, mint_bytes], ephemeral_spl_token_program_bytes)

    # Derive permission: PDA from ["permission:", ephemeral_ata] using permission program
    # Convert ephemeral_ata back to bytes for this derivation
    ephemeral_ata_bytes = _pubkey_bytes(ephemeral_ata_pubkey)
    permission_pubkey, permission_bump = _find_pda([b"permission:", ephemeral_ata_bytes], permission_program_bytes)
    permission_bytes = _pubkey_bytes(permission_pubkey)
    
    # Derive delegation PDAs from ephemeral_ata and programs
    eata_delegation_record, _ = _find_pda([b"delegation", ephemeral_ata_bytes], delegation_program_bytes)
    eata_delegation_metadata, _ = _find_pda([b"delegation-metadata", ephemeral_ata_bytes], delegation_program_bytes)
    eata_delegation_buffer, _ = _find_pda([b"buffer", ephemeral_ata_bytes], ephemeral_spl_token_program_bytes)
    
    # Derive permission delegation PDAs from permission and programs
    permission_delegation_record, _ = _find_pda([b"delegation", permission_bytes], delegation_program_bytes)
    permission_delegation_metadata, _ = _find_pda([b"delegation-metadata", permission_bytes], delegation_program_bytes)
    permission_delegation_buffer, _ = _find_pda([b"buffer", permission_bytes], permission_program_bytes)
    
    return DerivedAccounts(
        vault=vault_pubkey,
        vault_bump=vault_bump,
        vault_ata=vault_ata_pubkey,
        ephemeral_ata=ephemeral_ata_pubkey,
        ephemeral_ata_bump=ephemeral_ata_bump,
        user_ata=user_ata_pubkey,
        permission=permission_pubkey,
        permission_bump=permission_bump,
        eata_delegation_record=eata_delegation_record,
        eata_delegation_metadata=eata_delegation_metadata,
        eata_delegation_buffer=eata_delegation_buffer,
        permission_delegation_buffer=permission_delegation_buffer,
        permission_delegation_record=permission_delegation_record,
        permission_delegation_metadata=permission_delegation_metadata,
    )
