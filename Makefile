# CU metrics: compare two JSON files from integration tests (aligned table on stdout).
# ANSI colors when stdout is a terminal; NO_COLOR or --no-color disables colors.
#
# Examples:
#   make compare
#   make compare BASELINE=metrics/old.json CURRENT=metrics/new.json
#   make compare COMPARE_ARGS='--title "CU diff"'

.PHONY: help compare

BASELINE ?= metrics/baseline.json
CURRENT ?= metrics/current.json

help:
	@echo "Targets:"
	@echo "  compare   Run compare-metrics (override BASELINE, CURRENT; optional COMPARE_ARGS e.g. --title T)"
	@echo "  test-current   Run tests with current metrics path"
	@echo "  test-baseline   Run tests with baseline metrics path"

test-current:
	METRICS_PATH=metrics/current.json cargo test-sbf

test-baseline:
	METRICS_PATH=metrics/baseline.json cargo test-sbf

compare:
	cargo run -p compare-metrics -- $(COMPARE_ARGS) $(BASELINE) $(CURRENT)
