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
        WithdrawRequest,
        DelegateEphemeralAtaRequest,
        UndelegateEphemeralAtaRequest,
        TransferAmountRequest,
        InitializeAtaRequest,
        DepositRequest,
        PrepareWithdrawalRequest
    )
    from .builder import builder, serialize_transaction
    from .rpc import check_accounts, get_mint_decimals, get_mint_program
    from .pda import (
        derive_accounts
    )

    @asynccontextmanager
    async def lifespan(app: FastAPI):
        yield

    app = FastAPI(
        title="Private Ephemeral SPL Token API",
        description="Build transactions for the MagicBlock Private token program (https://docs.magicblock.gg/pages/private-ephemeral-rollups-pers)",
        version="0.1.0",
        lifespan=lifespan,
        servers=[
            {
                "url": "https://api.docs.magicblock.app",
                "description": "Private Transfer API (Beta)",
            },
        ],
    )

    async def get_token_program(mint: str, endpoint_url: str) -> str:
        """
        Fetch token program from mint account, fallback to settings default.
        
        Args:
            mint: Token mint pubkey
            endpoint_url: RPC endpoint URL
            
        Returns:
            Token program ID (fetched from mint or from settings)
        """
        try:
            return await get_mint_program(mint, endpoint_url)
        except Exception:
            # Fallback to default token program from settings
            settings = get_settings()
            return settings.token_program


    async def build_deposit_instructions(payer, user, mint, amount, endpoint_url, validator, ephemeral_spl_token_program, token_program, permission_program, delegation_program):
        """
        Build instructions to initialize accounts and optionally deposit tokens.
        
        Returns a list of instructions based on account state checks.
        """
        
        # Derive all accounts from user and mint in one call
        derived = derive_accounts(
            user,
            mint,
            ephemeral_spl_token_program=ephemeral_spl_token_program,
            token_program=token_program,
            permission_program=permission_program,
            delegation_program=delegation_program,
        )
        
        # Check all accounts in parallel (Promise.all pattern)
        account_checks = {
            "vault": derived.vault,
            "vault_ata": derived.vault_ata,
            "user_ata": derived.user_ata,
            "ephemeral_ata": derived.ephemeral_ata,
            "permission": derived.permission,
        }
        # Pass delegation program for checking delegation status
        account_states = await check_accounts(
            account_checks,
            endpoint_url,
            delegation_program=delegation_program,
        )
        
        # Check if ephemeral_ata or permission are already delegated
        if account_states["ephemeral_ata"].is_delegated:
            raise HTTPException(
                status_code=400,
                detail="Ephemeral ATA is already delegated. Cannot initialize or deposit, please undelegate first.",
            )
        
        # Build list of instructions based on account states
        instructions = []
        
        # Initialize global vault if not already initialized
        if not account_states["vault"].exists:
            init_vault_ix = builder.initialize_global_vault(
                payer,
                mint,
                derived.vault,
                derived.vault_bump,
            )
            instructions.append(init_vault_ix)
        
        # Initialize vault's ATA if not already initialized
        if not account_states["vault_ata"].exists:
            init_vault_ata_ix = builder.initialize_ata(
                payer,
                derived.vault,
                mint,
                derived.vault_ata,
                token_program,
            )
            instructions.append(init_vault_ata_ix)
        
        # Initialize user's ATA if not already initialized
        if not account_states["user_ata"].exists:
            init_ata_ix = builder.initialize_ata(
                payer,
                user,
                mint,
                derived.user_ata,
                token_program,
            )
            instructions.append(init_ata_ix)
        
        # Initialize ephemeral ATA if not already initialized
        ephemeral_ata_just_initialized = False
        if not account_states["ephemeral_ata"].exists:
            init_ephemeral_ata_ix = builder.initialize_ephemeral_ata(
                payer,
                user,
                mint,
                derived.ephemeral_ata,
                derived.ephemeral_ata_bump,
            )
            instructions.append(init_ephemeral_ata_ix)
            ephemeral_ata_just_initialized = True

        # Include deposit instruction if amount > 0
        if not account_states["ephemeral_ata"].is_delegated:
            if amount > 0:
                deposit_ix = builder.deposit_spl_tokens(
                    user,
                    derived.user_ata,
                    derived.vault_ata,
                    amount,
                    derived.ephemeral_ata,
                    derived.vault,
                    mint,
                    token_program,
                )
                instructions.append(deposit_ix)
        
        # Delegate ephemeral ATA if not already delegated
        if ephemeral_ata_just_initialized or not account_states["ephemeral_ata"].is_delegated:
            delegate_ix = builder.delegate_ephemeral_ata(
                payer,
                ephemeral_spl_token_program,
                derived.eata_delegation_buffer,
                derived.eata_delegation_record,
                derived.eata_delegation_metadata,
                derived.ephemeral_ata,
                derived.ephemeral_ata_bump,
                validator,
            )
            instructions.append(delegate_ix)
        
        
        # Initialize ephemeral ATA's permission if not already initialized
        permission_just_initialized = False
        if not account_states["permission"].exists:
            create_permission_ix = builder.create_ephemeral_ata_permission(
                payer,
                mint,
                0,  # flags: default permissions
                derived.ephemeral_ata,
                derived.ephemeral_ata_bump,
                derived.permission,
            )
            instructions.append(create_permission_ix)
            permission_just_initialized = True
        
        # Delegate ephemeral ATA's permission if not already delegated
        if permission_just_initialized or not account_states["permission"].is_delegated:
            delegate_permission_ix = builder.delegate_ephemeral_ata_permission(
                payer,
                derived.permission_delegation_buffer,
                derived.permission_delegation_record,
                derived.permission_delegation_metadata,
                validator,
                derived.ephemeral_ata,
                derived.ephemeral_ata_bump,
                derived.permission,
            )
            instructions.append(delegate_permission_ix)
        
        return instructions

    @app.get("/", tags=["Health"])
    async def root():
        settings = get_settings()
        return {
            "status": "ok",
            "ephemeral_spl_token_program": settings.ephemeral_spl_token_program,
            "endpoint_url": settings.endpoint_url,
        }

    @app.get("/config", response_model=Settings, tags=["Config"])
    async def get_config():
        """
        Get configuration settings for default endpoints, programs, and validator.
        """
        settings = get_settings()
        return settings

    @app.post("/tx/initialize-ephemeral-ata", response_model=TransactionResponse, tags=["Transactions"])
    async def initialize_ephemeral_ata(req: InitializeEphemeralAtaRequest):
        try:
            settings = get_settings()
            endpoint_url = req.endpoint_url or settings.endpoint_url

            # Fetch token program from mint (with fallback to settings)
            token_program = await get_token_program(req.mint, endpoint_url)

            # Use defaults from settings if not provided in request
            derived = derive_accounts(
                req.owner,
                req.mint,
                ephemeral_spl_token_program=settings.ephemeral_spl_token_program,
                token_program=token_program,
                permission_program=settings.permission_program,
                delegation_program=settings.delegation_program,
            )
            ix = builder.initialize_ephemeral_ata(
                req.payer,
                req.owner,
                req.mint,
                derived.ephemeral_ata,
                derived.ephemeral_ata_bump,
            )
            tx = await serialize_transaction(ix, req.payer, endpoint_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/initialize-global-vault", response_model=TransactionResponse, tags=["Transactions"])
    async def initialize_global_vault(req: InitializeGlobalVaultRequest):
        try:
            settings = get_settings()
            endpoint_url = req.endpoint_url or settings.endpoint_url

            # Use defaults from settings if not provided in request
            # Fetch token program from mint (with fallback to settings)

            token_program = await get_token_program(req.mint, endpoint_url)

            

            derived = derive_accounts(
                req.payer,
                req.mint,
                ephemeral_spl_token_program=settings.ephemeral_spl_token_program,
                token_program=token_program,
                permission_program=settings.permission_program,
                delegation_program=settings.delegation_program,
            )
            ix = builder.initialize_global_vault(
                req.payer,
                req.mint,
                derived.vault,
                derived.vault_bump,
            )
            tx = await serialize_transaction(ix, req.payer, endpoint_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/deposit-spl-tokens", response_model=TransactionResponse, tags=["Transactions"])
    async def deposit_spl_tokens(req: DepositSplTokensRequest):
        try:

            settings = get_settings()
            endpoint_url = req.endpoint_url or settings.endpoint_url

            # Use defaults from settings if not provided in request
            # Fetch token program from mint (with fallback to settings)

            token_program = await get_token_program(req.mint, endpoint_url)

            

            derived = derive_accounts(
                req.user,
                req.mint,
                ephemeral_spl_token_program=settings.ephemeral_spl_token_program,
                token_program=token_program,
                permission_program=settings.permission_program,
                delegation_program=settings.delegation_program,
            )
            ix = builder.deposit_spl_tokens(
                req.owner,
                req.user,
                req.mint,
                derived.user_ata,
                derived.vault_ata,
                req.amount,
                derived.ephemeral_ata,
                derived.vault,
                settings.token_program,
            )
            tx = await serialize_transaction(ix, req.owner, endpoint_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/withdraw-spl-tokens", response_model=TransactionResponse, tags=["Transactions"])
    async def withdraw_spl_tokens(req: WithdrawRequest):
        try:

            settings = get_settings()
            endpoint_url = req.endpoint_url or settings.endpoint_url

            # Use defaults from settings if not provided in request
            # Fetch token program from mint (with fallback to settings)

            token_program = await get_token_program(req.mint, endpoint_url)

            

            derived = derive_accounts(
                req.user,
                req.mint,
                ephemeral_spl_token_program=settings.ephemeral_spl_token_program,
                token_program=token_program,
                permission_program=settings.permission_program,
                delegation_program=settings.delegation_program,
            )
            ix = builder.withdraw_spl_tokens(
                req.owner,
                req.mint,
                derived.vault_ata,
                derived.user_ata,
                req.amount,
                derived.ephemeral_ata,
                derived.vault,
                derived.vault_bump,
                settings.token_program,
            )
            tx = await serialize_transaction(ix, req.owner, endpoint_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/delegate-ephemeral-ata", response_model=TransactionResponse, tags=["Transactions"])
    async def delegate_ephemeral_ata(req: DelegateEphemeralAtaRequest):
        try:

            settings = get_settings()
            endpoint_url = req.endpoint_url or settings.endpoint_url

            # Use defaults from settings if not provided in request
            # Fetch token program from mint (with fallback to settings)

            token_program = await get_token_program(req.mint, endpoint_url)

            

            derived = derive_accounts(
                req.user,
                req.mint,
                ephemeral_spl_token_program=settings.ephemeral_spl_token_program,
                token_program=token_program,
                permission_program=settings.permission_program,
                delegation_program=settings.delegation_program,
            )
            ix = builder.delegate_ephemeral_ata(
                req.payer,
                req.owner_program,
                derived.eata_delegation_buffer,
                derived.eata_delegation_record,
                derived.eata_delegation_metadata,
                derived.ephemeral_ata,
                derived.ephemeral_ata_bump,
                req.validator,
            )
            tx = await serialize_transaction(ix, req.payer, endpoint_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/undelegate-ephemeral-ata", response_model=TransactionResponse, tags=["Transactions"])
    async def undelegate_ephemeral_ata(req: UndelegateEphemeralAtaRequest):
        try:

            settings = get_settings()
            endpoint_url = req.endpoint_url or settings.tee_endpoint_url

            # Use defaults from settings if not provided in request
            # Fetch token program from mint (with fallback to settings)

            token_program = await get_token_program(req.mint, endpoint_url)

            

            derived = derive_accounts(
                req.user,
                req.mint,
                ephemeral_spl_token_program=settings.ephemeral_spl_token_program,
                token_program=token_program,
                permission_program=settings.permission_program,
                delegation_program=settings.delegation_program,
            )
            ix = builder.undelegate_ephemeral_ata(
                req.payer,
                derived.user_ata,
                derived.ephemeral_ata,
            )

            tx = await serialize_transaction(ix, req.payer, endpoint_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/checked-transfer", response_model=TransactionResponse, tags=["Transactions"])
    async def checked_transfer(req: TransferAmountRequest):
        try:
            settings = get_settings()
            endpoint_url = req.endpoint_url or settings.tee_endpoint_url

            # Use defaults from settings if not provided in request
            # Fetch token program from mint (with fallback to settings)

            token_program = await get_token_program(req.mint, endpoint_url)

            derived_sender = derive_accounts(
                req.sender,
                req.mint,
                ephemeral_spl_token_program=settings.ephemeral_spl_token_program,
                token_program=token_program,
                permission_program=settings.permission_program,
                delegation_program=settings.delegation_program,
            )
            # Fetch token program from mint (with fallback to settings)

            token_program = await get_token_program(req.mint, endpoint_url)

            

            derived_recipient = derive_accounts(
                req.recipient,
                req.mint,
                ephemeral_spl_token_program=settings.ephemeral_spl_token_program,
                token_program=token_program,
                permission_program=settings.permission_program,
                delegation_program=settings.delegation_program,
            )
            
            # Fetch decimals from mint account
            decimals = await get_mint_decimals(req.mint, endpoint_url)
            
            ix = builder.checked_transfer(
                derived_sender.user_ata,
                derived_recipient.user_ata,
                req.mint,
                req.amount,
                decimals,
                req.sender,
                settings.token_program,
            )
            tx = await serialize_transaction(ix, req.sender, endpoint_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/tx/initialize-ata", response_model=TransactionResponse, tags=["Transactions"])
    async def initialize_ata(req: InitializeAtaRequest):
        try:

            settings = get_settings()
            endpoint_url = req.endpoint_url or settings.endpoint_url

            # Use defaults from settings if not provided in request
            # Fetch token program from mint (with fallback to settings)

            token_program = await get_token_program(req.mint, endpoint_url)

            

            derived = derive_accounts(
                req.owner,
                req.mint,
                ephemeral_spl_token_program=settings.ephemeral_spl_token_program,
                token_program=token_program,
                permission_program=settings.permission_program,
                delegation_program=settings.delegation_program,
            )
            
            ix = builder.initialize_ata(
                req.payer,
                req.owner,
                req.mint,
                derived.user_ata,
                settings.token_program,
            )
            tx = await serialize_transaction(ix, req.payer, endpoint_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))
    


    # === Private Transaction Endpoints (tx/private/...) ===


    @app.post("/private/tx/deposit", response_model=TransactionResponse, tags=["Private Transactions"])
    async def private_deposit(req: DepositRequest):
        """
        Deposit SPL token account on Solana for private transfer.

        This initialization process includes the following steps:
        - Initialize global vault (if not already initialized)
        - Initialize vault's ATA (if not already initialized)
        - Initialize user's ATA (if not already initialized)
        - Initialize ephemeral ATA (if not already initialized)
        - Deposit SPL tokens (if amount > 0)
        - Delegate ephemeral ATA (if not already delegated)
        - Initialize ephemeral ATA's permission (if not already initialized)
        - Delegate ephemeral ATA's permission (if not already delegated)
        """
        try:
            settings = get_settings()
            endpoint_url = req.endpoint_url or settings.endpoint_url

            # Use defaults from settings if not provided in request
            ephemeral_spl_token_program = settings.ephemeral_spl_token_program
            token_program = settings.token_program
            permission_program = settings.permission_program
            delegation_program = settings.delegation_program
            validator = settings.default_validator
            amount = req.amount or 0
            user = req.user
            mint = req.mint

            instructions = await build_deposit_instructions(req.payer, user, mint, amount, endpoint_url, validator, ephemeral_spl_token_program, token_program, permission_program, delegation_program)
            
            # Serialize all instructions in one transaction
            tx = await serialize_transaction(instructions, req.payer, endpoint_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))
        
    @app.post("/private/tx/transfer-amount", response_model=TransactionResponse, tags=["Private Transactions"])
    async def private_transfer_amount(req: TransferAmountRequest):
        """
        Transfer SPL token amount (checked_transfer) privately.
        """
        try:
            settings = get_settings()
            endpoint_url = req.endpoint_url or settings.tee_endpoint_url

            # Use defaults from settings if not provided in request
            # Fetch token program from mint (with fallback to settings)

            token_program = await get_token_program(req.mint, endpoint_url)

            derived_sender = derive_accounts(
                req.sender,
                req.mint,
                ephemeral_spl_token_program=settings.ephemeral_spl_token_program,
                token_program=token_program,
                permission_program=settings.permission_program,
                delegation_program=settings.delegation_program,
            )
            # Fetch token program from mint (with fallback to settings)

            token_program = await get_token_program(req.mint, endpoint_url)

            

            derived_recipient = derive_accounts(
                req.recipient,
                req.mint,
                ephemeral_spl_token_program=settings.ephemeral_spl_token_program,
                token_program=token_program,
                permission_program=settings.permission_program,
                delegation_program=settings.delegation_program,
            )

            # Fetch decimals from mint account
            decimals = await get_mint_decimals(req.mint, endpoint_url)

            ix = builder.checked_transfer(
                derived_sender.user_ata,
                derived_recipient.user_ata,
                req.mint,
                req.amount,
                decimals,
                req.sender,
                settings.token_program,
            )
            
            tx = await serialize_transaction(ix, req.sender, endpoint_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/private/tx/prepare-withdrawal", response_model=TransactionResponse, tags=["Private Transactions"])
    async def private_prepare_withdrawal(req: PrepareWithdrawalRequest):
        """
        Undelegate ephemeral ATA from Private Ephemeral Rollup and prepare for withdrawal on Solana.
        """
        try:
            settings = get_settings()
            endpoint_url = req.endpoint_url or settings.tee_endpoint_url

            # Use defaults from settings if not provided in request
            ephemeral_spl_token_program = settings.ephemeral_spl_token_program
            token_program = settings.token_program
            permission_program = settings.permission_program
            delegation_program = settings.delegation_program
            endpoint_url = req.endpoint_url or settings.endpoint_url

            # Derive all accounts from user and mint in one call
            derived = derive_accounts(
                req.user,
                req.mint,
                ephemeral_spl_token_program=ephemeral_spl_token_program,
                token_program=token_program,
                permission_program=permission_program,
                delegation_program=delegation_program,
            )

            ix = builder.undelegate_ephemeral_ata(
                req.user,
                derived.user_ata,
                derived.ephemeral_ata,
            )

            tx = await serialize_transaction(ix, req.user, endpoint_url)
            return TransactionResponse(transaction=tx)
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    @app.post("/private/tx/withdraw", response_model=TransactionResponse, tags=["Private Transactions"])
    async def private_withdraw(req: WithdrawRequest):
        """
        Withdraw SPL tokens from an ephemeral ATA on Solana.
        """
        try:
            settings = get_settings()

            # Use defaults from settings if not provided in request
            ephemeral_spl_token_program = settings.ephemeral_spl_token_program
            token_program = settings.token_program
            permission_program = settings.permission_program
            delegation_program = settings.delegation_program
            endpoint_url = req.endpoint_url or settings.endpoint_url

            # Derive all accounts from user and mint in one call
            derived = derive_accounts(
                req.user,
                req.mint,
                ephemeral_spl_token_program=ephemeral_spl_token_program,
                token_program=token_program,
                permission_program=permission_program,
                delegation_program=delegation_program,
            )

            ix = builder.withdraw_spl_tokens(
                req.owner,
                req.mint,
                derived.vault_ata,
                derived.user_ata,
                req.amount,
                derived.ephemeral_ata,
                derived.vault,
                derived.vault_bump,
                token_program,
            )
            tx = await serialize_transaction(ix, req.owner, endpoint_url)
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
