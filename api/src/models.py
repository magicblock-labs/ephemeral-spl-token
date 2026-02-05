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
    token_program: Optional[str] = Field(None, description="Token program ID override (defaults to SPL Token)")


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


class CheckedTransferRequest(ClusterConfig):
    """Transfer SPL tokens with checked mint and decimals (TransferChecked)."""
    source: str = Field(..., description="Source token account (writable)")
    destination: str = Field(..., description="Destination token account (writable)")
    mint: str = Field(..., description="Token mint (readonly)")
    authority: str = Field(..., description="Authority/owner of source token account (signer)")
    amount: int = Field(..., description="Amount of tokens to transfer", ge=0)
    decimals: int = Field(..., description="Expected token decimals", ge=0, le=18)
    token_program: Optional[str] = Field(None, description="Token program ID override (defaults to SPL Token)")


class InitializeAtaRequest(ClusterConfig):
    """Initialize an Associated Token Account for a user-mint pair."""
    payer: str = Field(..., description="Payer pubkey (signer)")
    user: str = Field(..., description="Owner of the ATA")
    mint: str = Field(..., description="SPL token mint")
    ata: str = Field(..., description="Associated Token Account address")
    token_program: Optional[str] = Field(None, description="Token program ID override (defaults to SPL Token)")


class DepositPrivateBalanceRequest(ClusterConfig):
    """Deposit SPL tokens and initialize necessary accounts in a single transaction.
    
    Note: All account addresses are automatically derived from user, mint, and ephemeral_spl_token_program:
    - source_token: Derived as ATA from [user, token_program, mint]
    - vault, ephemeral_ata, user_ata, vault_ata: Derived as PDAs
    - Delegation PDAs: Auto-derived from ephemeral_ata and ephemeral_spl_token_program
    - user is also the authority (payer/signer)
    """
    user: str = Field(..., description="User pubkey (owner of ephemeral ATA, source token account, and payer/signer)")
    mint: str = Field(..., description="SPL token mint")
    amount: int = Field(..., description="Amount of tokens to deposit", ge=0)
    # Optional overrides
    validator: Optional[str] = Field(None, description="Optional validator pubkey for delegation")
    ephemeral_spl_token_program: Optional[str] = Field(None, description="Ephemeral SPL Token Program ID override (defaults from config)")
    token_program: Optional[str] = Field(None, description="Token program ID override (defaults to SPL Token)")
    permission_program: Optional[str] = Field(None, description="Permission program ID override (defaults from config)")
    delegation_program: Optional[str] = Field(None, description="Delegation program ID override (defaults from config)")