"""Configuration for Ephemeral SPL Token API."""

from pydantic_settings import BaseSettings
from functools import lru_cache


class Settings(BaseSettings):
    """API configuration settings."""

    # Solana cluster URLs
    cluster_url: str = "https://rpc.magicblock.app/mainnet"
    tee_cluster_url: str = "https://tee.magicblock.app"

    # Program IDs
    ephemeral_spl_token_program: str = "SPLxh1LVZzEkX99H6rqYizhytLWPZVV296zyYDPagv2"
    system_program: str = "11111111111111111111111111111111"
    token_program: str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
    delegation_program: str = "DELeGGvXpWV2fqJUhqcF5ZSYMS4JTLjteaAMARRSaeSh"
    permission_program: str = "ACLseoPoyC3cBqoUtkbjZ4aDrkurZW86v19pXz2XQnp1"
    magic_program: str = "Magic11111111111111111111111111111111111111"
    ata_program: str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
    
    # Default validator for permission delegation
    default_validator: str = "FnE6VJT5QNZdedZPnCoLsARgBwoE6DeJNjBs2H1gySXA"

    class Config:
        env_prefix = "EPHEMERAL_"


@lru_cache
def get_settings() -> Settings:
    return Settings()
