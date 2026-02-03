"""Pytest configuration for API tests."""

import sys
from pathlib import Path

# Add the api directory to Python path so 'src' module can be imported
api_dir = Path(__file__).parent.parent
sys.path.insert(0, str(api_dir))
