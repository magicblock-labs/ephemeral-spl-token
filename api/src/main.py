"""
Ephemeral SPL Token API

FastAPI service for building Solana transactions for the Ephemeral SPL Token program.
Returns serialized transactions ready to be signed and submitted.
"""

from contextlib import asynccontextmanager

_app = None


def create_app():
    from fastapi import FastAPI, HTTPException

    from .config import get_settings, Settings
    from .models import (
        TransactionResponse,
        InitializeEphemeralAtaRequest,
        InitializeGlobalVaultRequest,
        DepositSplTokensRequest,
        WithdrawSplTokensRequest,
        DelegateEphemeralAtaRequest,
        UndelegateEphemeralAtaRequest,
        CreateEphemeralAtaPermissionRequest,
        DelegateEphemeralAtaPermissionRequest,
        UndelegateEphemeralAtaPermissionRequest,
        ResetEphemeralAtaPermissionRequest,
        CheckedTransferRequest,
        InitializeAtaRequest,
    )
    from .builder import builder, serialize_transaction

    @asynccontextmanager
    async def lifespan(app: FastAPI):
        yield

    app = FastAPI(
        title="Private Ephemeral SPL Token API",
        description="Build transactions for the MagicBlock Private token program (https://docs.magicblock.gg/pages/private-ephemeral-rollups-pers)",
        version="0.1.0",
        lifespan=lifespan,
    )

    @app.get("/", tags=["Health"])
    async def root():
        settings = get_settings()
        return {
            "status": "ok",
            "program_id": settings.program_id,
            "cluster_url": settings.cluster_url,
        }

    @app.get("/config", response_model=Settings, tags=["Config"])
    async def get_config():
        return get_settings()

    @app.post("/tx/initialize-ephemeral-ata", response_model=TransactionResponse, tags=["Transactions"])
    async def initialize_ephemeral_ata(req: InitializeEphemeralAtaRequest):
        try:
            ix = builder.initialize_ephemeral_ata(
                req.payer,
                req.user,
                req.mint,
                req.ephemeral_ata,
                req.ephemeral_ata_bump,
            )
            tx = await serialize_transaction(ix, req.payer, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/initialize-global-vault", response_model=TransactionResponse, tags=["Transactions"])
    async def initialize_global_vault(req: InitializeGlobalVaultRequest):
        try:
            ix = builder.initialize_global_vault(
                req.payer,
                req.mint,
                req.vault,
                req.vault_bump,
            )
            tx = await serialize_transaction(ix, req.payer, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/deposit-spl-tokens", response_model=TransactionResponse, tags=["Transactions"])
    async def deposit_spl_tokens(req: DepositSplTokensRequest):
        try:
            ix = builder.deposit_spl_tokens(
                req.authority,
                req.user,
                req.mint,
                req.source_token,
                req.vault_token,
                req.amount,
                req.ephemeral_ata,
                req.vault,
                req.token_program,
            )
            tx = await serialize_transaction(ix, req.authority, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/withdraw-spl-tokens", response_model=TransactionResponse, tags=["Transactions"])
    async def withdraw_spl_tokens(req: WithdrawSplTokensRequest):
        try:
            ix = builder.withdraw_spl_tokens(
                req.owner,
                req.mint,
                req.vault_source,
                req.user_dest,
                req.amount,
                req.ephemeral_ata,
                req.vault,
                req.vault_bump,
                req.token_program,
            )
            tx = await serialize_transaction(ix, req.owner, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/delegate-ephemeral-ata", response_model=TransactionResponse, tags=["Transactions"])
    async def delegate_ephemeral_ata(req: DelegateEphemeralAtaRequest):
        try:
            ix = builder.delegate_ephemeral_ata(
                req.payer,
                req.user,
                req.mint,
                req.owner_program,
                req.buffer,
                req.delegation_record,
                req.delegation_metadata,
                req.ephemeral_ata,
                req.ephemeral_ata_bump,
                req.validator,
            )
            tx = await serialize_transaction(ix, req.payer, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/undelegate-ephemeral-ata", response_model=TransactionResponse, tags=["Transactions"])
    async def undelegate_ephemeral_ata(req: UndelegateEphemeralAtaRequest):
        try:
            ix = builder.undelegate_ephemeral_ata(
                req.payer,
                req.ata,
                req.magic_context,
                req.ephemeral_ata,
            )
            tx = await serialize_transaction(ix, req.payer, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/create-ephemeral-ata-permission", response_model=TransactionResponse, tags=["Transactions"])
    async def create_ephemeral_ata_permission(req: CreateEphemeralAtaPermissionRequest):
        try:
            ix = builder.create_ephemeral_ata_permission(
                req.payer,
                req.mint,
                req.flags,
                req.ephemeral_ata,
                req.ephemeral_ata_bump,
                req.permission,
            )
            tx = await serialize_transaction(ix, req.payer, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/delegate-ephemeral-ata-permission", response_model=TransactionResponse, tags=["Transactions"])
    async def delegate_ephemeral_ata_permission(req: DelegateEphemeralAtaPermissionRequest):
        try:
            ix = builder.delegate_ephemeral_ata_permission(
                req.payer,
                req.buffer,
                req.record,
                req.metadata,
                req.validator,
                req.ephemeral_ata,
                req.ephemeral_ata_bump,
                req.permission,
            )
            tx = await serialize_transaction(ix, req.payer, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/undelegate-ephemeral-ata-permission", response_model=TransactionResponse, tags=["Transactions"])
    async def undelegate_ephemeral_ata_permission(req: UndelegateEphemeralAtaPermissionRequest):
        try:
            ix = builder.undelegate_ephemeral_ata_permission(
                req.payer,
                req.magic_context,
                req.ephemeral_ata,
                req.permission,
            )
            tx = await serialize_transaction(ix, req.payer, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/reset-ephemeral-ata-permission", response_model=TransactionResponse, tags=["Transactions"])
    async def reset_ephemeral_ata_permission(req: ResetEphemeralAtaPermissionRequest):
        try:
            ix = builder.reset_ephemeral_ata_permission(
                req.owner,
                req.mint,
                req.flags,
                req.ephemeral_ata,
                req.ephemeral_ata_bump,
                req.permission,
            )
            tx = await serialize_transaction(ix, req.owner, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/checked-transfer", response_model=TransactionResponse, tags=["Transactions"])
    async def checked_transfer(req: CheckedTransferRequest):
        try:
            ix = builder.checked_transfer(
                req.source,
                req.destination,
                req.mint,
                req.amount,
                req.decimals,
                req.authority,
                req.token_program,
            )
            tx = await serialize_transaction(ix, req.authority, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/initialize-ata", response_model=TransactionResponse, tags=["Transactions"])
    async def initialize_ata(req: InitializeAtaRequest):
        try:
            ix = builder.initialize_ata(
                req.payer,
                req.user,
                req.mint,
                req.ata,
                req.token_program,
            )
            tx = await serialize_transaction(ix, req.payer, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    return app


def _get_app():
    global _app
    if _app is None:
        _app = create_app()
    return _app


# Cloudflare Workers entrypoint - only loaded in Workers environment
try:
    from workers import WorkerEntrypoint

    class Default(WorkerEntrypoint):
        async def on_fetch(self, request):
            import asgi
            return await asgi.fetch(_get_app(), request, self.env)
except ImportError:
    pass  # Not running in Cloudflare Workers environment
