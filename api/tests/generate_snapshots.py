#!/usr/bin/env python3
"""
Generate transaction snapshots for deterministic testing.

This script generates the expected transaction outputs with a fixed blockhash.
Run this BEFORE refactoring to capture baseline, then run tests to verify.

Usage:
    cd api/tests
    python generate_snapshots.py

    # Or to see what would be generated without saving:
    python generate_snapshots.py --dry-run
"""

import json
import sys
from pathlib import Path
from unittest.mock import patch, AsyncMock

# Add parent to path for imports
sys.path.insert(0, str(Path(__file__).parent.parent))

# Fixed blockhash - must match test_snapshots.py
FIXED_BLOCKHASH = b'\x00' * 32

# Import test inputs
from tests.test_snapshots import TEST_INPUTS

SNAPSHOTS_FILE = Path(__file__).parent / "snapshots.json"


def generate_snapshots(dry_run: bool = False) -> dict:
    """Generate snapshots for all test cases."""
    # Mock blockhash before importing app
    with patch('src.builder._get_latest_blockhash', new_callable=AsyncMock) as mock:
        mock.return_value = FIXED_BLOCKHASH

        from fastapi.testclient import TestClient
        from src.main import create_app

        app = create_app()
        client = TestClient(app)

        snapshots = {}
        errors = []

        for case_name, case in TEST_INPUTS.items():
            endpoint = case["endpoint"]
            payload = case["payload"]

            try:
                response = client.post(endpoint, json=payload)
                if response.status_code != 200:
                    errors.append(f"{case_name}: HTTP {response.status_code} - {response.json()}")
                    continue

                tx = response.json()["transaction"]
                snapshots[case_name] = tx
                print(f"✓ {case_name}: {len(tx)} chars")

            except Exception as e:
                errors.append(f"{case_name}: {e}")

        if errors:
            print("\nErrors:")
            for err in errors:
                print(f"  ✗ {err}")

        return snapshots


def main():
    dry_run = "--dry-run" in sys.argv

    print("Generating transaction snapshots...")
    print(f"Using fixed blockhash: {FIXED_BLOCKHASH.hex()[:16]}...")
    print()

    snapshots = generate_snapshots(dry_run)

    if not snapshots:
        print("No snapshots generated!")
        sys.exit(1)

    if dry_run:
        print("\n[DRY RUN] Would write to snapshots.json:")
        print(json.dumps(snapshots, indent=2)[:500] + "...")
    else:
        with open(SNAPSHOTS_FILE, "w") as f:
            json.dump(snapshots, f, indent=2)
        print(f"\n✓ Wrote {len(snapshots)} snapshots to {SNAPSHOTS_FILE}")

    print(f"\nTotal: {len(snapshots)} test cases")


if __name__ == "__main__":
    main()
