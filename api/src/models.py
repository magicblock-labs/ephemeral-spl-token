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
    ephemeral_ata: str = Field(..., description="Ephemeral ATA PDA")
    ephemeral_ata_bump: int = Field(..., description="Ephemeral ATA bump", ge=0, le=255)


class InitializeGlobalVaultRequest(ClusterConfig):
    """Initialize a Global Vault for a specific mint."""
    payer: str = Field(..., description="Payer pubkey (signer)")
    mint: str = Field(..., description="SPL token mint")
    vault: str = Field(..., description="Global vault PDA")
    vault_bump: int = Field(..., description="Global vault bump", ge=0, le=255)


class DepositSplTokensRequest(ClusterConfig):
    """Deposit SPL tokens into an ephemeral ATA."""
    authority: str = Field(..., description="Authority over source token account (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    source_token: str = Field(..., description="User's source token account")
    vault_token: str = Field(..., description="Vault's token account")
    amount: int = Field(..., description="Amount of tokens to deposit", ge=0)
    ephemeral_ata: str = Field(..., description="Ephemeral ATA PDA")
    vault: str = Field(..., description="Global vault PDA")


class WithdrawSplTokensRequest(ClusterConfig):
    """Withdraw SPL tokens from an ephemeral ATA."""
    owner: str = Field(..., description="Owner of ephemeral ATA (signer)")
    mint: str = Field(..., description="SPL token mint")
    vault_source: str = Field(..., description="Vault's source token account")
    user_dest: str = Field(..., description="User's destination token account")
    amount: int = Field(..., description="Amount of tokens to withdraw", ge=0)
    ephemeral_ata: str = Field(..., description="Ephemeral ATA PDA")
    vault: str = Field(..., description="Global vault PDA")
    vault_bump: int = Field(..., description="Global vault bump", ge=0, le=255)


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
    ephemeral_ata: str = Field(..., description="Ephemeral ATA PDA")
    ephemeral_ata_bump: int = Field(..., description="Ephemeral ATA bump", ge=0, le=255)


class UndelegateEphemeralAtaRequest(ClusterConfig):
    """Undelegate an ephemeral ATA from a DLP."""
    payer: str = Field(..., description="Payer/owner pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    ata: str = Field(..., description="User's SPL token account")
    magic_context: str = Field(..., description="Magic context account")
    ephemeral_ata: str = Field(..., description="Ephemeral ATA PDA")


class CreateEphemeralAtaPermissionRequest(ClusterConfig):
    """Create a permission account for an ephemeral ATA."""
    payer: str = Field(..., description="Payer pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    flags: int = Field(..., description="Permission flags (MemberFlags bitfield)", ge=0, le=255)
    ephemeral_ata: str = Field(..., description="Ephemeral ATA PDA")
    ephemeral_ata_bump: int = Field(..., description="Ephemeral ATA bump", ge=0, le=255)
    permission: str = Field(..., description="Permission PDA")


class DelegateEphemeralAtaPermissionRequest(ClusterConfig):
    """Delegate an ephemeral ATA's permission to a DLP."""
    payer: str = Field(..., description="Payer pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    buffer: str = Field(..., description="Delegation buffer account")
    record: str = Field(..., description="Delegation record account")
    metadata: str = Field(..., description="Delegation metadata account")
    validator: str = Field(..., description="Validator to restrict delegation")
    ephemeral_ata: str = Field(..., description="Ephemeral ATA PDA")
    ephemeral_ata_bump: int = Field(..., description="Ephemeral ATA bump", ge=0, le=255)
    permission: str = Field(..., description="Permission PDA")


class UndelegateEphemeralAtaPermissionRequest(ClusterConfig):
    """Undelegate an ephemeral ATA's permission from a DLP."""
    payer: str = Field(..., description="Payer/owner pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    magic_context: str = Field(..., description="Magic context account")
    ephemeral_ata: str = Field(..., description="Ephemeral ATA PDA")
    permission: str = Field(..., description="Permission PDA")


class ResetEphemeralAtaPermissionRequest(ClusterConfig):
    """Reset permission flags on an ephemeral ATA's permission."""
    owner: str = Field(..., description="Owner of ephemeral ATA (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    flags: int = Field(..., description="New permission flags", ge=0, le=255)
    ephemeral_ata: str = Field(..., description="Ephemeral ATA PDA")
    ephemeral_ata_bump: int = Field(..., description="Ephemeral ATA bump", ge=0, le=255)
    permission: str = Field(..., description="Permission PDA")
