# MacInformer — standardized build, lint, test, drift targets.
# Zero external dependencies: only cargo, curl, and sh are required.

CARGO ?= cargo
DARWIN_TARGET := aarch64-apple-darwin

.PHONY: all check fmt lint lint-darwin test build release drift reviews clean

all: fmt lint test build

check:
	$(CARGO) check --workspace

fmt:
	$(CARGO) fmt --check

lint:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

lint-darwin:
	$(CARGO) clippy --workspace --all-targets --target $(DARWIN_TARGET) -- -D warnings

test:
	$(CARGO) test --workspace

build:
	$(CARGO) build --workspace

release:
	$(CARGO) build --release --workspace

drift: release
	tools/drift.sh

# The full merge/push gate (DONE.md)
gate: fmt lint lint-darwin test release drift
	@echo "GATE: all green"

clean:
	$(CARGO) clean
