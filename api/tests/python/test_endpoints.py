#!/usr/bin/env python3
"""
Test script for Ephemeral SPL Token API endpoints.
Tests all available transaction endpoints with sample data.
"""

import httpx
import json
from typing import Dict, Any

# Test configuration
BASE_URL = "http://localhost:8000"
TIMEOUT = 30.0

# Sample Solana pubkeys for testing (valid base58 format)
# These are real Solana addresses or valid test addresses
SAMPLE_PAYER = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
SAMPLE_USER = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
SAMPLE_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
SAMPLE_ATA = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
SAMPLE_AUTHORITY = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
SAMPLE_SOURCE = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
SAMPLE_DEST = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
SAMPLE_VAULT = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
SAMPLE_VAULT_TOKEN = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
SAMPLE_VAULT_SOURCE = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
SAMPLE_OWNER_PROGRAM = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
SAMPLE_BUFFER = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
SAMPLE_DELEGATION_RECORD = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
SAMPLE_DELEGATION_METADATA = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
SAMPLE_PERMISSION = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
SAMPLE_MAGIC_CONTEXT = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"


class APITester:
    """Test helper for API endpoints."""

    def __init__(self, base_url: str = BASE_URL):
        self.base_url = base_url
        self.client = httpx.Client(timeout=TIMEOUT)
        self.results = []

    def test(self, method: str, endpoint: str, data: Dict[str, Any] = None) -> bool:
        """Test an endpoint and record the result."""
        url = f"{self.base_url}{endpoint}"
        try:
            if method.upper() == "GET":
                response = self.client.get(url)
            elif method.upper() == "POST":
                response = self.client.post(url, json=data)
            else:
                raise ValueError(f"Unsupported method: {method}")

            success = response.status_code in (200, 201)
            status = "✓ PASS" if success else "✗ FAIL"
            
            result = {
                "endpoint": endpoint,
                "method": method,
                "status_code": response.status_code,
                "success": success,
            }

            print(f"{status} {method} {endpoint} - {response.status_code}")

            if not success:
                try:
                    result["error"] = response.json()
                except:
                    result["error"] = response.text
                print(f"     Error: {result['error']}")

            self.results.append(result)
            return success

        except Exception as e:
            print(f"✗ ERROR {method} {endpoint}")
            print(f"     Exception: {str(e)}")
            self.results.append({
                "endpoint": endpoint,
                "method": method,
                "success": False,
                "error": str(e),
            })
            return False

    def test_health_endpoints(self):
        """Test health check endpoints."""
        print("\n=== Health Check Endpoints ===")
        self.test("GET", "/")
        self.test("GET", "/config")

    def test_ephemeral_ata_endpoints(self):
        """Test ephemeral ATA endpoints."""
        print("\n=== Ephemeral ATA Endpoints ===")
        
        # Initialize ephemeral ATA
        self.test("POST", "/tx/initialize-ephemeral-ata", {
            "user": SAMPLE_USER,
            "mint": SAMPLE_MINT,
        })

        # Delegate ephemeral ATA
        self.test("POST", "/tx/delegate-ephemeral-ata", {
            "payer": SAMPLE_PAYER,
            "user": SAMPLE_USER,
            "mint": SAMPLE_MINT,
            "owner_program": SAMPLE_OWNER_PROGRAM,
        })

        # Undelegate ephemeral ATA
        self.test("POST", "/tx/undelegate-ephemeral-ata", {
            "payer": SAMPLE_PAYER,
            "user": SAMPLE_USER,
            "mint": SAMPLE_MINT,
            "magic_context": SAMPLE_MAGIC_CONTEXT,
        })

    def test_vault_endpoints(self):
        """Test global vault endpoints."""
        print("\n=== Global Vault Endpoints ===")
        
        self.test("POST", "/tx/initialize-global-vault", {
            "payer": SAMPLE_PAYER,
            "mint": SAMPLE_MINT,
        })

    def test_deposit_withdraw_endpoints(self):
        """Test deposit and withdraw endpoints."""
        print("\n=== Deposit/Withdraw Endpoints ===")
        
        # Deposit SPL tokens
        self.test("POST", "/tx/deposit-spl-tokens", {
            "owner": SAMPLE_AUTHORITY,
            "user": SAMPLE_USER,
            "mint": SAMPLE_MINT,
            "amount": 1000000,
        })

        # Withdraw SPL tokens
        self.test("POST", "/tx/withdraw-spl-tokens", {
            "owner": SAMPLE_USER,
            "user": SAMPLE_USER,
            "mint": SAMPLE_MINT,
            "amount": 500000,
        })

    def test_permission_endpoints(self):
        """Test permission endpoints."""
        print("\n=== Permission Endpoints ===")
        
        # Create ephemeral ATA permission
        self.test("POST", "/tx/create-ephemeral-ata-permission", {
            "payer": SAMPLE_PAYER,
            "user": SAMPLE_USER,
            "mint": SAMPLE_MINT,
            "flags": 1,
        })

        # Delegate ephemeral ATA permission
        self.test("POST", "/tx/delegate-ephemeral-ata-permission", {
            "payer": SAMPLE_PAYER,
            "user": SAMPLE_USER,
            "mint": SAMPLE_MINT,
            "validator": SAMPLE_PAYER,
        })

        # Undelegate ephemeral ATA permission
        self.test("POST", "/tx/undelegate-ephemeral-ata-permission", {
            "payer": SAMPLE_PAYER,
            "user": SAMPLE_USER,
            "mint": SAMPLE_MINT,
            "magic_context": SAMPLE_MAGIC_CONTEXT,
        })

        # Reset ephemeral ATA permission
        self.test("POST", "/tx/reset-ephemeral-ata-permission", {
            "owner": SAMPLE_USER,
            "user": SAMPLE_USER,
            "mint": SAMPLE_MINT,
            "flags": 0,
        })

    def test_token_endpoints(self):
        """Test token transfer endpoints."""
        print("\n=== Token Transfer Endpoints ===")
        
        # Checked transfer
        self.test("POST", "/tx/checked-transfer", {
            "source": SAMPLE_SOURCE,
            "destination": SAMPLE_DEST,
            "mint": SAMPLE_MINT,
            "amount": 1000000,
            "decimals": 6,
            "authority": SAMPLE_AUTHORITY,
            "token_program": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        })

    def test_ata_endpoints(self):
        """Test Associated Token Account endpoints."""
        print("\n=== Associated Token Account Endpoints ===")
        
        # Initialize ATA
        self.test("POST", "/tx/initialize-ata", {
            "payer": SAMPLE_PAYER,
            "user": SAMPLE_USER,
            "mint": SAMPLE_MINT,
            "ata": SAMPLE_ATA,
        })

    def print_summary(self):
        """Print test summary."""
        print("\n" + "=" * 50)
        print("TEST SUMMARY")
        print("=" * 50)
        
        total = len(self.results)
        passed = sum(1 for r in self.results if r["success"])
        failed = total - passed
        
        print(f"Total Tests: {total}")
        print(f"Passed: {passed}")
        print(f"Failed: {failed}")
        print(f"Success Rate: {(passed/total*100):.1f}%")
        
        if failed > 0:
            print("\nFailed Tests:")
            for result in self.results:
                if not result["success"]:
                    print(f"  - {result['method']} {result['endpoint']}")
                    if "error" in result:
                        print(f"    {result['error']}")

    def close(self):
        """Close the HTTP client."""
        self.client.close()


def main():
    """Run all tests."""
    tester = APITester()

    try:
        # Run all test groups
        tester.test_health_endpoints()
        tester.test_ephemeral_ata_endpoints()
        tester.test_vault_endpoints()
        tester.test_deposit_withdraw_endpoints()
        tester.test_permission_endpoints()
        tester.test_token_endpoints()
        tester.test_ata_endpoints()

        # Print summary
        tester.print_summary()

    finally:
        tester.close()


if __name__ == "__main__":
    main()
