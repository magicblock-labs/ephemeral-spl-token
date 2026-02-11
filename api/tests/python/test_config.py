#!/usr/bin/env python3
"""
Unit tests for configuration module.
Tests Settings class and configuration management.
"""

import pytest
import os

from src.config import Settings, get_settings


# Test fixtures
DEFAULT_ENDPOINT = "https://rpc.magicblock.app/mainnet"
DEFAULT_TEE_ENDPOINT = "https://tee.magicblock.app"
DEFAULT_EPHEMERAL_PROGRAM = "SPLxh1LVZzEkX99H6rqYizhytLWPZVV296zyYDPagv2"
DEFAULT_TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"


class TestSettingsDefaults:
    """Test Settings default values."""

    def test_settings_default_endpoint(self):
        """Test default endpoint URL."""
        settings = Settings()
        assert settings.endpoint_url == DEFAULT_ENDPOINT

    def test_settings_default_tee_endpoint(self):
        """Test default TEE endpoint URL."""
        settings = Settings()
        assert settings.tee_endpoint_url == DEFAULT_TEE_ENDPOINT

    def test_settings_default_ephemeral_program(self):
        """Test default ephemeral SPL token program."""
        settings = Settings()
        assert settings.ephemeral_spl_token_program == DEFAULT_EPHEMERAL_PROGRAM

    def test_settings_default_system_program(self):
        """Test default system program."""
        settings = Settings()
        assert settings.system_program == "11111111111111111111111111111111"

    def test_settings_default_token_program(self):
        """Test default token program."""
        settings = Settings()
        assert settings.token_program == DEFAULT_TOKEN_PROGRAM

    def test_settings_default_delegation_program(self):
        """Test default delegation program."""
        settings = Settings()
        assert settings.delegation_program == "DELeGGvXpWV2fqJUhqcF5ZSYMS4JTLjteaAMARRSaeSh"

    def test_settings_default_permission_program(self):
        """Test default permission program."""
        settings = Settings()
        assert settings.permission_program == "ACLseoPoyC3cBqoUtkbjZ4aDrkurZW86v19pXz2XQnp1"

    def test_settings_default_magic_program(self):
        """Test default magic program."""
        settings = Settings()
        assert settings.magic_program == "Magic11111111111111111111111111111111111111"

    def test_settings_default_magic_context(self):
        """Test default magic context."""
        settings = Settings()
        assert settings.magic_context == "MagicContext1111111111111111111111111111111"

    def test_settings_default_ata_program(self):
        """Test default ATA program."""
        settings = Settings()
        assert settings.ata_program == "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"

    def test_settings_default_validator(self):
        """Test default validator."""
        settings = Settings()
        assert settings.default_validator == "FnE6VJT5QNZdedZPnCoLsARgBwoE6DeJNjBs2H1gySXA"


class TestSettingsCustomization:
    """Test Settings with custom values."""

    def test_settings_custom_endpoint(self):
        """Test Settings with custom endpoint."""
        custom_endpoint = "https://custom.rpc.com"
        settings = Settings(endpoint_url=custom_endpoint)
        assert settings.endpoint_url == custom_endpoint

    def test_settings_custom_program(self):
        """Test Settings with custom program ID."""
        custom_program = "CustomProgram111111111111111111111111111111"
        settings = Settings(ephemeral_spl_token_program=custom_program)
        assert settings.ephemeral_spl_token_program == custom_program

    def test_settings_multiple_custom_values(self):
        """Test Settings with multiple custom values."""
        settings = Settings(
            endpoint_url="https://custom.rpc.com",
            tee_endpoint_url="https://custom.tee.com",
            token_program="CustomTokenProgram111111111111111111111",
        )
        assert settings.endpoint_url == "https://custom.rpc.com"
        assert settings.tee_endpoint_url == "https://custom.tee.com"
        assert settings.token_program == "CustomTokenProgram111111111111111111111"


class TestSettingsEnvironmentVariables:
    """Test Settings with environment variables."""

    def test_settings_env_prefix(self):
        """Test that Settings uses EPHEMERAL_ prefix."""
        # Settings should have env_prefix = "EPHEMERAL_"
        # Setting env var should override defaults
        os.environ["EPHEMERAL_ENDPOINT_URL"] = "https://env.rpc.com"
        try:
            settings = Settings()
            assert settings.endpoint_url == "https://env.rpc.com"
        finally:
            del os.environ["EPHEMERAL_ENDPOINT_URL"]

    def test_settings_multiple_env_vars(self):
        """Test Settings with multiple environment variables."""
        os.environ["EPHEMERAL_ENDPOINT_URL"] = "https://env1.com"
        os.environ["EPHEMERAL_TOKEN_PROGRAM"] = "EnvTokenProgram111111111111111111111111111"
        try:
            settings = Settings()
            assert settings.endpoint_url == "https://env1.com"
            assert settings.token_program == "EnvTokenProgram111111111111111111111111111"
        finally:
            del os.environ["EPHEMERAL_ENDPOINT_URL"]
            del os.environ["EPHEMERAL_TOKEN_PROGRAM"]


class TestGetSettingsSingleton:
    """Test get_settings function (singleton pattern)."""

    def test_get_settings_returns_settings(self):
        """Test that get_settings returns Settings instance."""
        settings = get_settings()
        assert isinstance(settings, Settings)

    def test_get_settings_caches_result(self):
        """Test that get_settings caches the result."""
        settings1 = get_settings()
        settings2 = get_settings()
        assert settings1 is settings2

    def test_get_settings_has_defaults(self):
        """Test that get_settings returns instance with defaults."""
        settings = get_settings()
        assert settings.endpoint_url is not None
        assert settings.token_program is not None
        assert settings.ephemeral_spl_token_program is not None


class TestSettingsValidation:
    """Test Settings validation."""

    def test_settings_all_fields_present(self):
        """Test that Settings has all required fields."""
        settings = Settings()
        assert hasattr(settings, 'endpoint_url')
        assert hasattr(settings, 'tee_endpoint_url')
        assert hasattr(settings, 'ephemeral_spl_token_program')
        assert hasattr(settings, 'system_program')
        assert hasattr(settings, 'token_program')
        assert hasattr(settings, 'delegation_program')
        assert hasattr(settings, 'permission_program')
        assert hasattr(settings, 'magic_program')
        assert hasattr(settings, 'magic_context')
        assert hasattr(settings, 'ata_program')
        assert hasattr(settings, 'default_validator')

    def test_settings_fields_not_empty(self):
        """Test that all Settings fields have non-empty values."""
        settings = Settings()
        assert len(settings.endpoint_url) > 0
        assert len(settings.tee_endpoint_url) > 0
        assert len(settings.ephemeral_spl_token_program) > 0
        assert len(settings.system_program) > 0
        assert len(settings.token_program) > 0
        assert len(settings.delegation_program) > 0
        assert len(settings.permission_program) > 0
        assert len(settings.magic_program) > 0
        assert len(settings.magic_context) > 0
        assert len(settings.ata_program) > 0
        assert len(settings.default_validator) > 0

    def test_settings_program_ids_are_strings(self):
        """Test that all program IDs are strings."""
        settings = Settings()
        assert isinstance(settings.system_program, str)
        assert isinstance(settings.token_program, str)
        assert isinstance(settings.delegation_program, str)
        assert isinstance(settings.permission_program, str)
        assert isinstance(settings.magic_program, str)
        assert isinstance(settings.magic_context, str)
        assert isinstance(settings.ata_program, str)


class TestSettingsEndpoints:
    """Test Settings endpoints."""

    def test_endpoint_url_format(self):
        """Test that endpoint URL is properly formatted."""
        settings = Settings()
        assert settings.endpoint_url.startswith("http://") or \
               settings.endpoint_url.startswith("https://")

    def test_tee_endpoint_url_format(self):
        """Test that TEE endpoint URL is properly formatted."""
        settings = Settings()
        assert settings.tee_endpoint_url.startswith("http://") or \
               settings.tee_endpoint_url.startswith("https://")

    def test_custom_endpoint_validation(self):
        """Test custom endpoint URLs."""
        valid_endpoints = [
            "http://localhost:8000",
            "https://rpc.example.com",
            "http://127.0.0.1:8899",
        ]
        for endpoint in valid_endpoints:
            settings = Settings(endpoint_url=endpoint)
            assert settings.endpoint_url == endpoint


class TestSettingsPrograms:
    """Test Settings program IDs."""

    def test_system_program_id(self):
        """Test system program ID."""
        settings = Settings()
        # System program is all 1s
        assert settings.system_program == "11111111111111111111111111111111"

    def test_token_program_id(self):
        """Test token program ID."""
        settings = Settings()
        # Known token program ID
        assert settings.token_program == "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"

    def test_ata_program_id(self):
        """Test ATA program ID."""
        settings = Settings()
        # Known ATA program ID
        assert settings.ata_program == "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"

    def test_ephemeral_program_id_valid_length(self):
        """Test that ephemeral program ID has valid length."""
        settings = Settings()
        # Solana addresses are base58 and should be 43-44 chars
        assert 40 <= len(settings.ephemeral_spl_token_program) <= 50

    def test_all_program_ids_unique(self):
        """Test that all program IDs are unique."""
        settings = Settings()
        programs = [
            settings.system_program,
            settings.token_program,
            settings.delegation_program,
            settings.permission_program,
            settings.magic_program,
            settings.ata_program,
        ]
        # All programs should be unique
        assert len(programs) == len(set(programs))


class TestSettingsComparison:
    """Test Settings instance comparison."""

    def test_settings_equality(self):
        """Test Settings equality."""
        settings1 = Settings()
        settings2 = Settings()
        # Two default Settings instances should be equal
        assert settings1.endpoint_url == settings2.endpoint_url
        assert settings1.token_program == settings2.token_program

    def test_settings_custom_values_equality(self):
        """Test equality with custom values."""
        custom_endpoint = "https://custom.com"
        settings1 = Settings(endpoint_url=custom_endpoint)
        settings2 = Settings(endpoint_url=custom_endpoint)
        assert settings1.endpoint_url == settings2.endpoint_url


class TestSettingsDocumentation:
    """Test Settings documentation and schema."""

    def test_settings_has_docstring(self):
        """Test that Settings class has documentation."""
        assert Settings.__doc__ is not None
        assert len(Settings.__doc__) > 0

    def test_settings_config_class(self):
        """Test Settings has Config class."""
        assert hasattr(Settings, 'Config')
        assert hasattr(Settings.Config, 'env_prefix')
        assert Settings.Config.env_prefix == "EPHEMERAL_"


class TestDefaultValidatorAddress:
    """Test default validator address."""

    def test_default_validator_format(self):
        """Test default validator is valid address format."""
        settings = Settings()
        validator = settings.default_validator
        # Should be base58 string
        assert isinstance(validator, str)
        assert len(validator) > 0
        # Should be valid Solana address length
        assert 40 <= len(validator) <= 50


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
