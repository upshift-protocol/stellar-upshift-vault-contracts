# Stellar Vault Program

A share-based vault contract for Stellar Soroban, built on the [OpenZeppelin Stellar Contracts](https://github.com/OpenZeppelin/stellar-contracts) library.

Users deposit a Stellar asset into the vault and receive shares proportional to their ownership. The contract follows the [ERC-4626 Tokenized Vault Standard](https://eips.ethereum.org/EIPS/eip-4626) pattern.

---

## Features

- Accepts a single configurable deposit token (Stellar asset or SAC), set at deployment
- Users receive shares for deposits and burn shares for withdrawals
- Configurable virtual decimals offset for inflation attack protection
- Share/asset conversion functions for UI integration
- Automatic instance TTL extension on every deposit, mint, withdraw, and redeem
- Built on OpenZeppelin's `stellar-tokens` library

---

## Architecture

The contract is a thin wrapper around OpenZeppelin's `stellar-tokens` vault library, implementing the `FungibleToken` and `FungibleVault` traits:

```
┌─────────────────────────────────┐
│         August Vault            │
│    (FungibleToken + Vault)      │
├─────────────────────────────────┤
│  Constructor args:              │
│  - name, symbol (share token)   │
│  - asset (underlying token)     │
│  - decimals_offset              │
├─────────────────────────────────┤
│  Delegates to:                  │
│  stellar-tokens::vault::Vault   │
└─────────────────────────────────┘
```

---

## Contract Interface

### Deposit / Withdraw

| Function | Description |
| --- | --- |
| `deposit(assets, receiver, from, operator)` | Deposit assets, mint shares to receiver |
| `mint(shares, receiver, from, operator)` | Mint exact shares, pull required assets |
| `withdraw(assets, receiver, owner, operator)` | Burn shares, send exact assets to receiver |
| `redeem(shares, receiver, owner, operator)` | Burn exact shares, send proportional assets |

### View Functions

| Function | Description |
| --- | --- |
| `query_asset()` | Underlying asset contract address |
| `total_assets()` | Total assets held by the vault |
| `total_supply()` | Total share supply |
| `balance(account)` | Share balance of an account |
| `convert_to_shares(assets)` | Convert asset amount to share equivalent |
| `convert_to_assets(shares)` | Convert share amount to asset equivalent |
| `preview_deposit(assets)` | Preview shares for a given deposit |
| `preview_mint(shares)` | Preview assets required for minting shares |
| `preview_withdraw(assets)` | Preview shares burned for a withdrawal |
| `preview_redeem(shares)` | Preview assets received for redeeming shares |
| `max_deposit(receiver)` | Maximum depositable assets |
| `max_mint(receiver)` | Maximum mintable shares |
| `max_withdraw(owner)` | Maximum withdrawable assets |
| `max_redeem(owner)` | Maximum redeemable shares |
| `decimals()` | Share token decimals (asset decimals + offset) |
| `name()` | Share token name |
| `symbol()` | Share token symbol |

### Maintenance

| Function | Description |
| --- | --- |
| `extend_ttl()` | Extends the contract instance TTL to prevent expiration (permissionless, fallback for quiet periods) |

---

## Development

### Prerequisites

- [Rust](https://rustup.rs/) (stable, with `wasm32-unknown-unknown` target)
- [Stellar CLI](https://developers.stellar.org/docs/tools/developer-tools/cli/install-cli) (for deployment and optimization)

```bash
# Install the WASM target
rustup target add wasm32-unknown-unknown

# Install Stellar CLI
cargo install stellar-cli --locked
```

### Build

```bash
# Build the WASM contract
make build

# Or using the build script
./scripts/build.sh
```

### Test

```bash
# Run all tests
make test

# Run tests with output
./scripts/test.sh

# Run a specific test
cargo test test_vault_deposit -- --nocapture
```

### Lint

```bash
# Check formatting
make fmt

# Fix formatting
make fmt-fix

# Run clippy
make clippy

# Run all checks (fmt + clippy + test + build)
make check
```

---

## Deployment

The vault constructor takes four arguments: `name`, `symbol`, `asset`, and `decimals_offset`.

### Testnet

```bash
# 1. Configure a Stellar identity
stellar keys generate deployer --network testnet

# 2. Fund the account
stellar keys fund deployer --network testnet

# 3. Build and deploy
./scripts/deploy.sh testnet deployer <ASSET_CONTRACT_ID> 6

# With custom share token name and symbol
./scripts/deploy.sh testnet deployer <ASSET_CONTRACT_ID> 6 "My Vault Shares" "avXLM"
```

### Mainnet

```bash
# 1. Ensure you have a funded identity configured
stellar keys add deployer --secret-key

# 2. Deploy
./scripts/deploy.sh mainnet deployer <ASSET_CONTRACT_ID> 6
```

Deploy script parameters:
- **network**: `testnet` or `mainnet`
- **source**: Stellar identity name
- **asset**: Contract address of the underlying token
- **decimals_offset**: Virtual decimals offset (default: 6, max: 10)
- **name**: Share token name (default: "August Vault Shares")
- **symbol**: Share token symbol (default: "avVAULT")

### Post-Deployment Configuration

```bash
# Set operator and AUM limits
./scripts/configure.sh --vault <VAULT_ID> --network testnet --source deployer \
    --operator <OPERATOR_ADDRESS> \
    --aum-increase-bps 1000 --aum-decrease-bps 500

# Add a strategy subaccount
./scripts/add-strategy.sh --vault <VAULT_ID> --network testnet --source deployer \
    --strategy <STRATEGY_CONTRACT_ID>
```

### Upgrade

```bash
# Automated: build, install, verify hash, upgrade, verify state
./scripts/upgrade.sh --vault <VAULT_ID> --network testnet --source deployer
```

For the full deployment parameters and upgrade procedure, see the script usage in [Deployment](#deployment) above and the inline `--help` for each script in `scripts/`.

---

## Project Structure

```
stellar-upshift-vault-contracts/
├── .github/
│   └── workflows/
│       └── ci.yml                  # CI pipeline (fmt, clippy, test, build, e2e, coverage)
├── contracts/
│   ├── august-vault/
│   │   ├── Cargo.toml              # Contract dependencies
│   │   └── src/
│   │       ├── lib.rs              # Module declarations
│   │       ├── contract.rs         # Vault contract (FungibleToken + FungibleVault)
│   │       ├── strategy.rs         # Strategy subaccount logic
│   │       ├── storage.rs          # Storage keys & TTL management
│   │       ├── events.rs           # Contract events
│   │       ├── errors.rs           # Error definitions
│   │       └── test.rs             # Unit tests
│   └── xlm-strategy/               # XLM strategy contract
│       ├── Cargo.toml
│       ├── README.md
│       └── src/
├── scripts/
│   ├── build.sh                    # Build and optimize WASM
│   ├── test.sh                     # Run tests
│   ├── deploy.sh                   # Deploy to testnet/mainnet
│   ├── deploy-all.sh               # Full testnet deployment (vault + strategy)
│   ├── deploy-strategy.sh          # Deploy a strategy contract
│   ├── configure.sh                # Post-deployment configuration
│   ├── add-strategy.sh             # Add strategy subaccount
│   ├── add-wallet.sh               # Add a wallet to the vault
│   ├── upgrade.sh                  # Contract upgrade with hash verification
│   └── e2e-test.sh                 # End-to-end test suite
├── Cargo.toml                      # Workspace configuration
├── Cargo.lock                      # Pinned dependency versions
├── Makefile                        # Common commands
├── deny.toml                       # cargo-deny config (advisories, licenses, bans)
├── rust-toolchain.toml             # Rust toolchain configuration
└── README.md
```
