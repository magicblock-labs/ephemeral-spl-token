"""Pydantic models for API requests and responses."""

from pydantic import BaseModel, Field
from typing import Optional


class TransactionResponse(BaseModel):
    transaction: str = Field(..., description="Base64-encoded serialized transaction")
    message: str = Field(default="Transaction created successfully")


class ClusterConfig(BaseModel):
    endpoint_url: Optional[str] = Field(None, description="Solana endpoint URL override")


# === Instruction Request Models ===

class InitializeEphemeralAtaRequest(ClusterConfig):
    payer: str = Field(..., description="Payer pubkey (signer)")
    owner: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")


class InitializeGlobalVaultRequest(ClusterConfig):
    payer: str = Field(..., description="Payer pubkey (signer)")
    mint: str = Field(..., description="SPL token mint")


class DepositSplTokensRequest(ClusterConfig):
    owner: str = Field(..., description="Owner of source token account (payer and signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    amount: int = Field(..., description="Amount of tokens to deposit", ge=0)


class WithdrawRequest(BaseModel):
    owner: str = Field(..., description="Owner of ephemeral ATA (payer and signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    amount: int = Field(..., description="Amount of tokens to withdraw", ge=0)
    endpoint_url: Optional[str] = Field(None, description="Solana endpoint URL override")


class DelegateEphemeralAtaRequest(ClusterConfig):
    payer: str = Field(..., description="Payer pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    owner_program: str = Field(..., description="Program that will own the delegated account")
    validator: Optional[str] = Field(None, description="Optional validator pubkey")


class UndelegateEphemeralAtaRequest(ClusterConfig):
    payer: str = Field(..., description="Payer/owner pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")


class CreateEphemeralAtaPermissionRequest(ClusterConfig):
    payer: str = Field(..., description="Payer pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    flags: int = Field(..., description="Permission flags (MemberFlags bitfield)", ge=0, le=255)


class DelegateEphemeralAtaPermissionRequest(ClusterConfig):
    payer: str = Field(..., description="Payer pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    validator: str = Field(..., description="Validator to restrict delegation")


class UndelegateEphemeralAtaPermissionRequest(ClusterConfig):
    payer: str = Field(..., description="Payer/owner pubkey (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")


class ResetEphemeralAtaPermissionRequest(ClusterConfig):
    owner: str = Field(..., description="Owner of ephemeral ATA (signer)")
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    flags: int = Field(..., description="New permission flags", ge=0, le=255)


class TransferAmountRequest(BaseModel):
    sender: str = Field(..., description="Sender pubkey (signer, owner of source ATA)")
    recipient: str = Field(..., description="Recipient pubkey (owner of destination ATA)")
    mint: str = Field(..., description="Token mint (readonly)")
    amount: int = Field(..., description="Amount of tokens to transfer", ge=0)
    endpoint_url: Optional[str] = Field(None, description="Solana endpoint URL override")


class InitializeAtaRequest(ClusterConfig):
    payer: str = Field(..., description="Payer pubkey (signer)")
    owner: str = Field(..., description="Owner of the ATA")
    mint: str = Field(..., description="SPL token mint")


class DepositRequest(BaseModel):
    user: str = Field(..., description="User pubkey (owner of ephemeral ATA, source token account, and payer/signer)")
    mint: str = Field(..., description="SPL token mint")
    amount: int = Field(default=0, description="Amount of tokens to deposit (defaults to 0 for initialization only)", ge=0)
    validator: Optional[str] = Field(None, description="Optional validator pubkey (defaults to default_validator from settings)")
    endpoint_url: Optional[str] = Field(None, description="Solana endpoint URL override")


class PrepareWithdrawalRequest(BaseModel):
    user: str = Field(..., description="Owner of the ephemeral ATA")
    mint: str = Field(..., description="SPL token mint")
    endpoint_url: Optional[str] = Field(None, description="Solana endpoint URL override")

