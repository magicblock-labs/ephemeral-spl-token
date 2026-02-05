"""PDA derivation utilities for Ephemeral SPL Token program and related programs."""

import hashlib
from typing import NamedTuple

from .config import get_settings


_B58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
_B58_INDEX = {ch: i for i, ch in enumerate(_B58_ALPHABET)}


def _b58decode(value: str) -> bytes:
    """Decode a base58 string into bytes."""
    num = 0
    for ch in value:
        num = num * 58 + _B58_INDEX[ch]
    if num == 0:
        decoded = b""
    else:
        decoded = num.to_bytes((num.bit_length() + 7) // 8, "big")
    pad = len(value) - len(value.lstrip("1"))
    return (b"\x00" * pad) + decoded


def _b58encode(data: bytes) -> str:
    """Encode bytes to base58 string."""
    if not data:
        return ""
    
    num = int.from_bytes(data, "big")
    encoded = ""
    while num > 0:
        num, remainder = divmod(num, 58)
        encoded = _B58_ALPHABET[remainder] + encoded
    
    # Add leading 1s for leading zeros
    pad = len(data) - len(data.lstrip(b"\x00"))
    return "1" * pad + encoded


def _pubkey_bytes(value: str) -> bytes:
    """Convert a base58 pubkey string to bytes."""
    key = _b58decode(value)
    if len(key) != 32:
        raise ValueError("Invalid pubkey length")
    return key


def _find_pda(seeds: list[bytes], program_id: bytes) -> tuple[str, int]:
    """
    Find a Program Derived Address (PDA) using SHA256 hashing.
    
    Returns:
        Tuple of (pubkey_b58, bump)
    """
    bump = 255
    while bump >= 0:
        try:
            seed_data = b"".join(seeds) + bytes([bump])
            hash_result = hashlib.sha256(seed_data + program_id).digest()
            
            # Check if it's a valid pubkey (we just use the hash directly)
            # In real Solana, this would check if it's off-curve
            pubkey_b58 = _b58encode(hash_result)
            return pubkey_b58, bump
        except Exception:
            bump -= 1
    
    raise ValueError("Could not find valid PDA")


class DerivedAccounts(NamedTuple):
    """Accounts derived from user and mint."""
    vault: str
    vault_bump: int
    ephemeral_ata: str
    ephemeral_ata_bump: int
    user_ata: str
    permission: str
    permission_bump: int


def derive_accounts(user: str, mint: str, ephemeral_spl_token_program: str = None, token_program: str = None) -> DerivedAccounts:
    """
    Derive all necessary accounts from just user and mint addresses.
    
    Args:
        user: User/owner pubkey (base58)
        mint: Token mint pubkey (base58)
        ephemeral_spl_token_program: Ephemeral SPL Token program ID (defaults to config)
        token_program: SPL Token program ID (defaults to config)
    
    Returns:
        DerivedAccounts with all derived account addresses and bumps
    """
    settings = get_settings()
    program_id = _pubkey_bytes(ephemeral_spl_token_program or settings.ephemeral_spl_token_program)
    user_bytes = _pubkey_bytes(user)
    mint_bytes = _pubkey_bytes(mint)
    
    # Derive vault: PDA from [mint]
    vault_pubkey, vault_bump = _find_pda([mint_bytes], program_id)
    
    # Derive ephemeral ATA: PDA from [user, mint]
    ephemeral_ata_pubkey, ephemeral_ata_bump = _find_pda([user_bytes, mint_bytes], program_id)
    
    # Derive user ATA: Standard ATA program derivation
    # ATA is derived from [owner, token_program, mint] using ATA program
    token_prog_id = _pubkey_bytes(token_program or settings.token_program)
    ata_program_id = _pubkey_bytes(settings.ata_program)
    user_ata_pubkey, _ = _find_pda([user_bytes, token_prog_id, mint_bytes], ata_program_id)
    
    # Derive permission: PDA from ["permission:", ephemeral_ata]
    # Convert ephemeral_ata back to bytes for this derivation
    ephemeral_ata_bytes = _pubkey_bytes(ephemeral_ata_pubkey)
    permission_pubkey, permission_bump = _find_pda([b"permission:", ephemeral_ata_bytes], program_id)
    
    return DerivedAccounts(
        vault=vault_pubkey,
        vault_bump=vault_bump,
        ephemeral_ata=ephemeral_ata_pubkey,
        ephemeral_ata_bump=ephemeral_ata_bump,
        user_ata=user_ata_pubkey,
        permission=permission_pubkey,
        permission_bump=permission_bump,
    )


def derive_vault(mint: str, ephemeral_spl_token_program: str = None) -> tuple[str, int]:
    """Derive global vault PDA from mint."""
    settings = get_settings()
    program_id = _pubkey_bytes(ephemeral_spl_token_program or settings.ephemeral_spl_token_program)
    mint_bytes = _pubkey_bytes(mint)
    return _find_pda([mint_bytes], program_id)


def derive_ephemeral_ata(user: str, mint: str, ephemeral_spl_token_program: str = None) -> tuple[str, int]:
    """Derive ephemeral ATA PDA from user and mint."""
    settings = get_settings()
    program_id = _pubkey_bytes(ephemeral_spl_token_program or settings.ephemeral_spl_token_program)
    user_bytes = _pubkey_bytes(user)
    mint_bytes = _pubkey_bytes(mint)
    return _find_pda([user_bytes, mint_bytes], program_id)


def derive_user_ata(user: str, mint: str, token_program: str = None) -> str:
    """Derive user's standard ATA from user and mint."""
    settings = get_settings()
    token_prog_id = _pubkey_bytes(token_program or settings.token_program)
    ata_program_id = _pubkey_bytes(settings.ata_program)
    user_bytes = _pubkey_bytes(user)
    mint_bytes = _pubkey_bytes(mint)
    ata_pubkey, _ = _find_pda([user_bytes, token_prog_id, mint_bytes], ata_program_id)
    return ata_pubkey


def derive_vault_ata(vault: str, mint: str, token_program: str = None) -> str:
    """Derive vault's ATA from vault address and mint."""
    settings = get_settings()
    token_prog_id = _pubkey_bytes(token_program or settings.token_program)
    ata_program_id = _pubkey_bytes(settings.ata_program)
    vault_bytes = _pubkey_bytes(vault)
    mint_bytes = _pubkey_bytes(mint)
    vault_ata_pubkey, _ = _find_pda([vault_bytes, token_prog_id, mint_bytes], ata_program_id)
    return vault_ata_pubkey


def derive_permission(ephemeral_ata: str, ephemeral_spl_token_program: str = None) -> tuple[str, int]:
    """Derive permission PDA from ephemeral ATA."""
    settings = get_settings()
    program_id = _pubkey_bytes(ephemeral_spl_token_program or settings.ephemeral_spl_token_program)
    ephemeral_ata_bytes = _pubkey_bytes(ephemeral_ata)
    permission, permission_bump = _find_pda([b"permission:", ephemeral_ata_bytes], program_id)
    return permission, permission_bump


class DelegationPDAs(NamedTuple):
    """Delegation program PDAs derived from ephemeral ATA."""
    delegation_record: str
    delegation_metadata: str
    delegation_buffer: str
    undelegate_buffer: str
    commit_state: str
    commit_record: str


def derive_delegation_pdas(
    ephemeral_ata: str,
    ephemeral_spl_token_program: str,
    delegation_program: str = None,
) -> DelegationPDAs:
    """
    Derive all delegation PDAs from ephemeral ATA and programs.
    
    Delegation PDAs are derived using the delegation program and ephemeral SPL token program:
    - delegation_record: ["delegation", ephemeral_ata] with delegation_program
    - delegation_metadata: ["delegation-metadata", ephemeral_ata] with delegation_program
    - delegation_buffer: ["buffer", ephemeral_ata] with ephemeral_spl_token_program
    - undelegate_buffer: ["undelegate-buffer", ephemeral_ata] with delegation_program
    - commit_state: ["state-diff", ephemeral_ata] with delegation_program
    - commit_record: ["commit-state-record", ephemeral_ata] with delegation_program
    
    Args:
        ephemeral_ata: The ephemeral ATA address (base58)
        ephemeral_spl_token_program: The ephemeral SPL token program ID (base58)
        delegation_program: Delegation program ID (defaults from config)
    
    Returns:
        DelegationPDAs with all delegation-related account addresses
    """
    settings = get_settings()
    delegation_prog_id = _pubkey_bytes(delegation_program or settings.delegation_program)
    ephemeral_prog_id = _pubkey_bytes(ephemeral_spl_token_program)
    ephemeral_ata_bytes = _pubkey_bytes(ephemeral_ata)
    
    # All these PDAs use delegation_program except delegation_buffer
    delegation_record, _ = _find_pda([b"delegation", ephemeral_ata_bytes], delegation_prog_id)
    delegation_metadata, _ = _find_pda([b"delegation-metadata", ephemeral_ata_bytes], delegation_prog_id)
    delegation_buffer, _ = _find_pda([b"buffer", ephemeral_ata_bytes], ephemeral_prog_id)  # Uses ephemeral_spl_token_program!
    undelegate_buffer, _ = _find_pda([b"undelegate-buffer", ephemeral_ata_bytes], delegation_prog_id)
    commit_state, _ = _find_pda([b"state-diff", ephemeral_ata_bytes], delegation_prog_id)
    commit_record, _ = _find_pda([b"commit-state-record", ephemeral_ata_bytes], delegation_prog_id)
    
    return DelegationPDAs(
        delegation_record=delegation_record,
        delegation_metadata=delegation_metadata,
        delegation_buffer=delegation_buffer,
        undelegate_buffer=undelegate_buffer,
        commit_state=commit_state,
        commit_record=commit_record,
    )


def derive_delegation_record(ephemeral_ata: str, delegation_program: str = None) -> str:
    """Derive delegation record PDA from ephemeral ATA."""
    settings = get_settings()
    delegation_prog_id = _pubkey_bytes(delegation_program or settings.delegation_program)
    ephemeral_ata_bytes = _pubkey_bytes(ephemeral_ata)
    record, _ = _find_pda([b"delegation", ephemeral_ata_bytes], delegation_prog_id)
    return record


def derive_delegation_metadata(ephemeral_ata: str, delegation_program: str = None) -> str:
    """Derive delegation metadata PDA from ephemeral ATA."""
    settings = get_settings()
    delegation_prog_id = _pubkey_bytes(delegation_program or settings.delegation_program)
    ephemeral_ata_bytes = _pubkey_bytes(ephemeral_ata)
    metadata, _ = _find_pda([b"delegation-metadata", ephemeral_ata_bytes], delegation_prog_id)
    return metadata


def derive_delegation_buffer(ephemeral_ata: str, ephemeral_spl_token_program: str) -> str:
    """Derive delegation buffer PDA from ephemeral ATA and ephemeral SPL token program.
    
    Note: This uses ephemeral_spl_token_program, NOT delegation_program!
    """
    ephemeral_prog_id = _pubkey_bytes(ephemeral_spl_token_program)
    ephemeral_ata_bytes = _pubkey_bytes(ephemeral_ata)
    buffer, _ = _find_pda([b"buffer", ephemeral_ata_bytes], ephemeral_prog_id)
    return buffer


def derive_undelegate_buffer(ephemeral_ata: str, delegation_program: str = None) -> str:
    """Derive undelegate buffer PDA from ephemeral ATA."""
    settings = get_settings()
    delegation_prog_id = _pubkey_bytes(delegation_program or settings.delegation_program)
    ephemeral_ata_bytes = _pubkey_bytes(ephemeral_ata)
    buffer, _ = _find_pda([b"undelegate-buffer", ephemeral_ata_bytes], delegation_prog_id)
    return buffer


class PermissionDelegationPDAs(NamedTuple):
    """Delegation program PDAs derived from permission account."""
    delegation_buffer: str
    delegation_record: str
    delegation_metadata: str


def derive_permission_delegation_pdas(
    permission: str,
    permission_program: str = None,
    delegation_program: str = None,
) -> PermissionDelegationPDAs:
    """
    Derive all permission delegation PDAs from permission account and programs.
    
    Similar to ephemeral_ata delegation, but uses permission as the delegated account.
    Permission is owned by permission_program, not ephemeral_spl_token_program.
    
    Args:
        permission: The permission account address (base58)
        permission_program: The permission program ID (base58)
        delegation_program: Delegation program ID (defaults from config)
    
    Returns:
        PermissionDelegationPDAs with all delegation-related account addresses
    """
    settings = get_settings()
    delegation_prog_id = _pubkey_bytes(delegation_program or settings.delegation_program)
    permission_prog_id = _pubkey_bytes(permission_program or settings.permission_program)
    permission_bytes = _pubkey_bytes(permission)
    
    delegation_record, _ = _find_pda([b"delegation", permission_bytes], delegation_prog_id)
    delegation_metadata, _ = _find_pda([b"delegation-metadata", permission_bytes], delegation_prog_id)
    delegation_buffer, _ = _find_pda([b"buffer", permission_bytes], permission_prog_id)
    
    return PermissionDelegationPDAs(
        delegation_buffer=delegation_buffer,
        delegation_record=delegation_record,
        delegation_metadata=delegation_metadata,
    )
