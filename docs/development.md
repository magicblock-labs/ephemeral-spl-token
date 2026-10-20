# Program development

Run the commands below from the repository root.

| Task | Command |
| :-- | :-- |
| Build the SBF program | `make build` |
| Run in-process tests | `make test` |
| Exercise live-validator settlement | `make test-e2e` |
| Compare recorded compute units | `make compare` |

## Requirements and build

Use the Rust toolchain pinned in [`rust-toolchain.toml`](../rust-toolchain.toml).
Install Solana CLI and SBF build/test tooling; [CI](../.github/workflows/ci.yml)
uses Solana `3.1.5`. The CLI metadata in [`Cargo.toml`](../Cargo.toml) still lists
`2.3.4`. Use CI as the reference for the tested version. Compute-unit collection
also requires `jq`; the [upgrade workflow](upgrades.md) pins its build toolchain
separately.

```bash
make build
```

The build uses `e-token/Cargo.toml` and produces
`target/deploy/ephemeral_token_program.so`.

## In-process tests

```bash
make test
```

The target runs `cargo test-sbf` with program logging enabled. To select a test:

```bash
cargo test-sbf --manifest-path e-token/Cargo.toml --features logging delegate_ephemeral_ata
```

Tests under [`e-token/tests`](../e-token/tests) exercise accounting, delegation,
permissions, shuttle flows, and queue behavior. Scheduling mocks capture
requests without executing them; use the live-validator suite to check
scheduled settlement.

## Live-validator tests

Install the packaged validators, then run the private-transfer suite:

```bash
npm i -g @magicblock-labs/ephemeral-validator
make test-e2e
```

The target builds the program with logging, starts a base validator and an
Ephemeral Rollup, runs the suite, and stops the validators it started. The tests
follow the private-transfer flow through queued settlement and check the
recipient's base-chain balance.

### Reuse validators

Keep a base validator running across test runs:

```bash
make e2e-base-validator                # terminal 1: stays running
make test-e2e SKIP_BASE_VALIDATOR=1     # terminal 2
```

Use `make e2e-er-validator` with `SKIP_ER_VALIDATOR=1` to reuse the rollup too.
Both skip flags can be combined. Reused validators are left running; otherwise,
a busy port fails startup rather than attaching to an existing instance.
The standalone validator targets reset their configured storage at startup.

### Diagnose a failed run

Retain temporary ledgers and detailed validator logs:

```bash
KEEP_E2E_LOGS=1 E2E_ER_LOG=info make test-e2e
```

Use `make e2e-stop` to stop validators left behind by an interrupted run.

### Scheduling backends

The packaged `ephemeral-validator` executes scheduled tasks without an external
cranker. A `magicblock-validator` setup using Hydra also needs a running cranker
and execution funding; the suite's
[`fund_queue_crank`](../e-token-e2e/src/fixture.rs) helper supplies crank funding
when applicable, but does not start a cranker.

## Compute-unit comparisons

```bash
make test-baseline
# Make the program change, then collect a new measurement.
make test-current
make compare
```

The collection targets replace `metrics/baseline.json` and `metrics/current.json`,
respectively. Override `BASELINE` and `CURRENT` to use other paths:

```bash
make compare BASELINE=metrics/old.json CURRENT=metrics/new.json
```

These are compute-unit measurements from program tests, not end-to-end validator
performance measurements.

## HTTP API development

The Worker has a separate Yarn toolchain and test suite. Follow the
[API setup and verification instructions](../api/README.md).
