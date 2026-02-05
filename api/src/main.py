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
        DepositPrivateBalanceRequest,
    )
    from .builder import builder, serialize_transaction
    from .rpc import check_accounts
    from .pda import (
        derive_accounts,
        derive_vault_ata,
        derive_delegation_pdas,
        derive_permission,
        derive_permission_delegation_pdas,
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
            "ephemeral_spl_token_program": settings.ephemeral_spl_token_program,
            "cluster_url": settings.cluster_url,
        }

    @app.get("/config", response_model=Settings, tags=["Config"])
    async def get_config():
        settings = get_settings()
        return settings

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
            cluster = req.cluster_url or settings.tee_cluster_url
            tx = await serialize_transaction(ix, req.payer, cluster)
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
            cluster = req.cluster_url or settings.tee_cluster_url
            tx = await serialize_transaction(ix, req.payer, cluster)
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
            cluster = req.cluster_url or settings.tee_cluster_url
            tx = await serialize_transaction(ix, req.owner, cluster)
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
            cluster = req.cluster_url or settings.tee_cluster_url
            tx = await serialize_transaction(ix, req.authority, cluster)
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
        
    @app.post("/tx/deposit_private_balance", response_model=TransactionResponse, tags=["Transactions"])
    async def deposit_private_balance(req: DepositPrivateBalanceRequest):
        """
        Deposit SPL tokens and initialize necessary accounts in a single transaction.
        
        Automatically derives accounts from user and mint:
        - vault (PDA from mint)
        - ephemeral_ata (PDA from [user, mint])
        - user_ata (standard ATA from user and mint)
        - vault_ata (standard ATA for vault)
        
        Checks account states in parallel (Promise.all pattern):
        - Global vault initialization
        - Vault's token account initialization
        - User's token account initialization
        - Ephemeral ATA initialization
        - Ephemeral ATA delegation status
        
        Creates and aggregates only the necessary instructions based on account states.
        """
        try:
            # Derive all accounts from user and mint
            derived = derive_accounts(
                req.user,
                req.mint,
                ephemeral_program=req.ephemeral_spl_token_program,
                token_program=req.token_program,
            )
            # source_token is the same as user_ata
            source_token = derived.user_ata
            
            vault_ata = derive_vault_ata(
                derived.vault,
                req.mint,
                token_program=req.token_program,
            )
            
            # Derive delegation PDAs from ephemeral_ata and ephemeral_spl_token_program
            ephemeral_program = req.ephemeral_spl_token_program or settings.ephemeral_spl_token_program
            delegation_pdas = derive_delegation_pdas(
                derived.ephemeral_ata,
                ephemeral_program,
                delegation_program=req.delegation_program,
            )
            
            # Derive permission and its delegation PDAs
            permission, permission_bump = derive_permission(derived.ephemeral_ata)
            permission_delegation_pdas = derive_permission_delegation_pdas(
                permission,
                permission_program=req.permission_program,
                delegation_program=req.delegation_program,
            )
            
            # Check all accounts in parallel (Promise.all pattern)
            account_checks = {
                "vault": derived.vault,
                "vault_token": vault_ata,
                "ata": derived.user_ata,
                "ephemeral_ata": derived.ephemeral_ata,
                "permission": permission,
            }
            # Pass delegation programs for each account that can be delegated
            account_states = await check_accounts(
                account_checks,
                req.cluster_url,
                delegation_programs={
                    "ephemeral_ata": ephemeral_program,
                    "permission": req.permission_program or settings.permission_program,
                },
            )
            
            # Build list of instructions based on account states
            instructions = []
            
            # Initialize global vault if not already initialized
            if not account_states["vault"].exists:
                init_vault_ix = builder.initialize_global_vault(
                    req.user,
                    req.mint,
                    derived.vault,
                    derived.vault_bump,
                )
                instructions.append(init_vault_ix)
            
            # Initialize vault's ATA if not already initialized
            if not account_states["vault_token"].exists:
                init_vault_ata_ix = builder.initialize_ata(
                    req.user,
                    derived.vault,
                    req.mint,
                    vault_ata,
                    req.token_program,
                )
                instructions.append(init_vault_ata_ix)
            
            # Initialize user's ATA if not already initialized
            if not account_states["ata"].exists:
                init_ata_ix = builder.initialize_ata(
                    req.user,
                    req.user,
                    req.mint,
                    derived.user_ata,
                    req.token_program,
                )
                instructions.append(init_ata_ix)
            
            # Initialize ephemeral ATA if not already initialized
            if not account_states["ephemeral_ata"].exists:
                init_ephemeral_ata_ix = builder.initialize_ephemeral_ata(
                    req.user,
                    req.user,
                    req.mint,
                    derived.ephemeral_ata,
                    derived.ephemeral_ata_bump,
                )
                instructions.append(init_ephemeral_ata_ix)
            
            # Delegate ephemeral ATA if not already delegated
            if account_states["ephemeral_ata"].exists and not account_states["ephemeral_ata"].is_delegated:
                delegate_ix = builder.delegate_ephemeral_ata(
                    req.user,
                    req.user,
                    req.mint,
                    req.ephemeral_spl_token_program,
                    delegation_pdas.delegation_buffer,
                    delegation_pdas.delegation_record,
                    delegation_pdas.delegation_metadata,
                    derived.ephemeral_ata,
                    derived.ephemeral_ata_bump,
                    req.validator,
                )
                instructions.append(delegate_ix)
            
            # Initialize ephemeral ATA's permission if not already initialized
            if not account_states["permission"].exists:
                create_permission_ix = builder.create_ephemeral_ata_permission(
                    req.user,
                    req.mint,
                    0,  # flags: default permissions
                    derived.ephemeral_ata,
                    derived.ephemeral_ata_bump,
                    permission,
                )
                instructions.append(create_permission_ix)
            
            # Delegate ephemeral ATA's permission if not already delegated
            if account_states["permission"].exists and not account_states["permission"].is_delegated:
                delegate_permission_ix = builder.delegate_ephemeral_ata_permission(
                    req.user,
                    permission_delegation_pdas.delegation_buffer,
                    permission_delegation_pdas.delegation_record,
                    permission_delegation_pdas.delegation_metadata,
                    req.validator or settings.default_validator,  # validator: use provided or default from config
                    derived.ephemeral_ata,
                    derived.ephemeral_ata_bump,
                    permission,
                )
                instructions.append(delegate_permission_ix)
            
            # Always include the deposit instruction
            deposit_ix = builder.deposit_private_balance(
                req.user,
                req.user,
                req.mint,
                source_token,
                vault_ata,
                req.amount,
                derived.ephemeral_ata,
                derived.vault,
                req.token_program,
            )
            instructions.append(deposit_ix)
            
            # Serialize all instructions in one transaction
            tx = await serialize_transaction(instructions, req.user, req.cluster_url)
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
