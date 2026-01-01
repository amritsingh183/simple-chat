.PHONY: all setup check check-strict fmt lint test audit udeps outdated deny install-hook fix clean build build-server build-client help ci upgrade upgrade-all

all: check

help:
	@echo 'Usage:'
	@echo '  make setup         Install required tools (cargo-run-bin, pinned nightly)'
	@echo '  make check         Run all checks (fmt, lint, test, audit, udeps)'
	@echo '  make check-strict  Run critical checks only (CI gate)'
	@echo '  make install-hook  Install git pre-commit hook'
	@echo '  make fix           Auto-fix formatting and clippy issues'
	@echo '  make ci            Full CI pipeline with strict checks'

setup:
	@echo "Setting up environment..."
	@command -v cargo-run-bin >/dev/null || cargo install cargo-run-bin
	@cargo bin --sync-aliases
	@rustup toolchain install nightly
	@echo "Setup complete. Toolchain pinned to 1.92.0 via rust-toolchain.toml"

install-hook:
	@echo "Installing git pre-commit hook..."
	@mkdir -p .git/hooks
	@echo "#!/bin/bash" > .git/hooks/pre-commit
	@echo "set -e" >> .git/hooks/pre-commit
	@echo "echo 'Running pre-commit checks...'" >> .git/hooks/pre-commit
	@echo "make check-strict" >> .git/hooks/pre-commit
	@chmod +x .git/hooks/pre-commit
	@echo "Hook installed."

fix:
	@echo "Auto-fixing issues..."
	@cargo +nightly fmt --all
	@cargo clippy --fix --allow-dirty --allow-staged --all-targets --all-features

fmt:
	@echo "Checking formatting..."
	@cargo +nightly fmt --all -- --check

lint:
	@echo "Linting..."
	@cargo clippy --all-targets --all-features

test:
	@echo "Testing..."
	@cargo test --all-features

build-server: check
	@echo "Building server..."
	@cargo build --release -p server

build-client: check
	@echo "Building client..."
	@cargo build --release -p client

build: build-server build-client
	@echo "Build complete."

audit:
	@echo "Auditing dependencies..."
	@cargo bin cargo-audit

deny:
	@echo "Checking dependency policy..."
	@cargo bin cargo-deny check

udeps:
	@echo "Checking unused dependencies..."
	@cargo +nightly bin cargo-udeps --all-targets

outdated:
	@echo "Checking outdated dependencies..."
	@cargo bin cargo-outdated

upgrade: # For hobby projects only. Production systems must always pin versions.
	@echo "Upgrading dependencies to latest compatible versions..."
	@cargo update
	@echo "Dependencies in Cargo.lock updated."

upgrade-all: # For hobby projects only. Production systems must always pin versions.
	@echo "Upgrading ALL dependencies to latest versions (including breaking)..."
	@command -v cargo-upgrade >/dev/null || cargo install cargo-edit
	@cargo upgrade --workspace --incompatible
	@cargo update
	@echo "Cargo.toml and Cargo.lock updated. Review changes!"

check-strict: fmt lint test audit
	@echo "Critical checks passed!"

check: check-strict udeps outdated fix
	@echo "All checks passed!"

ci: fmt lint test audit deny udeps build
	@echo "Checking outdated dependencies (strict)..."
	@cargo bin cargo-outdated --exit-code 1
	@echo "CI checks passed!"

clean:
	@echo "Cleaning..."
	@cargo clean

integration-test: build-release
	@echo "Building integration test binary..."
	@go build -o scripts/integration-tests scripts/integration-tests.go
	@echo "Running integration tests..."
	@./scripts/integration-tests

build-release:
	@echo "Building release binaries (fast)..."
	@cargo build --release -p server
	@cargo build --release -p client
