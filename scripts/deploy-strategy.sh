#!/usr/bin/env bash
# =============================================================================
# XLM Strategy — Standalone Deployment
#
# Deploys the XLM strategy contract against an existing vault and optionally
# whitelists it as a subaccount.
#
# Usage:
#   ./scripts/deploy-strategy.sh [options]
#
# Required:
#   --vault <contract-id>       Vault contract to bind the strategy to
#
# Options:
#   --network <name>            testnet | mainnet (default: testnet)
#   --source <identity>         Stellar identity name (default: deployer)
#   --asset <contract-id>       Override the underlying asset (default: reads from vault)
#   --controller <address>      Strategy controller address (default: same as deployer)
#   --register                  Also whitelist strategy as vault subaccount (requires admin)
#
# Examples:
#   ./scripts/deploy-strategy.sh --vault CABC...
#   ./scripts/deploy-strategy.sh --vault CABC... --controller GXYZ... --register
#   ./scripts/deploy-strategy.sh --vault CABC... --network mainnet --source admin
#
# Prerequisites:
#   - stellar CLI (cargo install stellar-cli --locked)
#   - Rust wasm32-unknown-unknown target (rustup target add wasm32-unknown-unknown)
#   - A funded identity
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# ---------------------------------------------------------------------------
# Defaults
# ---------------------------------------------------------------------------
NETWORK="testnet"
SOURCE="deployer"
VAULT_ID=""
ASSET=""
CONTROLLER=""
REGISTER=false

# Deploy the optimized WASM produced by build.sh. Raw rustc output
# (since 1.95) trips stellar-core's "reference-types not enabled" check
# because call_indirect is emitted with padded LEB128 bytes.
STRATEGY_WASM="$PROJECT_ROOT/target/wasm32-unknown-unknown/release/xlm_strategy.optimized.wasm"

# Temp file for capturing stderr
STDERR_FILE=$(mktemp)
trap 'rm -f "$STDERR_FILE"' EXIT

# ---------------------------------------------------------------------------
# Colors
# ---------------------------------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

log()  { echo -e "${BLUE}[DEPLOY]${NC} $*"; }
ok()   { echo -e "${GREEN}[OK]${NC}     $*"; }
fail() { echo -e "${RED}[ERROR]${NC}  $*"; }

validate_contract_id() {
    local label="$1"
    local value="$2"
    if ! [[ "$value" =~ ^C[A-Z0-9]{55}$ ]]; then
        fail "$label: invalid contract ID: '$value'"
        exit 1
    fi
}

require_arg() {
    if [[ $# -lt 2 || "$2" == --* ]]; then
        fail "$1 requires a value"
        exit 1
    fi
}

# ---------------------------------------------------------------------------
# Parse arguments
# ---------------------------------------------------------------------------
while [[ $# -gt 0 ]]; do
    case "$1" in
        --network)    require_arg "$@"; NETWORK="$2";    shift 2 ;;
        --source)     require_arg "$@"; SOURCE="$2";     shift 2 ;;
        --vault)      require_arg "$@"; VAULT_ID="$2";   shift 2 ;;
        --asset)      require_arg "$@"; ASSET="$2";      shift 2 ;;
        --controller) require_arg "$@"; CONTROLLER="$2"; shift 2 ;;
        --register)   REGISTER=true; shift ;;
        -h|--help)
            sed -n '2,/^# ===/p' "$0" | grep '^#' | sed 's/^# \?//'
            exit 0 ;;
        *)
            fail "Unknown option: $1"
            echo "Run with --help for usage."
            exit 1 ;;
    esac
done

# ---------------------------------------------------------------------------
# Validation
# ---------------------------------------------------------------------------
if [ -z "$VAULT_ID" ]; then
    fail "--vault <contract-id> is required"
    exit 1
fi
validate_contract_id "Vault" "$VAULT_ID"

if [[ "$NETWORK" != "testnet" && "$NETWORK" != "mainnet" ]]; then
    fail "--network must be 'testnet' or 'mainnet' (got: '$NETWORK')"
    exit 1
fi

if [ -n "$CONTROLLER" ]; then
    if ! [[ "$CONTROLLER" =~ ^G[A-Z0-9]{55}$ ]]; then
        fail "--controller: invalid Stellar address: '$CONTROLLER'"
        exit 1
    fi
fi

if [ -n "$ASSET" ]; then
    validate_contract_id "Asset" "$ASSET"
fi

# ---------------------------------------------------------------------------
# Prerequisites
# ---------------------------------------------------------------------------
log "Checking prerequisites..."

if ! command -v stellar &> /dev/null; then
    fail "stellar CLI not found. Install with: cargo install stellar-cli --locked"
    exit 1
fi

if ! stellar keys address "$SOURCE" &>/dev/null; then
    fail "Identity '$SOURCE' not found."
    exit 1
fi

SOURCE_ADDRESS=$(stellar keys address "$SOURCE")
CONTROLLER_ADDRESS="${CONTROLLER:-$SOURCE_ADDRESS}"

ok "Prerequisites met"

# ---------------------------------------------------------------------------
# Resolve asset from vault if not provided
# ---------------------------------------------------------------------------
if [ -z "$ASSET" ]; then
    log "Reading asset from vault $VAULT_ID..."
    if ! ASSET=$(stellar contract invoke \
        --id "$VAULT_ID" \
        --network "$NETWORK" \
        --source-account "$SOURCE" \
        -- \
        asset 2>"$STDERR_FILE"); then
        fail "Could not read asset from vault: $(cat "$STDERR_FILE")"
        fail "Provide the asset explicitly with --asset <contract-id>"
        exit 1
    fi
    ASSET=$(echo "$ASSET" | tr -d '"')
    validate_contract_id "Asset (from vault)" "$ASSET"
fi

ok "Asset: $ASSET"

# ---------------------------------------------------------------------------
# Build WASM
# ---------------------------------------------------------------------------
log "Building XLM strategy..."
"$SCRIPT_DIR/build.sh"

if [ ! -f "$STRATEGY_WASM" ]; then
    fail "Strategy WASM not found at $STRATEGY_WASM"
    exit 1
fi

STRATEGY_SIZE=$(wc -c < "$STRATEGY_WASM")
log "Strategy WASM: $((STRATEGY_SIZE / 1024))KB ($STRATEGY_SIZE bytes)"

# ---------------------------------------------------------------------------
# Deploy
# ---------------------------------------------------------------------------
log "Deploying XLM Strategy..."
log "  Asset:      $ASSET"
log "  Vault:      $VAULT_ID"
log "  Controller: $CONTROLLER_ADDRESS"

if ! STRATEGY_ID=$(stellar contract deploy \
    --wasm "$STRATEGY_WASM" \
    --network "$NETWORK" \
    --source-account "$SOURCE" \
    -- \
    --asset "$ASSET" \
    --vault "$VAULT_ID" \
    --controller "$CONTROLLER_ADDRESS" 2>"$STDERR_FILE"); then
    fail "Strategy deployment failed: $(cat "$STDERR_FILE")"
    exit 1
fi

validate_contract_id "Strategy" "$STRATEGY_ID"
ok "Strategy deployed: $STRATEGY_ID"

# ---------------------------------------------------------------------------
# Register as subaccount (optional)
# ---------------------------------------------------------------------------
if [ "$REGISTER" = true ]; then
    log "Registering strategy as vault subaccount..."
    if ! stellar contract invoke \
        --id "$VAULT_ID" \
        --network "$NETWORK" \
        --source-account "$SOURCE" \
        -- \
        add_subaccount \
        --admin "$SOURCE_ADDRESS" \
        --subaccount "$STRATEGY_ID" >/dev/null 2>"$STDERR_FILE"; then
        fail "Failed to register strategy: $(cat "$STDERR_FILE")"
        fail "The source identity may not be the vault admin."
        exit 1
    fi
    ok "Strategy registered as subaccount"
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}  Strategy Deployment Complete${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo "  Network:    $NETWORK"
echo "  Strategy:   $STRATEGY_ID"
echo "  Vault:      $VAULT_ID"
echo "  Asset:      $ASSET"
echo "  Controller: $CONTROLLER_ADDRESS"
if [ "$REGISTER" = true ]; then
    echo "  Registered: yes"
fi
echo ""
if [ "$REGISTER" = false ]; then
    echo "  To whitelist this strategy in the vault, run:"
    echo "    ./scripts/add-strategy.sh --vault $VAULT_ID --strategy $STRATEGY_ID --network $NETWORK --source $SOURCE"
    echo ""
fi
