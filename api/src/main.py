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
    from .builder import (
        builder,
        serialize_transaction,
        derive_ephemeral_ata,
        derive_vault,
        derive_permission,
        derive_ata,
        _pubkey_bytes,
    )

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
            settings = get_settings()
            # Derive ephemeral_ata and bump if not provided
            if req.ephemeral_ata is None or req.ephemeral_ata_bump is None:
                ata, bump = derive_ephemeral_ata(req.user, req.mint, _pubkey_bytes(settings.program_id))
                ephemeral_ata = req.ephemeral_ata or ata
                ephemeral_ata_bump = req.ephemeral_ata_bump if req.ephemeral_ata_bump is not None else bump
            else:
                ephemeral_ata = req.ephemeral_ata
                ephemeral_ata_bump = req.ephemeral_ata_bump

            ix = builder.initialize_ephemeral_ata(
                req.payer,
                req.user,
                req.mint,
                ephemeral_ata,
                ephemeral_ata_bump,
            )
            tx = await serialize_transaction(ix, req.payer, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/initialize-global-vault", response_model=TransactionResponse, tags=["Transactions"])
    async def initialize_global_vault(req: InitializeGlobalVaultRequest):
        try:
            settings = get_settings()
            # Derive vault and bump if not provided
            if req.vault is None or req.vault_bump is None:
                v, bump = derive_vault(req.mint, _pubkey_bytes(settings.program_id))
                vault = req.vault or v
                vault_bump = req.vault_bump if req.vault_bump is not None else bump
            else:
                vault = req.vault
                vault_bump = req.vault_bump

            ix = builder.initialize_global_vault(
                req.payer,
                req.mint,
                vault,
                vault_bump,
            )
            tx = await serialize_transaction(ix, req.payer, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/deposit-spl-tokens", response_model=TransactionResponse, tags=["Transactions"])
    async def deposit_spl_tokens(req: DepositSplTokensRequest):
        try:
            settings = get_settings()
            program_id = _pubkey_bytes(settings.program_id)
            token_prog = _pubkey_bytes(req.token_program or settings.token_program)
            ata_prog = _pubkey_bytes(settings.ata_program)

            # Derive ephemeral_ata if not provided
            if req.ephemeral_ata is None:
                ephemeral_ata, _ = derive_ephemeral_ata(req.user, req.mint, program_id)
            else:
                ephemeral_ata = req.ephemeral_ata

            # Derive vault if not provided
            if req.vault is None:
                vault, _ = derive_vault(req.mint, program_id)
            else:
                vault = req.vault

            # Derive source_token (authority's ATA) if not provided
            if req.source_token is None:
                source_token = derive_ata(req.authority, req.mint, token_prog, ata_prog)
            else:
                source_token = req.source_token

            # Derive vault_token (vault's ATA) if not provided
            if req.vault_token is None:
                vault_token = derive_ata(vault, req.mint, token_prog, ata_prog)
            else:
                vault_token = req.vault_token

            ix = builder.deposit_spl_tokens(
                req.authority,
                req.user,
                req.mint,
                source_token,
                vault_token,
                req.amount,
                ephemeral_ata,
                vault,
                req.token_program,
            )
            tx = await serialize_transaction(ix, req.authority, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/withdraw-spl-tokens", response_model=TransactionResponse, tags=["Transactions"])
    async def withdraw_spl_tokens(req: WithdrawSplTokensRequest):
        try:
            settings = get_settings()
            program_id = _pubkey_bytes(settings.program_id)
            token_prog = _pubkey_bytes(req.token_program or settings.token_program)
            ata_prog = _pubkey_bytes(settings.ata_program)

            # Derive ephemeral_ata if not provided
            if req.ephemeral_ata is None:
                ephemeral_ata, _ = derive_ephemeral_ata(req.owner, req.mint, program_id)
            else:
                ephemeral_ata = req.ephemeral_ata

            # Derive vault and bump if not provided
            if req.vault is None or req.vault_bump is None:
                v, bump = derive_vault(req.mint, program_id)
                vault = req.vault or v
                vault_bump = req.vault_bump if req.vault_bump is not None else bump
            else:
                vault = req.vault
                vault_bump = req.vault_bump

            # Derive vault_source (vault's ATA) if not provided
            if req.vault_source is None:
                vault_source = derive_ata(vault, req.mint, token_prog, ata_prog)
            else:
                vault_source = req.vault_source

            # Derive user_dest (owner's ATA) if not provided
            if req.user_dest is None:
                user_dest = derive_ata(req.owner, req.mint, token_prog, ata_prog)
            else:
                user_dest = req.user_dest

            ix = builder.withdraw_spl_tokens(
                req.owner,
                req.mint,
                vault_source,
                user_dest,
                req.amount,
                ephemeral_ata,
                vault,
                vault_bump,
                req.token_program,
            )
            tx = await serialize_transaction(ix, req.owner, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/delegate-ephemeral-ata", response_model=TransactionResponse, tags=["Transactions"])
    async def delegate_ephemeral_ata(req: DelegateEphemeralAtaRequest):
        try:
            settings = get_settings()
            program_id = _pubkey_bytes(settings.program_id)

            # Derive ephemeral_ata and bump if not provided
            if req.ephemeral_ata is None or req.ephemeral_ata_bump is None:
                ata, bump = derive_ephemeral_ata(req.user, req.mint, program_id)
                ephemeral_ata = req.ephemeral_ata or ata
                ephemeral_ata_bump = req.ephemeral_ata_bump if req.ephemeral_ata_bump is not None else bump
            else:
                ephemeral_ata = req.ephemeral_ata
                ephemeral_ata_bump = req.ephemeral_ata_bump

            ix = builder.delegate_ephemeral_ata(
                req.payer,
                req.user,
                req.mint,
                req.owner_program,
                req.buffer,
                req.delegation_record,
                req.delegation_metadata,
                ephemeral_ata,
                ephemeral_ata_bump,
                req.validator,
            )
            tx = await serialize_transaction(ix, req.payer, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/undelegate-ephemeral-ata", response_model=TransactionResponse, tags=["Transactions"])
    async def undelegate_ephemeral_ata(req: UndelegateEphemeralAtaRequest):
        try:
            settings = get_settings()
            program_id = _pubkey_bytes(settings.program_id)
            token_prog = _pubkey_bytes(settings.token_program)
            ata_prog = _pubkey_bytes(settings.ata_program)

            # Derive ephemeral_ata if not provided
            if req.ephemeral_ata is None:
                ephemeral_ata, _ = derive_ephemeral_ata(req.user, req.mint, program_id)
            else:
                ephemeral_ata = req.ephemeral_ata

            # Derive ata (user's token account) if not provided
            if req.ata is None:
                ata = derive_ata(req.user, req.mint, token_prog, ata_prog)
            else:
                ata = req.ata

            ix = builder.undelegate_ephemeral_ata(
                req.payer,
                ata,
                req.magic_context,
                ephemeral_ata,
            )
            tx = await serialize_transaction(ix, req.payer, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/create-ephemeral-ata-permission", response_model=TransactionResponse, tags=["Transactions"])
    async def create_ephemeral_ata_permission(req: CreateEphemeralAtaPermissionRequest):
        try:
            settings = get_settings()
            program_id = _pubkey_bytes(settings.program_id)
            permission_prog = _pubkey_bytes(settings.permission_program)

            # Derive ephemeral_ata and bump if not provided
            if req.ephemeral_ata is None or req.ephemeral_ata_bump is None:
                ata, bump = derive_ephemeral_ata(req.user, req.mint, program_id)
                ephemeral_ata = req.ephemeral_ata or ata
                ephemeral_ata_bump = req.ephemeral_ata_bump if req.ephemeral_ata_bump is not None else bump
            else:
                ephemeral_ata = req.ephemeral_ata
                ephemeral_ata_bump = req.ephemeral_ata_bump

            # Derive permission if not provided
            if req.permission is None:
                permission = derive_permission(ephemeral_ata, permission_prog)
            else:
                permission = req.permission

            ix = builder.create_ephemeral_ata_permission(
                req.payer,
                req.mint,
                req.flags,
                ephemeral_ata,
                ephemeral_ata_bump,
                permission,
            )
            tx = await serialize_transaction(ix, req.payer, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/delegate-ephemeral-ata-permission", response_model=TransactionResponse, tags=["Transactions"])
    async def delegate_ephemeral_ata_permission(req: DelegateEphemeralAtaPermissionRequest):
        try:
            settings = get_settings()
            program_id = _pubkey_bytes(settings.program_id)
            permission_prog = _pubkey_bytes(settings.permission_program)

            # Derive ephemeral_ata and bump if not provided
            if req.ephemeral_ata is None or req.ephemeral_ata_bump is None:
                ata, bump = derive_ephemeral_ata(req.user, req.mint, program_id)
                ephemeral_ata = req.ephemeral_ata or ata
                ephemeral_ata_bump = req.ephemeral_ata_bump if req.ephemeral_ata_bump is not None else bump
            else:
                ephemeral_ata = req.ephemeral_ata
                ephemeral_ata_bump = req.ephemeral_ata_bump

            # Derive permission if not provided
            if req.permission is None:
                permission = derive_permission(ephemeral_ata, permission_prog)
            else:
                permission = req.permission

            ix = builder.delegate_ephemeral_ata_permission(
                req.payer,
                req.buffer,
                req.record,
                req.metadata,
                req.validator,
                ephemeral_ata,
                ephemeral_ata_bump,
                permission,
            )
            tx = await serialize_transaction(ix, req.payer, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/undelegate-ephemeral-ata-permission", response_model=TransactionResponse, tags=["Transactions"])
    async def undelegate_ephemeral_ata_permission(req: UndelegateEphemeralAtaPermissionRequest):
        try:
            settings = get_settings()
            program_id = _pubkey_bytes(settings.program_id)
            permission_prog = _pubkey_bytes(settings.permission_program)

            # Derive ephemeral_ata if not provided
            if req.ephemeral_ata is None:
                ephemeral_ata, _ = derive_ephemeral_ata(req.user, req.mint, program_id)
            else:
                ephemeral_ata = req.ephemeral_ata

            # Derive permission if not provided
            if req.permission is None:
                permission = derive_permission(ephemeral_ata, permission_prog)
            else:
                permission = req.permission

            ix = builder.undelegate_ephemeral_ata_permission(
                req.payer,
                req.magic_context,
                ephemeral_ata,
                permission,
            )
            tx = await serialize_transaction(ix, req.payer, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/reset-ephemeral-ata-permission", response_model=TransactionResponse, tags=["Transactions"])
    async def reset_ephemeral_ata_permission(req: ResetEphemeralAtaPermissionRequest):
        try:
            settings = get_settings()
            program_id = _pubkey_bytes(settings.program_id)
            permission_prog = _pubkey_bytes(settings.permission_program)

            # Derive ephemeral_ata and bump if not provided
            if req.ephemeral_ata is None or req.ephemeral_ata_bump is None:
                ata, bump = derive_ephemeral_ata(req.user, req.mint, program_id)
                ephemeral_ata = req.ephemeral_ata or ata
                ephemeral_ata_bump = req.ephemeral_ata_bump if req.ephemeral_ata_bump is not None else bump
            else:
                ephemeral_ata = req.ephemeral_ata
                ephemeral_ata_bump = req.ephemeral_ata_bump

            # Derive permission if not provided
            if req.permission is None:
                permission = derive_permission(ephemeral_ata, permission_prog)
            else:
                permission = req.permission

            ix = builder.reset_ephemeral_ata_permission(
                req.owner,
                req.mint,
                req.flags,
                ephemeral_ata,
                ephemeral_ata_bump,
                permission,
            )
            tx = await serialize_transaction(ix, req.owner, req.cluster_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/checked-transfer", response_model=TransactionResponse, tags=["Transactions"])
    async def checked_transfer(req: CheckedTransferRequest):
        try:
            # Note: decimals could be fetched from mint account via RPC if not provided
            # For now, require it to be provided to avoid RPC calls
            if req.decimals is None:
                raise ValueError("decimals is required (RPC fetch not yet implemented)")

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
            settings = get_settings()
            token_prog = _pubkey_bytes(req.token_program or settings.token_program)
            ata_prog = _pubkey_bytes(settings.ata_program)

            # Derive ATA if not provided
            if req.ata is None:
                ata = derive_ata(req.user, req.mint, token_prog, ata_prog)
            else:
                ata = req.ata

            ix = builder.initialize_ata(
                req.payer,
                req.user,
                req.mint,
                ata,
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
