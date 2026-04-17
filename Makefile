# CU metrics: compare two JSON files from integration tests (aligned table on stdout).
# compare-metrics always emits ANSI colors.
#
# Examples:
#   make compare
#   make compare BASELINE=metrics/old.json CURRENT=metrics/new.json
#   make compare COMPARE_ARGS='--title "CU diff"'

.PHONY: help compare test-current test-baseline

BASELINE ?= metrics/baseline.json
CURRENT ?= metrics/current.json

help:
	@echo "Targets:"
	@echo "  compare   Run compare-metrics (override BASELINE, CURRENT; optional COMPARE_ARGS e.g. --title T)"
	@echo "  test-current   Run tests with current metrics path"
	@echo "  test-baseline   Run tests with baseline metrics path"

test-current:
	rm -f $(CURRENT)
	METRICS_PATH=$(CURRENT) cargo test-sbf
	jq -S . $(CURRENT) > $(CURRENT).tmp && mv $(CURRENT).tmp $(CURRENT)

test-baseline:
	rm -f $(BASELINE)
	METRICS_PATH=$(BASELINE) cargo test-sbf
	jq -S . $(BASELINE) > $(BASELINE).tmp && mv $(BASELINE).tmp $(BASELINE)

compare:
	cargo run -p compare-metrics -- $(COMPARE_ARGS) $(BASELINE) $(CURRENT)
