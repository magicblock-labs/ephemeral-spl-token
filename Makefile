# CU metrics: compare two JSON files from integration tests (aligned table on stdout).
# compare-metrics always emits ANSI colors.
#
# Examples:
#   make compare
#   make compare BASELINE=metrics/old.json CURRENT=metrics/new.json
#   make compare COMPARE_ARGS='--title "CU diff"'

SHELL := /bin/bash

.PHONY: help compare test-current test-baseline build build-e2e test lint fmt \
        test-e2e e2e-base-validator e2e-er-validator e2e-stop

BASELINE ?= metrics/baseline.json
CURRENT ?= metrics/current.json

# --- e2e configuration ------------------------------------------------------
# Set SKIP_BASE_VALIDATOR=1 / SKIP_ER_VALIDATOR=1 to reuse a validator that is
# already running instead of having the test spawn (and later kill) its own.
SKIP_BASE_VALIDATOR ?=
SKIP_ER_VALIDATOR ?=
E2E_BASE_RPC_PORT ?= 7101
E2E_ER_RPC_PORT ?= 7799
E2E_ARGS ?=

E_TOKEN_PROGRAM_ID := SPLxh1LVZzEkX99H6rqYizhytLWPZVV296zyYDPagv2
PERMISSION_PROGRAM_ID := ACLseoPoyC3cBqoUtkbjZ4aDrkurZW86v19pXz2XQnp1
HYDRA_EPHEMERAL_PROGRAM_ID := eHyd5BU8QffvHi4GnXwxrK4WpS7pM2x9UGKHBWii7mf

E_TOKEN_SO := target/deploy/ephemeral_token_program.so
PERMISSION_SO := e-token/tests/fixtures/acl.so
HYDRA_EPHEMERAL_SO := e-token/tests/fixtures/hydra_ephemeral.so

PROGRAM_MANIFEST := e-token/Cargo.toml

E2E_LEDGER ?= .e2e/base-ledger
E2E_ER_STORAGE ?= .e2e/er-storage

help:
	@echo "Targets:"
	@echo "  build               cargo build-sbf"
	@echo "  test                Run the in-process (program-test) suite"
	@echo "  test-e2e            Run the live-validator e2e test (spawns both validators)"
	@echo "  e2e-base-validator  Run a base validator in the foreground, for reuse across e2e runs"
	@echo "  e2e-er-validator    Run an ephemeral validator in the foreground, for reuse across e2e runs"
	@echo "  e2e-stop            Kill any validators left behind by an interrupted e2e run"
	@echo "  compare             Run compare-metrics (override BASELINE, CURRENT; optional COMPARE_ARGS e.g. --title T)"
	@echo "  test-current        Run tests with current metrics path"
	@echo "  test-baseline       Run tests with baseline metrics path"
	@echo ""
	@echo "e2e flags (reuse an already-running validator instead of spawning one):"
	@echo "  make test-e2e SKIP_BASE_VALIDATOR=1"
	@echo "  make test-e2e SKIP_ER_VALIDATOR=1"
	@echo "  make test-e2e SKIP_BASE_VALIDATOR=1 SKIP_ER_VALIDATOR=1"

test-current:
	rm -f $(CURRENT)
	METRICS_PATH=$(CURRENT) cargo test-sbf --manifest-path $(PROGRAM_MANIFEST)
	jq -S . $(CURRENT) > $(CURRENT).tmp && mv $(CURRENT).tmp $(CURRENT)

test-baseline:
	rm -f $(BASELINE)
	METRICS_PATH=$(BASELINE) cargo test-sbf --manifest-path $(PROGRAM_MANIFEST)
	jq -S . $(BASELINE) > $(BASELINE).tmp && mv $(BASELINE).tmp $(BASELINE)

compare:
	cargo run -p compare-metrics -- $(COMPARE_ARGS) $(BASELINE) $(CURRENT)

build:
	@cargo build-sbf --manifest-path $(PROGRAM_MANIFEST)

build-e2e:
	@cargo build-sbf --manifest-path $(PROGRAM_MANIFEST) --features logging

test:
	RUST_LOG=off cargo test-sbf --manifest-path $(PROGRAM_MANIFEST) --features logging

# --- e2e --------------------------------------------------------------------
#
# The suite drives the real two-validator stack (mb-test-validator +
# ephemeral-validator, both from the @magicblock-labs/ephemeral-validator npm
# package). By default it starts and stops both itself.
#
# Spawning a base validator dominates the runtime, so for a tight edit/test loop
# leave one running and reuse it:
#
#   make e2e-base-validator                       # terminal 1
#   make test-e2e SKIP_BASE_VALIDATOR=1           # terminal 2, repeatedly
#
# The same works for the rollup (e2e-er-validator / SKIP_ER_VALIDATOR=1). When a
# validator is skipped the test attaches to whatever is listening on its port and
# leaves it running afterwards; when it is not skipped, the test refuses to start
# if the port is already busy rather than silently reusing a stale instance.
test-e2e: build-e2e
	E2E_SKIP_BASE_VALIDATOR=$(SKIP_BASE_VALIDATOR) \
	E2E_SKIP_ER_VALIDATOR=$(SKIP_ER_VALIDATOR) \
	E2E_BASE_RPC_PORT=$(E2E_BASE_RPC_PORT) \
	E2E_ER_RPC_PORT=$(E2E_ER_RPC_PORT) \
	cargo test -p ephemeral-token-e2e --test private_transfer_flow $(E2E_ARGS) -- --ignored --nocapture --test-threads=1

# A standalone base validator with the same genesis programs the suite expects.
# Keep it running and pass SKIP_BASE_VALIDATOR=1 to test-e2e.
e2e-base-validator: build-e2e
	@mkdir -p $(dir $(E2E_LEDGER))
	mb-test-validator \
		--reset \
		--ledger $(E2E_LEDGER) \
		--rpc-port $(E2E_BASE_RPC_PORT) \
		--bind-address 127.0.0.1 \
		--bpf-program $(E_TOKEN_PROGRAM_ID) $(E_TOKEN_SO) \
		--bpf-program $(PERMISSION_PROGRAM_ID) $(PERMISSION_SO) \
		--bpf-program $(HYDRA_EPHEMERAL_PROGRAM_ID) $(HYDRA_EPHEMERAL_SO)

# A standalone ephemeral rollup pointed at the base validator above.
# Keep it running and pass SKIP_ER_VALIDATOR=1 to test-e2e.
e2e-er-validator:
	@mkdir -p $(dir $(E2E_ER_STORAGE))
	ephemeral-validator \
		--no-tui \
		--reset \
		--lifecycle ephemeral \
		--remotes http://127.0.0.1:$(E2E_BASE_RPC_PORT) \
		--remotes ws://127.0.0.1:$$(($(E2E_BASE_RPC_PORT) + 1)) \
		--listen 127.0.0.1:$(E2E_ER_RPC_PORT) \
		--storage $(E2E_ER_STORAGE)

# Clean up validators an interrupted run may have orphaned.
# The npm wrappers re-spawn the real binaries under different names
# (`ephemeral-validator` -> `magicblock-validator`, `mb-test-validator` ->
# `solana-test-validator`), so matching only the wrappers leaves the actual
# validator holding the port.
E2E_PROCESS_PATTERN := [m]b-test-validator|[e]phemeral-validator|[m]agicblock-validator|[s]olana-test-validator

e2e-stop:
	-@pkill -INT -f '$(E2E_PROCESS_PATTERN)' 2>/dev/null || true
	@sleep 1
	-@pkill -KILL -f '$(E2E_PROCESS_PATTERN)' 2>/dev/null || true
	@echo "stopped any running e2e validators"

lint:
	cargo clippy -- -D warnings

fmt:
	cargo +nightly fmt
