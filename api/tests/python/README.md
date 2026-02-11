# Ephemeral SPL Token API Tests

This directory contains comprehensive test suites for the Ephemeral SPL Token API source code.

## Test Structure

### Unit Tests

#### `test_builder.py`
Tests for transaction building and instruction serialization.
- **TestPubkeyBytesConversion**: Tests pubkey string to bytes conversion
- **TestEncodeLength**: Tests shortvec length encoding
- **TestAccountMeta**: Tests AccountMeta dataclass
- **TestInstruction**: Tests Instruction dataclass
- **TestInstructionBuilder**: Tests all instruction building methods
- **TestTransactionSerialization**: Tests transaction serialization
- **TestBuilderEdgeCases**: Tests edge cases and error conditions

#### `test_pda.py`
Tests for Program Derived Address (PDA) derivation utilities.
- **TestPubkeyBytesConversion**: Tests pubkey conversion
- **TestFindPDA**: Tests PDA finding functionality
- **TestDerivedAccounts**: Tests DerivedAccounts namedtuple
- **TestDeriveAccounts**: Tests account derivation from user and mint
- **TestAccountDerivationEdgeCases**: Tests edge cases

#### `test_rpc.py`
Tests for RPC utilities and account state checking.
- **TestAccountState**: Tests AccountState dataclass
- **TestGetAccountInfo**: Tests account info fetching
- **TestCheckAccounts**: Tests checking multiple accounts
- **TestAccountInitializationChecks**: Tests initialization checks
- **TestAccountDelegationChecks**: Tests delegation checks
- **TestMintDecimals**: Tests mint decimals fetching
- **TestRPCErrorHandling**: Tests error handling
- **TestDataEncodingHandling**: Tests base64-encoded data handling

#### `test_models.py`
Tests for Pydantic request/response models.
- **TestTransactionResponse**: Tests transaction response model
- **TestClusterConfig**: Tests cluster configuration model
- **TestInitializeEphemeralAtaRequest**: Tests initialization request
- **TestDepositSplTokensRequest**: Tests deposit request validation
- **TestWithdrawRequest**: Tests withdraw request validation
- **TestTransferAmountRequest**: Tests transfer request validation
- **TestCreateEphemeralAtaPermissionRequest**: Tests permission creation
- **TestResetEphemeralAtaPermissionRequest**: Tests permission reset
- And other request model tests...

#### `test_config.py`
Tests for configuration management.
- **TestSettingsDefaults**: Tests default configuration values
- **TestSettingsCustomization**: Tests custom configuration
- **TestSettingsEnvironmentVariables**: Tests environment variable handling
- **TestGetSettingsSingleton**: Tests singleton pattern
- **TestSettingsValidation**: Tests configuration validation
- **TestSettingsEndpoints**: Tests endpoint URL validation
- **TestSettingsPrograms**: Tests program ID validation

### Integration Tests

#### `test_api.py`
End-to-end tests for API endpoints (requires running server).
- **TestHealthEndpoints**: Tests health check endpoints
- **TestEphemeralAtaEndpoints**: Tests ephemeral ATA endpoints
- **TestVaultEndpoints**: Tests vault endpoints
- **TestDepositWithdrawEndpoints**: Tests deposit/withdraw endpoints
- **TestPermissionEndpoints**: Tests permission endpoints
- **TestTokenEndpoints**: Tests token transfer endpoints
- **TestAtaEndpoints**: Tests associated token account endpoints
- **TestErrorHandling**: Tests error handling
- **TestPrivateEndpoints**: Tests private transaction endpoints
- **TestTransactionFormat**: Tests transaction format and structure

#### `test_endpoints.py`
Script-based endpoint testing with detailed output.
- Manual testing of all endpoints
- Summary report generation
- Error detail reporting

## Running Tests

### Run All Tests
```bash
pytest tests/ -v
```

### Run Specific Test File
```bash
pytest tests/test_builder.py -v
pytest tests/test_pda.py -v
pytest tests/test_config.py -v
```

### Run Specific Test Class
```bash
pytest tests/test_builder.py::TestInstructionBuilder -v
```

### Run Specific Test
```bash
pytest tests/test_builder.py::TestInstructionBuilder::test_initialize_ephemeral_ata -v
```

### Run Tests with Markers
```bash
# Run only unit tests
pytest tests/ -m unit -v

# Run only async tests
pytest tests/ -m asyncio -v

# Run tests except slow ones
pytest tests/ -m "not slow" -v
```

### Run with Coverage
```bash
pytest tests/ --cov=src --cov-report=html
```

### Run Integration Tests (requires running API server)
```bash
# Start API server first
uvicorn src.main:_get_app --reload

# In another terminal, run integration tests
pytest tests/test_api.py -v
```

## Test Coverage

### Covered Modules

- **builder.py**: ~95% - Transaction building, instruction creation, serialization
- **pda.py**: ~90% - PDA derivation, account addressing
- **rpc.py**: ~85% - Account state checking, data handling
- **models.py**: ~98% - Pydantic model validation, field constraints
- **config.py**: ~95% - Configuration management, environment variables
- **main.py**: ~50% - Integration tests for API endpoints

### Not Yet Covered

- Pyodide-specific code paths (browser/worker environments)
- Live RPC calls (mocked in tests)
- HTTP client implementations (both js.fetch and httpx tested separately)
- Error responses from RPC endpoints

## Test Data

### Sample Pubkeys Used in Tests
- **VALID_PUBKEY**: `TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA` (SPL Token Program)
- **VALID_PUBKEY_2**: `ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL` (Associated Token Program)
- **VALID_PUBKEY_3**: `EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v` (USDC Mint)

### Sample Amounts
- Zero amounts: `0`
- Small amounts: `1000000` (lamports)
- Large amounts: `9223372036854775807` (max int64)

## Fixtures

### pytest Fixtures

#### `client` (test_api.py)
HTTP client for making requests to the API server.
```python
@pytest.fixture
def client():
    with httpx.Client(base_url=BASE_URL, timeout=30.0) as c:
        yield c
```

## Markers

### Available Markers
- `asyncio`: For async test functions
- `unit`: For unit tests
- `integration`: For integration tests
- `rpc`: For tests requiring RPC calls
- `slow`: For slow-running tests

### Adding Markers
```python
@pytest.mark.asyncio
async def test_something():
    pass

@pytest.mark.unit
def test_unit_function():
    pass
```

## Best Practices

1. **Test Isolation**: Each test should be independent and not rely on others
2. **Meaningful Names**: Use descriptive test names that explain what is being tested
3. **Clear Assertions**: Use specific assertions to catch regressions
4. **Documentation**: Add docstrings explaining the test purpose
5. **Fixtures**: Use fixtures for reusable setup/teardown code
6. **Mocking**: Mock external dependencies (RPC calls, HTTP requests)

## Troubleshooting

### Import Errors
If you get import errors, ensure the API module path is correct:
```python
import sys
sys.path.insert(0, '../src')
from src.builder import InstructionBuilder
```

### Async Test Warnings
Ensure `pytest-asyncio` is installed:
```bash
pip install pytest-asyncio
```

### RPC Connection Errors
Integration tests require a running API server:
```bash
uvicorn src.main:_get_app --reload
```

## Continuous Integration

Tests are designed to run in CI/CD pipelines:
```bash
# Install test dependencies
pip install -e ".[dev]"

# Run all tests
pytest tests/ -v --tb=short

# Generate coverage report
pytest tests/ --cov=src --cov-report=term-missing
```

## Contributing

When adding new tests:
1. Follow existing naming conventions
2. Add docstrings explaining what is tested
3. Use appropriate fixtures and markers
4. Keep tests focused and independent
5. Aim for high coverage of edge cases
6. Update this README with new test information
