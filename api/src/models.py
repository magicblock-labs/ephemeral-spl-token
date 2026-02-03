"""Pydantic models for API requests and responses."""

from pydantic import BaseModel, Field
from typing import Optional


class TransactionResponse(BaseModel):
    """Response containing a serialized transaction."""
    transaction: str = Field(..., description="Base64-encoded serialized transaction")
    message: str = Field(default="Transaction created successfully")


class ClusterConfig(BaseModel):
    """Optional cluster configuration override."""
    cluster_url: Optional[str] = Field(None, description="Solana cluster URL override")


# === Instruction Request Models ===

class InitializeEphemeralAtaRequest(ClusterConfig):
    """Initialize an Ephemeral ATA for a user-mint pair."""
    payer: str = Field(..., description="Payer pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    ephemeral_ata: Optional[str] = Field(None, description="Ephemeral ATA PDA (derived if not provided)")
    ephemeral_ata_bump: Optional[int] = Field(None, description="Ephemeral ATA bump (derived if not provided)", ge=0, le=255)


class InitializeGlobalVaultRequest(ClusterConfig):
    """Initialize a Global Vault for a specific mint."""
    payer: str = Field(..., description="Payer pubkey (signer)")
    mint: str = Field(..., description="SPL token mint")
    vault: Optional[str] = Field(None, description="Global vault PDA (derived if not provided)")
    vault_bump: Optional[int] = Field(None, description="Global vault bump (derived if not provided)", ge=0, le=255)


class DepositSplTokensRequest(ClusterConfig):
    """Deposit SPL tokens into an ephemeral ATA."""
    authority: str = Field(..., description="Authority over source token account (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    source_token: Optional[str] = Field(None, description="User's source token account (derived from authority if not provided)")
    vault_token: Optional[str] = Field(None, description="Vault's token account (derived if not provided)")
    amount: int = Field(..., description="Amount of tokens to deposit", ge=0)
    ephemeral_ata: Optional[str] = Field(None, description="Ephemeral ATA PDA (derived if not provided)")
    vault: Optional[str] = Field(None, description="Global vault PDA (derived if not provided)")
    token_program: Optional[str] = Field(None, description="Token program ID override (defaults to SPL Token)")


class WithdrawSplTokensRequest(ClusterConfig):
    """Withdraw SPL tokens from an ephemeral ATA."""
    owner: str = Field(..., description="Owner of ephemeral ATA (signer)")
    mint: str = Field(..., description="SPL token mint")
    vault_source: Optional[str] = Field(None, description="Vault's source token account (derived if not provided)")
    user_dest: Optional[str] = Field(None, description="User's destination token account (derived from owner if not provided)")
    amount: int = Field(..., description="Amount of tokens to withdraw", ge=0)
    ephemeral_ata: Optional[str] = Field(None, description="Ephemeral ATA PDA (derived if not provided)")
    vault: Optional[str] = Field(None, description="Global vault PDA (derived if not provided)")
    vault_bump: Optional[int] = Field(None, description="Global vault bump (derived if not provided)", ge=0, le=255)
    token_program: Optional[str] = Field(None, description="Token program ID override (defaults to SPL Token)")


class DelegateEphemeralAtaRequest(ClusterConfig):
    """Delegate an ephemeral ATA to a DLP."""
    payer: str = Field(..., description="Payer pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    owner_program: str = Field(..., description="Program that will own the delegated account")
    buffer: str = Field(..., description="Delegation buffer account")
    delegation_record: str = Field(..., description="Delegation record account")
    delegation_metadata: str = Field(..., description="Delegation metadata account")
    validator: Optional[str] = Field(None, description="Optional validator pubkey")
    ephemeral_ata: Optional[str] = Field(None, description="Ephemeral ATA PDA (derived if not provided)")
    ephemeral_ata_bump: Optional[int] = Field(None, description="Ephemeral ATA bump (derived if not provided)", ge=0, le=255)


class UndelegateEphemeralAtaRequest(ClusterConfig):
    """Undelegate an ephemeral ATA from a DLP."""
    payer: str = Field(..., description="Payer/owner pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    ata: Optional[str] = Field(None, description="User's SPL token account (derived if not provided)")
    magic_context: str = Field(..., description="Magic context account")
    ephemeral_ata: Optional[str] = Field(None, description="Ephemeral ATA PDA (derived if not provided)")


class CreateEphemeralAtaPermissionRequest(ClusterConfig):
    """Create a permission account for an ephemeral ATA."""
    payer: str = Field(..., description="Payer pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    flags: int = Field(..., description="Permission flags (MemberFlags bitfield)", ge=0, le=255)
    ephemeral_ata: Optional[str] = Field(None, description="Ephemeral ATA PDA (derived if not provided)")
    ephemeral_ata_bump: Optional[int] = Field(None, description="Ephemeral ATA bump (derived if not provided)", ge=0, le=255)
    permission: Optional[str] = Field(None, description="Permission PDA (derived if not provided)")


class DelegateEphemeralAtaPermissionRequest(ClusterConfig):
    """Delegate an ephemeral ATA's permission to a DLP."""
    payer: str = Field(..., description="Payer pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    buffer: str = Field(..., description="Delegation buffer account")
    record: str = Field(..., description="Delegation record account")
    metadata: str = Field(..., description="Delegation metadata account")
    validator: str = Field(..., description="Validator to restrict delegation")
    ephemeral_ata: Optional[str] = Field(None, description="Ephemeral ATA PDA (derived if not provided)")
    ephemeral_ata_bump: Optional[int] = Field(None, description="Ephemeral ATA bump (derived if not provided)", ge=0, le=255)
    permission: Optional[str] = Field(None, description="Permission PDA (derived if not provided)")


class UndelegateEphemeralAtaPermissionRequest(ClusterConfig):
    """Undelegate an ephemeral ATA's permission from a DLP."""
    payer: str = Field(..., description="Payer/owner pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    magic_context: str = Field(..., description="Magic context account")
    ephemeral_ata: Optional[str] = Field(None, description="Ephemeral ATA PDA (derived if not provided)")
    permission: Optional[str] = Field(None, description="Permission PDA (derived if not provided)")


class ResetEphemeralAtaPermissionRequest(ClusterConfig):
    """Reset permission flags on an ephemeral ATA's permission."""
    owner: str = Field(..., description="Owner of ephemeral ATA (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    flags: int = Field(..., description="New permission flags", ge=0, le=255)
    ephemeral_ata: Optional[str] = Field(None, description="Ephemeral ATA PDA (derived if not provided)")
    ephemeral_ata_bump: Optional[int] = Field(None, description="Ephemeral ATA bump (derived if not provided)", ge=0, le=255)
    permission: Optional[str] = Field(None, description="Permission PDA (derived if not provided)")


class CheckedTransferRequest(ClusterConfig):
    """Transfer SPL tokens with checked mint and decimals (TransferChecked)."""
    source: str = Field(..., description="Source token account (writable)")
    destination: str = Field(..., description="Destination token account (writable)")
    mint: str = Field(..., description="Token mint (readonly)")
    authority: str = Field(..., description="Authority/owner of source token account (signer)")
    amount: int = Field(..., description="Amount of tokens to transfer", ge=0)
    decimals: Optional[int] = Field(None, description="Expected token decimals (fetched from mint if not provided)", ge=0, le=18)
    token_program: Optional[str] = Field(None, description="Token program ID override (defaults to SPL Token)")


class InitializeAtaRequest(ClusterConfig):
    """Initialize an Associated Token Account for a user-mint pair."""
    payer: str = Field(..., description="Payer pubkey (signer)")
    user: str = Field(..., description="Owner of the ATA")
    mint: str = Field(..., description="SPL token mint")
    ata: Optional[str] = Field(None, description="Associated Token Account address (derived if not provided)")
    token_program: Optional[str] = Field(None, description="Token program ID override (defaults to SPL Token)")