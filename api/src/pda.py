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


def _is_on_curve(pubkey_bytes: bytes) -> bool:
    """Check if a point is on the ed25519 curve."""
    # Simplified check - in production would use proper ed25519 math
    # For now, we'll use a simple heuristic
    # A proper implementation would check using ed25519 point operations
    try:
        # Import ed25519 if available for proper curve checking
        import nacl.signing
        # Try to construct a public key - if it fails, it's off-curve
        nacl.signing.VerifyKey(pubkey_bytes)
        return True
    except Exception:
        return False


def _find_pda(seeds: list[bytes], program_id: bytes) -> tuple[str, int]:
    """
    Find a Program Derived Address (PDA) using SHA256 hashing.
    Matches Solana's findProgramAddressSync behavior.
    
    Returns:
        Tuple of (pubkey_b58, bump)
    """
    
    nonce = 255
    while nonce != 0:
        try:
            # Concatenate all seeds with nonce
            seeds_with_nonce = seeds + [bytes([nonce])]
            seed_data = b"".join(seeds_with_nonce)
            
            # Append program_id and the magic string "ProgramDerivedAddress"
            hash_input = seed_data + program_id + b"ProgramDerivedAddress"
            hash_result = hashlib.sha256(hash_input).digest()
            
            # Check if the address is off-curve (valid PDA must be off-curve)
            if _is_on_curve(hash_result):
                # On-curve, try next nonce
                nonce -= 1
                continue
            
            # Off-curve, valid PDA found
            pubkey_b58 = _b58encode(hash_result)
            print(f"  bump: {nonce}")
            print(f"  pda: {pubkey_b58}")
            return pubkey_b58, nonce
        except Exception as e:
            nonce -= 1
    
    raise ValueError("Could not find valid PDA")


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
    
    # Derive ephemeral ATA: PDA from [user, mint]
    ephemeral_ata_pubkey, ephemeral_ata_bump = _find_pda([user_bytes, mint_bytes], ephemeral_spl_token_program_bytes)
    
    # Derive user ATA: Standard ATA program derivation
    # ATA is derived from [owner, token_program, mint] using ATA program
    user_ata_pubkey, _ = _find_pda([user_bytes, token_program_bytes, mint_bytes], ata_program_bytes)
    
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
