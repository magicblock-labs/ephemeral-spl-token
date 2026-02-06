"""RPC utilities for checking account states on Solana."""

import json
from typing import Optional
from dataclasses import dataclass

from .config import get_settings

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


@dataclass(frozen=True)
class AccountState:
    """Represents the state of an account."""
    exists: bool
    owner: Optional[str] = None
    lamports: int = 0
    data: bytes = b""
    is_delegated: bool = False  # Set to True if owner matches delegation program


async def get_account_info(
    pubkey: str,
    cluster_url: Optional[str] = None,
) -> AccountState:
    """Fetch account info from RPC and determine if it's initialized/delegated."""
    settings = get_settings()
    url = cluster_url or settings.cluster_url
    
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getAccountInfo",
        "params": [pubkey, {"encoding": "base64", "commitment": "confirmed"}],
    }
    
    try:
        response = await _http_post_json(url, payload)
        result = response.get("result")
        
        if result is None or result["value"] is None:
            return AccountState(exists=False)
        
        account_data = result["value"]
        owner = account_data.get("owner")
        lamports = account_data.get("lamports", 0)
        # Note: data comes as base64, we keep it as string for now
        data = account_data.get("data", ["", "base64"])[0]
        
        return AccountState(
            exists=True,
            owner=owner,
            lamports=lamports,
            data=data,
        )
    except Exception as e:
        print(f"Error fetching account info for {pubkey}: {e}")
        return AccountState(exists=False)


async def check_accounts(
    accounts: dict[str, str],
    cluster_url: Optional[str] = None,
    delegation_program: Optional[str] = None,
) -> dict[str, AccountState]:
    """
    Check multiple accounts in parallel (Promise.all pattern).
    
    Args:
        accounts: Dict mapping account name to pubkey
        cluster_url: Optional RPC URL override
        delegation_program: Optional delegation program ID to check delegation status
    
    Returns:
        Dict mapping account name to AccountState
    """
    import asyncio
    
    tasks = {}
    for name, pubkey in accounts.items():
        tasks[name] = get_account_info(pubkey, cluster_url)
    
    results = await asyncio.gather(*tasks.values())
    state_dict = dict(zip(tasks.keys(), results))
    
    # Check each account and set delegation status
    for account_name, account in state_dict.items():
        if not account.exists:
            # Account not initialized
            state_dict[account_name] = AccountState(
                exists=False,
                is_delegated=False,
            )
        elif delegation_program:
            # Account exists, check if it's delegated to the delegation program
            if account.owner == delegation_program:
                # Account is delegated
                state_dict[account_name] = AccountState(
                    exists=account.exists,
                    owner=account.owner,
                    lamports=account.lamports,
                    data=account.data,
                    is_delegated=True,
                )
            else:
                # Account exists but not delegated
                state_dict[account_name] = AccountState(
                    exists=account.exists,
                    owner=account.owner,
                    lamports=account.lamports,
                    data=account.data,
                    is_delegated=False,
                )
    
    return state_dict


async def is_vault_initialized(vault_pubkey: str, cluster_url: Optional[str] = None) -> bool:
    """Check if global vault is initialized by checking if any data exists."""
    account = await get_account_info(vault_pubkey, cluster_url)
    return account.exists and len(account.data) > 0


async def is_ata_initialized(ata_pubkey: str, cluster_url: Optional[str] = None) -> bool:
    """Check if an ATA is initialized by checking if any data exists."""
    account = await get_account_info(ata_pubkey, cluster_url)
    return account.exists and len(account.data) > 0


async def is_ephemeral_ata_initialized(ephemeral_ata_pubkey: str, cluster_url: Optional[str] = None) -> bool:
    """Check if ephemeral ATA is initialized by checking if any data exists."""
    account = await get_account_info(ephemeral_ata_pubkey, cluster_url)
    return account.exists and len(account.data) > 0


async def is_ephemeral_ata_delegated(
    ephemeral_ata_pubkey: str,
    delegation_program_owner: Optional[str] = None,
    cluster_url: Optional[str] = None,
) -> bool:
    """
    Check if ephemeral ATA is delegated by comparing its owner to the delegation program.
    
    Args:
        ephemeral_ata_pubkey: The ephemeral ATA account address
        delegation_program_owner: The expected owner if delegated (delegation program address)
        cluster_url: Optional RPC URL override
    
    Returns:
        True if account owner matches the delegation program owner, False otherwise
    """
    account = await get_account_info(ephemeral_ata_pubkey, cluster_url)
    if not account.exists or not account.owner:
        return False
    return account.owner == delegation_program_owner
