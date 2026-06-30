.PHONY: build test fmt fmt-fix clippy check clean optimize deploy-testnet deploy-mainnet deploy-all deploy-all-with-env deploy-strategy e2e setup upgrade configure add-strategy

# Build the contract WASM
build:
	cargo build --target wasm32-unknown-unknown --release

# Run all tests
test:
	cargo test --all

# Check formatting
fmt:
	cargo fmt --all --check

# Fix formatting
fmt-fix:
	cargo fmt --all

# Run clippy lints
clippy:
	cargo clippy -p august-vault -p xlm-strategy --all-targets -- -D warnings

# Run all checks (fmt + clippy + test + build)
check: fmt clippy test build

# Clean build artifacts
clean:
	cargo clean

# Optimize the WASM binary (requires stellar CLI)
optimize: build
	stellar contract optimize --wasm target/wasm32-unknown-unknown/release/august_vault.wasm

# Deploy (use the deploy script which passes constructor args)
# Usage: ./scripts/deploy.sh <network> <source> <asset-contract-id> [decimals-offset] [name] [symbol]
deploy-testnet deploy-mainnet:
	@echo "Use: ./scripts/deploy.sh <network> <source> <asset-contract-id> [decimals-offset] [name] [symbol]"
	@echo "Example: ./scripts/deploy.sh testnet deployer CDLZ... 6"
	@exit 1

# Deploy vault + strategy to testnet (builds, deploys, configures)
# Usage: make deploy-all [ARGS="--source my-key --symbol avXLM"]
deploy-all:
	./scripts/deploy-all.sh $(ARGS)

# Deploy and write frontend env in one step
# Usage: make deploy-all-with-env [ARGS="--source my-key"]
deploy-all-with-env:
	./scripts/deploy-all.sh --env-file frontend/.env.deploy $(ARGS)

# Run E2E tests against local Stellar network (requires Docker)
e2e:
	./scripts/e2e-test.sh

# Upgrade an existing vault contract
# Usage: make upgrade ARGS="--vault CABC... --network testnet --source deployer"
upgrade:
	./scripts/upgrade.sh $(ARGS)

# Configure vault parameters (AUM limits, operator, pause)
# Usage: make configure ARGS="--vault CABC... --aum-increase-bps 1000 --aum-decrease-bps 500"
configure:
	./scripts/configure.sh $(ARGS)

# Deploy XLM strategy against an existing vault
# Usage: make deploy-strategy ARGS="--vault CABC... --controller GXYZ..."
deploy-strategy:
	./scripts/deploy-strategy.sh $(ARGS)

# Add a strategy subaccount to the vault
# Usage: make add-strategy ARGS="--vault CABC... --strategy CDEF..."
add-strategy:
	./scripts/add-strategy.sh $(ARGS)

# Set up git hooks for pre-commit checks
setup:
	git config core.hooksPath .githooks
	@echo "Git hooks enabled. Pre-commit will run fmt + clippy checks."
