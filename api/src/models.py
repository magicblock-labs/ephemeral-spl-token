"""Pydantic models for API requests and responses."""

from pydantic import BaseModel, Field
from typing import Optional


class TransactionResponse(BaseModel):
    """Response containing a serialized transaction."""
    transaction: str = Field(..., description="Base64-encoded serialized transaction")
    message: str = Field(default="Transaction created successfully")


class ClusterConfig(BaseModel):
    """Optional endpoint configuration override."""
    endpoint_url: Optional[str] = Field(None, description="Solana endpoint URL override")


# === Instruction Request Models ===

class InitializeEphemeralAtaRequest(ClusterConfig):
    """Initialize an Ephemeral ATA for a user-mint pair.
    
    All account addresses are automatically derived from user and mint.
    User is the payer and signer.
    """
    payer: str = Field(..., description="Payer pubkey (signer)")
    owner: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")


class InitializeGlobalVaultRequest(ClusterConfig):
    """Initialize a Global Vault for a specific mint.
    
    All account addresses are automatically derived from mint.
    Payer is the caller/user.
    """
    payer: str = Field(..., description="Payer pubkey (signer)")
    mint: str = Field(..., description="SPL token mint")


class DepositSplTokensRequest(ClusterConfig):
    """Deposit SPL tokens into an ephemeral ATA.
    
    All account addresses are automatically derived from user and mint.
    Owner is the signer (source token account owner).
    """
    owner: str = Field(..., description="Owner of source token account (payer and signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    amount: int = Field(..., description="Amount of tokens to deposit", ge=0)


class WithdrawRequest(ClusterConfig):
    """Withdraw SPL tokens from an ephemeral ATA.
    
    All account addresses are automatically derived from user and mint.
    Owner is the signer.
    """
    owner: str = Field(..., description="Owner of ephemeral ATA (payer and signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    amount: int = Field(..., description="Amount of tokens to withdraw", ge=0)


class DelegateEphemeralAtaRequest(ClusterConfig):
    """Delegate an ephemeral ATA to a DLP.
    
    All account addresses are automatically derived from user and mint.
    Payer is the signer.
    """
    payer: str = Field(..., description="Payer pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    owner_program: str = Field(..., description="Program that will own the delegated account")
    validator: Optional[str] = Field(None, description="Optional validator pubkey")


class UndelegateEphemeralAtaRequest(ClusterConfig):
    """Undelegate an ephemeral ATA from a DLP.
    
    All account addresses are automatically derived from user and mint.
    Magic context is automatically provided from settings.
    Payer/owner is the signer.
    """
    payer: str = Field(..., description="Payer/owner pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")


class CreateEphemeralAtaPermissionRequest(ClusterConfig):
    """Create a permission account for an ephemeral ATA.
    
    All account addresses are automatically derived from user and mint.
    Payer is the signer.
    """
    payer: str = Field(..., description="Payer pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    flags: int = Field(..., description="Permission flags (MemberFlags bitfield)", ge=0, le=255)


class DelegateEphemeralAtaPermissionRequest(ClusterConfig):
    """Delegate an ephemeral ATA's permission to a DLP.
    
    All account addresses are automatically derived from user and mint.
    Payer is the signer.
    """
    payer: str = Field(..., description="Payer pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    validator: str = Field(..., description="Validator to restrict delegation")


class UndelegateEphemeralAtaPermissionRequest(ClusterConfig):
    """Undelegate an ephemeral ATA's permission from a DLP.
    
    All account addresses are automatically derived from user and mint.
    Magic context is automatically provided from settings.
    Payer/owner is the signer.
    """
    payer: str = Field(..., description="Payer/owner pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")


class ResetEphemeralAtaPermissionRequest(ClusterConfig):
    """Reset permission flags on an ephemeral ATA's permission.
    
    All account addresses are automatically derived from user and mint.
    Owner is the signer.
    """
    owner: str = Field(..., description="Owner of ephemeral ATA (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    flags: int = Field(..., description="New permission flags", ge=0, le=255)


class TransferAmountRequest(ClusterConfig):
    """Transfer SPL tokens with checked mint and decimals (TransferChecked)."""
    sender: str = Field(..., description="Sender pubkey (signer, owner of source ATA)")
    recipient: str = Field(..., description="Recipient pubkey (owner of destination ATA)")
    mint: str = Field(..., description="Token mint (readonly)")
    amount: int = Field(..., description="Amount of tokens to transfer", ge=0)


class InitializeAtaRequest(ClusterConfig):
    """Initialize an Associated Token Account for a owner-mint pair."""
    payer: str = Field(..., description="Payer pubkey (signer)")
    owner: str = Field(..., description="Owner of the ATA")
    mint: str = Field(..., description="SPL token mint")


class DepositRequest(ClusterConfig):
    """Initialize necessary accounts and/or deposit SPL tokens to private account in a single transaction.
    
    This endpoint:
    - Derives all accounts from user and mint
    - Checks which accounts need initialization
    - Creates only necessary initialization instructions for uninitialized accounts
    - Creates delegation instructions for accounts that exist but are not delegated
    - Deposits tokens if amount > 0
    
    All account addresses are automatically derived from user, mint, and program IDs:
    - vault, vault_ata, ephemeral_ata, user_ata, permission
    - Delegation PDAs for ephemeral_ata and permission
    - user is also the authority (payer/signer)
    
    Program IDs are optional and default to values in config.py. 
    They can be overridden by including them in the request body.
    """
    user: str = Field(..., description="User pubkey (owner of ephemeral ATA, source token account, and payer/signer)")
    mint: str = Field(..., description="SPL token mint")
    amount: int = Field(default=0, description="Amount of tokens to deposit (defaults to 0 for initialization only)", ge=0)


class PrepareWithdrawalRequest(ClusterConfig):
    """Undelegate an ephemeral ATA from a DLP.
    
    All account addresses are automatically derived from user and mint.
    Magic context is automatically provided from settings.
    Payer/owner is the signer.
    """
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")

