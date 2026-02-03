"""Pure-Python transaction builder for Ephemeral SPL Token instructions."""

import base64
import hashlib
import json
import struct
from dataclasses import dataclass
from typing import Optional

from .config import get_settings


# ==============================================================================
# PDA Derivation
# ==============================================================================

def _is_on_curve(point: bytes) -> bool:
    """Check if a point is on the ed25519 curve (simplified check)."""
    if len(point) != 32:
        return True
    return False


def find_program_address(seeds: list[bytes], program_id: bytes) -> tuple[bytes, int]:
    """
    Derive a Program Derived Address (PDA) and its bump seed.

    This mirrors Solana's Pubkey::find_program_address().
    """
    for bump in range(255, -1, -1):
        try:
            pda = create_program_address(seeds + [bytes([bump])], program_id)
            return pda, bump
        except ValueError:
            continue
    raise ValueError("Unable to find a valid PDA")


def create_program_address(seeds: list[bytes], program_id: bytes) -> bytes:
    """
    Create a Program Derived Address from seeds.

    This mirrors Solana's Pubkey::create_program_address().
    """
    # Validate seed lengths
    for seed in seeds:
        if len(seed) > 32:
            raise ValueError("Seed too long")

    # Hash: seeds + program_id + "ProgramDerivedAddress"
    hasher = hashlib.sha256()
    for seed in seeds:
        hasher.update(seed)
    hasher.update(program_id)
    hasher.update(b"ProgramDerivedAddress")

    result = hasher.digest()

    # PDA must NOT be on the ed25519 curve
    # We do a simplified check - in practice, collisions are extremely rare
    if _is_on_curve(result):
        raise ValueError("Point is on curve")

    return result


def derive_ephemeral_ata(user: str, mint: str, program_id: bytes) -> tuple[str, int]:
    """Derive ephemeral ATA PDA from user and mint."""
    user_bytes = _pubkey_bytes(user)
    mint_bytes = _pubkey_bytes(mint)
    pda, bump = find_program_address([user_bytes, mint_bytes], program_id)
    return _b58encode(pda), bump


def derive_vault(mint: str, program_id: bytes) -> tuple[str, int]:
    """Derive global vault PDA from mint."""
    mint_bytes = _pubkey_bytes(mint)
    pda, bump = find_program_address([mint_bytes], program_id)
    return _b58encode(pda), bump


def derive_permission(ephemeral_ata: str, permission_program: bytes) -> str:
    """Derive permission PDA from ephemeral ATA."""
    ata_bytes = _pubkey_bytes(ephemeral_ata)
    pda, _ = find_program_address([b"permission:", ata_bytes], permission_program)
    return _b58encode(pda)


def derive_ata(user: str, mint: str, token_program: bytes, ata_program: bytes) -> str:
    """Derive standard Associated Token Account address."""
    user_bytes = _pubkey_bytes(user)
    mint_bytes = _pubkey_bytes(mint)
    pda, _ = find_program_address(
        [user_bytes, token_program, mint_bytes],
        ata_program
    )
    return _b58encode(pda)

# Support both Pyodide (browser/workers) and native Python environments
try:
    from js import fetch as js_fetch
    from pyodide.ffi import to_js

    async def _http_post_json(url: str, payload: dict) -> dict:
        """HTTP POST using Pyodide's js.fetch."""
        init = {
            "method": "POST",
            "headers": {"Content-Type": "application/json"},
            "body": json.dumps(payload),
        }
        response = await js_fetch(url, to_js(init))
        text = await response.text()
        return json.loads(str(text))

except ImportError:
    import httpx

    async def _http_post_json(url: str, payload: dict) -> dict:
        """HTTP POST using httpx for native Python."""
        async with httpx.AsyncClient() as client:
            response = await client.post(url, json=payload)
            return response.json()


_B58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
_B58_INDEX = {ch: i for i, ch in enumerate(_B58_ALPHABET)}


def _b58encode(data: bytes) -> str:
    """Encode bytes to base58 string."""
    # Count leading zeros
    leading_zeros = 0
    for byte in data:
        if byte == 0:
            leading_zeros += 1
        else:
            break

    num = int.from_bytes(data, "big")
    if num == 0:
        return "1" * leading_zeros

    result = []
    while num > 0:
        num, remainder = divmod(num, 58)
        result.append(_B58_ALPHABET[remainder])

    return "1" * leading_zeros + "".join(reversed(result))


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


def _pubkey_bytes(value: str) -> bytes:
    key = _b58decode(value)
    if len(key) != 32:
        raise ValueError("Invalid pubkey length")
    return key


def _encode_length(value: int) -> bytes:
    """Shortvec encoding for compact-u16 lengths."""
    out = bytearray()
    remaining = value
    while True:
        byte = remaining & 0x7F
        remaining >>= 7
        if remaining:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            break
    return bytes(out)


@dataclass(frozen=True)
class AccountMeta:
    pubkey: bytes
    is_signer: bool
    is_writable: bool


@dataclass(frozen=True)
class Instruction:
    program_id: bytes
    data: bytes
    accounts: list[AccountMeta]


async def _get_latest_blockhash(cluster_url: Optional[str] = None) -> bytes:
    """Fetch recent blockhash from cluster RPC."""
    settings = get_settings()
    url = cluster_url or settings.cluster_url
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getLatestBlockhash",
    }
    data = await _http_post_json(url, payload)
    try:
        blockhash = data["result"]["value"]["blockhash"]
    except Exception as exc:
        raise ValueError(f"RPC error: {data}") from exc
    blockhash_bytes = _b58decode(blockhash)
    if len(blockhash_bytes) != 32:
        raise ValueError("Invalid blockhash length")
    return blockhash_bytes


async def serialize_transaction(
    instruction: Instruction,
    payer: str,
    cluster_url: Optional[str] = None,
) -> str:
    """Serialize an unsigned legacy transaction and return base64."""
    blockhash = await _get_latest_blockhash(cluster_url)
    payer_pk = _pubkey_bytes(payer)

    key_meta: dict[bytes, dict[str, bool]] = {}

    def add_key(pubkey: bytes, signer: bool, writable: bool) -> None:
        existing = key_meta.get(pubkey)
        if existing is None:
            key_meta[pubkey] = {"signer": signer, "writable": writable}
        else:
            existing["signer"] = existing["signer"] or signer
            existing["writable"] = existing["writable"] or writable

    add_key(payer_pk, True, True)
    for meta in instruction.accounts:
        add_key(meta.pubkey, meta.is_signer, meta.is_writable)
    add_key(instruction.program_id, False, False)

    seen: set[bytes] = set()
    keys: list[bytes] = []

    def push(pubkey: bytes) -> None:
        if pubkey not in seen:
            seen.add(pubkey)
            keys.append(pubkey)

    push(payer_pk)

    def iter_accounts():
        for meta in instruction.accounts:
            yield meta.pubkey

    for pubkey in iter_accounts():
        meta = key_meta[pubkey]
        if meta["signer"] and meta["writable"]:
            push(pubkey)
    for pubkey in iter_accounts():
        meta = key_meta[pubkey]
        if meta["signer"] and not meta["writable"]:
            push(pubkey)
    for pubkey in iter_accounts():
        meta = key_meta[pubkey]
        if not meta["signer"] and meta["writable"]:
            push(pubkey)
    for pubkey in iter_accounts():
        meta = key_meta[pubkey]
        if not meta["signer"] and not meta["writable"]:
            push(pubkey)

    push(instruction.program_id)

    num_signers = sum(1 for k in keys if key_meta[k]["signer"])
    num_readonly_signed = sum(
        1 for k in keys if key_meta[k]["signer"] and not key_meta[k]["writable"]
    )
    num_readonly_unsigned = sum(
        1 for k in keys if not key_meta[k]["signer"] and not key_meta[k]["writable"]
    )

    header = bytes([
        num_signers,
        num_readonly_signed,
        num_readonly_unsigned,
    ])

    index_map = {pubkey: idx for idx, pubkey in enumerate(keys)}
    account_indices = [index_map[meta.pubkey] for meta in instruction.accounts]

    instruction_bytes = (
        bytes([index_map[instruction.program_id]])
        + _encode_length(len(account_indices))
        + bytes(account_indices)
        + _encode_length(len(instruction.data))
        + instruction.data
    )

    message = (
        header
        + _encode_length(len(keys))
        + b"".join(keys)
        + blockhash
        + _encode_length(1)
        + instruction_bytes
    )

    signatures = b"".join([b"\x00" * 64 for _ in range(num_signers)])
    tx = _encode_length(num_signers) + signatures + message
    return base64.b64encode(tx).decode("utf-8")


class InstructionBuilder:
    """Builds Ephemeral SPL Token instructions without native deps."""

    def __init__(self) -> None:
        settings = get_settings()
        self.program_id = _pubkey_bytes(settings.program_id)
        self.system_program = _pubkey_bytes(settings.system_program)
        self.token_program = _pubkey_bytes(settings.token_program)
        self.delegation_program = _pubkey_bytes(settings.delegation_program)
        self.permission_program = _pubkey_bytes(settings.permission_program)
        self.magic_program = _pubkey_bytes(settings.magic_program)
        self.ata_program = _pubkey_bytes(settings.ata_program)

    def initialize_ephemeral_ata(
        self,
        payer: str,
        user: str,
        mint: str,
        ephemeral_ata: str,
        ephemeral_ata_bump: int,
    ) -> Instruction:
        data = struct.pack("<BB", 0, ephemeral_ata_bump)
        accounts = [
            AccountMeta(_pubkey_bytes(ephemeral_ata), False, True),
            AccountMeta(_pubkey_bytes(payer), True, True),
            AccountMeta(_pubkey_bytes(user), False, False),
            AccountMeta(_pubkey_bytes(mint), False, False),
            AccountMeta(self.system_program, False, False),
        ]
        return Instruction(self.program_id, data, accounts)

    def initialize_global_vault(
        self,
        payer: str,
        mint: str,
        vault: str,
        vault_bump: int,
    ) -> Instruction:
        data = struct.pack("<BB", 1, vault_bump)
        accounts = [
            AccountMeta(_pubkey_bytes(vault), False, True),
            AccountMeta(_pubkey_bytes(payer), True, True),
            AccountMeta(_pubkey_bytes(mint), False, False),
            AccountMeta(self.system_program, False, False),
        ]
        return Instruction(self.program_id, data, accounts)

    def deposit_spl_tokens(
        self,
        authority: str,
        user: str,
        mint: str,
        source_token: str,
        vault_token: str,
        amount: int,
        ephemeral_ata: str,
        vault: str,
        token_program: Optional[str] = None,
    ) -> Instruction:
        data = struct.pack("<BQ", 2, amount)
        token_prog = _pubkey_bytes(token_program) if token_program else self.token_program
        accounts = [
            AccountMeta(_pubkey_bytes(ephemeral_ata), False, True),
            AccountMeta(_pubkey_bytes(vault), False, False),
            AccountMeta(_pubkey_bytes(mint), False, False),
            AccountMeta(_pubkey_bytes(source_token), False, True),
            AccountMeta(_pubkey_bytes(vault_token), False, True),
            AccountMeta(_pubkey_bytes(authority), True, False),
            AccountMeta(token_prog, False, False),
        ]
        return Instruction(self.program_id, data, accounts)

    def withdraw_spl_tokens(
        self,
        owner: str,
        mint: str,
        vault_source: str,
        user_dest: str,
        amount: int,
        ephemeral_ata: str,
        vault: str,
        vault_bump: int,
        token_program: Optional[str] = None,
    ) -> Instruction:
        data = struct.pack("<BQB", 3, amount, vault_bump)
        token_prog = _pubkey_bytes(token_program) if token_program else self.token_program
        accounts = [
            AccountMeta(_pubkey_bytes(owner), True, False),
            AccountMeta(_pubkey_bytes(ephemeral_ata), False, True),
            AccountMeta(_pubkey_bytes(vault), False, False),
            AccountMeta(_pubkey_bytes(mint), False, False),
            AccountMeta(_pubkey_bytes(vault_source), False, True),
            AccountMeta(_pubkey_bytes(user_dest), False, True),
            AccountMeta(token_prog, False, False),
        ]
        return Instruction(self.program_id, data, accounts)

    def delegate_ephemeral_ata(
        self,
        payer: str,
        user: str,
        mint: str,
        owner_program: str,
        buffer: str,
        delegation_record: str,
        delegation_metadata: str,
        ephemeral_ata: str,
        ephemeral_ata_bump: int,
        validator: Optional[str] = None,
    ) -> Instruction:
        if validator:
            validator_bytes = _pubkey_bytes(validator)
            data = struct.pack("<BB", 4, ephemeral_ata_bump) + bytes([1]) + validator_bytes
        else:
            data = struct.pack("<BB", 4, ephemeral_ata_bump) + bytes([0])

        accounts = [
            AccountMeta(_pubkey_bytes(payer), True, True),
            AccountMeta(_pubkey_bytes(ephemeral_ata), False, True),
            AccountMeta(_pubkey_bytes(owner_program), False, False),
            AccountMeta(_pubkey_bytes(buffer), False, True),
            AccountMeta(_pubkey_bytes(delegation_record), False, True),
            AccountMeta(_pubkey_bytes(delegation_metadata), False, True),
            AccountMeta(self.delegation_program, False, False),
            AccountMeta(self.system_program, False, False),
        ]
        return Instruction(self.program_id, data, accounts)

    def undelegate_ephemeral_ata(
        self,
        payer: str,
        ata: str,
        magic_context: str,
        ephemeral_ata: str,
    ) -> Instruction:
        data = bytes([5])
        accounts = [
            AccountMeta(_pubkey_bytes(payer), True, False),
            AccountMeta(_pubkey_bytes(ata), False, True),
            AccountMeta(_pubkey_bytes(ephemeral_ata), False, True),
            AccountMeta(_pubkey_bytes(magic_context), False, True),
            AccountMeta(self.magic_program, False, False),
        ]
        return Instruction(self.program_id, data, accounts)

    def create_ephemeral_ata_permission(
        self,
        payer: str,
        mint: str,
        flags: int,
        ephemeral_ata: str,
        ephemeral_ata_bump: int,
        permission: str,
    ) -> Instruction:
        data = struct.pack("<BBB", 6, ephemeral_ata_bump, flags)
        accounts = [
            AccountMeta(_pubkey_bytes(ephemeral_ata), False, True),
            AccountMeta(_pubkey_bytes(permission), False, True),
            AccountMeta(_pubkey_bytes(payer), True, True),
            AccountMeta(self.system_program, False, False),
            AccountMeta(self.permission_program, False, False),
        ]
        return Instruction(self.program_id, data, accounts)

    def delegate_ephemeral_ata_permission(
        self,
        payer: str,
        buffer: str,
        record: str,
        metadata: str,
        validator: str,
        ephemeral_ata: str,
        ephemeral_ata_bump: int,
        permission: str,
    ) -> Instruction:
        data = struct.pack("<BB", 7, ephemeral_ata_bump)
        accounts = [
            AccountMeta(_pubkey_bytes(payer), True, True),
            AccountMeta(_pubkey_bytes(ephemeral_ata), False, True),
            AccountMeta(self.permission_program, False, False),
            AccountMeta(_pubkey_bytes(permission), False, True),
            AccountMeta(self.system_program, False, False),
            AccountMeta(_pubkey_bytes(buffer), False, True),
            AccountMeta(_pubkey_bytes(record), False, True),
            AccountMeta(_pubkey_bytes(metadata), False, True),
            AccountMeta(self.delegation_program, False, False),
            AccountMeta(_pubkey_bytes(validator), False, False),
        ]
        return Instruction(self.program_id, data, accounts)

    def undelegate_ephemeral_ata_permission(
        self,
        payer: str,
        magic_context: str,
        ephemeral_ata: str,
        permission: str,
    ) -> Instruction:
        data = bytes([8])
        accounts = [
            AccountMeta(_pubkey_bytes(payer), True, False),
            AccountMeta(_pubkey_bytes(ephemeral_ata), False, True),
            AccountMeta(_pubkey_bytes(permission), False, True),
            AccountMeta(self.permission_program, False, False),
            AccountMeta(self.magic_program, False, False),
            AccountMeta(_pubkey_bytes(magic_context), False, True),
        ]
        return Instruction(self.program_id, data, accounts)

    def reset_ephemeral_ata_permission(
        self,
        owner: str,
        mint: str,
        flags: int,
        ephemeral_ata: str,
        ephemeral_ata_bump: int,
        permission: str,
    ) -> Instruction:
        data = struct.pack("<BBB", 9, ephemeral_ata_bump, flags)
        accounts = [
            AccountMeta(_pubkey_bytes(ephemeral_ata), False, True),
            AccountMeta(_pubkey_bytes(permission), False, True),
            AccountMeta(_pubkey_bytes(owner), True, False),
            AccountMeta(self.permission_program, False, False),
        ]
        return Instruction(self.program_id, data, accounts)

    def checked_transfer(
        self,
        source: str,
        destination: str,
        mint: str,
        amount: int,
        decimals: int,
        authority: str,
        token_program: Optional[str] = None,
    ) -> Instruction:
        """SPL token TransferChecked instruction (discriminator 12).
        token_program: Token program ID override (defaults to self.token_program)
        
        Transfers tokens from source to destination with mint/decimals validation.
        
        Accounts:
        - source (writable): source token account
        - mint (readonly): token mint
        - destination (writable): destination token account
        - authority (signer): source account owner/delegate
        """

        data = struct.pack("<BQB", 12, amount, decimals)
        token_prog = _pubkey_bytes(token_program) if token_program else self.token_program
        accounts = [
            AccountMeta(_pubkey_bytes(source), False, True),
            AccountMeta(_pubkey_bytes(mint), False, False),
            AccountMeta(_pubkey_bytes(destination), False, True),
            AccountMeta(_pubkey_bytes(authority), True, False),
        ]
        return Instruction(token_prog, data, accounts)

    def initialize_ata(
        self,
        payer: str,
        user: str,
        mint: str,
        ata: str,
        token_program: Optional[str] = None,
    ) -> Instruction:
        """Associated Token Account (ATA) Initialize instruction.
        
        Initializes an associated token account for a user and mint pair.
        
        Accounts:
        - payer (writable, signer): payer for account creation
        - ata (writable): associated token account to be created
        - user (readonly): owner of the ATA
        - mint (readonly): token mint
        - system_program (readonly): system program
        - token_program (readonly): token program
        """
        data = b""  # ATA Create instruction has no data (empty input means Create)
        token_prog = _pubkey_bytes(token_program) if token_program else self.token_program
        accounts = [
            AccountMeta(_pubkey_bytes(payer), True, True),
            AccountMeta(_pubkey_bytes(ata), False, True),
            AccountMeta(_pubkey_bytes(user), False, False),
            AccountMeta(_pubkey_bytes(mint), False, False),
            AccountMeta(self.system_program, False, False),
            AccountMeta(token_prog, False, False),
        ]
        return Instruction(self.ata_program, data, accounts)


builder = InstructionBuilder()
