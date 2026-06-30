#!/usr/bin/env bash
# =============================================================================
# August Vault — Full Deployment
#
# Deploys the vault (and optionally the XLM strategy) to Stellar testnet or
# mainnet and configures it end-to-end: wraps or reuses the asset token,
# deploys the contract, sets the operator, and whitelists any strategy.
#
# Usage:
#   ./scripts/deploy-all.sh [options]
#
# Options:
#   --network <name>        testnet | mainnet (default: testnet)
#   --source <identity>     Stellar identity name (default: deployer)
#   --asset <contract-id>   Use an existing token contract instead of wrapping native XLM
#   --decimals-offset <n>   Virtual decimals offset, 1-10 (default: 6)
#   --name <string>         Share token name (default: "August Vault Shares")
#   --symbol <string>       Share token symbol (default: "avVAULT")
#   --operator <address>    Separate operator address (default: same as deployer)
#   --controller <address>  Strategy controller address (default: same as deployer)
#   --no-strategy           Skip XLM strategy deployment
#   --env-file <path>       Write frontend .env snippet to this file
#
# Examples:
#   ./scripts/deploy-all.sh
#   ./scripts/deploy-all.sh --network mainnet --source admin --asset CDLZFC...
#   ./scripts/deploy-all.sh --source my-key --symbol "avXLM"
#   ./scripts/deploy-all.sh --asset CDLZFC... --no-strategy
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
ASSET=""
DECIMALS_OFFSET=6
NAME="August Vault Shares"
SYMBOL="avVAULT"
OPERATOR=""
CONTROLLER=""
DEPLOY_STRATEGY=true
ENV_FILE=""

# Deploy the optimized WASMs produced by build.sh. Raw rustc output
# (since 1.95) trips stellar-core's "reference-types not enabled" check
# because call_indirect is emitted with padded LEB128 bytes.
VAULT_WASM="$PROJECT_ROOT/target/wasm32-unknown-unknown/release/august_vault.optimized.wasm"
STRATEGY_WASM="$PROJECT_ROOT/target/wasm32-unknown-unknown/release/xlm_strategy.optimized.wasm"

# Temp file for capturing stderr from invocations
STDERR_FILE=$(mktemp)
trap 'rm -f "$STDERR_FILE"' EXIT

# ---------------------------------------------------------------------------
# Colors
# ---------------------------------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log()  { echo -e "${BLUE}[DEPLOY]${NC} $*"; }
ok()   { echo -e "${GREEN}[OK]${NC}     $*"; }
fail() { echo -e "${RED}[ERROR]${NC}  $*"; }
warn() { echo -e "${YELLOW}[WARN]${NC}   $*"; }

phase() {
    echo ""
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${BLUE}  Step $1: $2${NC}"
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
}

validate_contract_id() {
    local label="$1"
    local value="$2"
    if ! [[ "$value" =~ ^C[A-Z0-9]{55}$ ]]; then
        fail "$label: invalid contract ID: '$value'"
        exit 1
    fi
}

# Require that a flag has a value argument following it.
require_arg() {
    if [[ $# -lt 2 || "$2" == --* ]]; then
        fail "$1 requires a value"
        exit 1
    fi
}

# Invoke a read-only function on the vault contract, capturing stderr.
# Usage: result=$(invoke_vault <function> [args...])
invoke_vault() {
    local function_name="$1"
    shift
    local result
    if ! result=$(stellar contract invoke \
        --id "$VAULT_ID" \
        --network "$NETWORK" \
        --source-account "$SOURCE" \
        -- \
        "$function_name" "$@" 2>"$STDERR_FILE"); then
        fail "invoke $function_name failed: $(cat "$STDERR_FILE")"
        return 1
    fi
    echo "$result" | tr -d '"'
}

# ---------------------------------------------------------------------------
# Parse arguments
# ---------------------------------------------------------------------------
while [[ $# -gt 0 ]]; do
    case "$1" in
        --network)         require_arg "$@"; NETWORK="$2";         shift 2 ;;
        --source)          require_arg "$@"; SOURCE="$2";          shift 2 ;;
        --asset)           require_arg "$@"; ASSET="$2";           shift 2 ;;
        --decimals-offset) require_arg "$@"; DECIMALS_OFFSET="$2"; shift 2 ;;
        --name)            require_arg "$@"; NAME="$2";            shift 2 ;;
        --symbol)          require_arg "$@"; SYMBOL="$2";          shift 2 ;;
        --operator)        require_arg "$@"; OPERATOR="$2";        shift 2 ;;
        --controller)      require_arg "$@"; CONTROLLER="$2";      shift 2 ;;
        --no-strategy)     DEPLOY_STRATEGY=false; shift ;;
        --env-file)        require_arg "$@"; ENV_FILE="$2";        shift 2 ;;
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
# Network validation
# ---------------------------------------------------------------------------
if [[ "$NETWORK" != "testnet" && "$NETWORK" != "mainnet" ]]; then
    fail "--network must be 'testnet' or 'mainnet' (got: '$NETWORK')"
    exit 1
fi

# ---------------------------------------------------------------------------
# Mainnet guards
# ---------------------------------------------------------------------------
if [ "$NETWORK" = "mainnet" ]; then
    # Mainnet requires an explicit asset — wrapping native XLM automatically
    # is too risky for production.
    if [ -z "$ASSET" ]; then
        fail "Mainnet deployment requires --asset <contract-id> (no automatic XLM wrapping)."
        exit 1
    fi

    # Require explicit --controller for mainnet strategy deployments.
    if [ "$DEPLOY_STRATEGY" = true ] && [ -z "$CONTROLLER" ]; then
        fail "Mainnet strategy deployment requires --controller <address>. Use --no-strategy to skip, or provide a controller address."
        exit 1
    fi
fi

# ---------------------------------------------------------------------------
# Prerequisites
# ---------------------------------------------------------------------------
phase 1 "Check prerequisites"

if ! command -v stellar &> /dev/null; then
    fail "stellar CLI not found. Install with: cargo install stellar-cli --locked"
    exit 1
fi
log "stellar CLI: $(stellar --version 2>/dev/null || echo 'unknown')"

if ! rustup target list --installed | grep -q wasm32-unknown-unknown; then
    fail "wasm32-unknown-unknown target not installed. Run: rustup target add wasm32-unknown-unknown"
    exit 1
fi

# Verify identity exists and is funded
if ! stellar keys address "$SOURCE" &>/dev/null; then
    fail "Identity '$SOURCE' not found."
    echo ""
    if [ "$NETWORK" = "mainnet" ]; then
        echo "  Import your key with:"
        echo "    stellar keys add $SOURCE --secret-key"
    else
        echo "  Create and fund it with:"
        echo "    stellar keys generate $SOURCE --network $NETWORK"
        echo "    stellar keys fund $SOURCE --network $NETWORK"
    fi
    exit 1
fi

SOURCE_ADDRESS=$(stellar keys address "$SOURCE")
log "Source identity: $SOURCE ($SOURCE_ADDRESS)"

# Validate --env-file path early, before any deployments
if [ -n "$ENV_FILE" ]; then
    ENV_DIR=$(dirname "$ENV_FILE")
    if [ ! -d "$ENV_DIR" ]; then
        fail "Directory for --env-file does not exist: $ENV_DIR"
        exit 1
    fi
fi

# Validate --operator address format if provided
if [ -n "$OPERATOR" ]; then
    if ! [[ "$OPERATOR" =~ ^G[A-Z0-9]{55}$ ]]; then
        fail "--operator: invalid Stellar address: '$OPERATOR' (expected G... public key)"
        exit 1
    fi
fi

# Validate --controller address format if provided
if [ -n "$CONTROLLER" ]; then
    if ! [[ "$CONTROLLER" =~ ^G[A-Z0-9]{55}$ ]]; then
        fail "--controller: invalid Stellar address: '$CONTROLLER' (expected G... public key)"
        exit 1
    fi
fi

# Validate --decimals-offset is an integer in range 3-10
# Minimum of 3 is required by the contract to mitigate the first-depositor
# share inflation attack (see contract.rs __constructor()).
if ! [[ "$DECIMALS_OFFSET" =~ ^[0-9]+$ ]] || [ "$DECIMALS_OFFSET" -lt 3 ] || [ "$DECIMALS_OFFSET" -gt 10 ]; then
    fail "--decimals-offset must be an integer between 3 and 10 (got: '$DECIMALS_OFFSET')"
    exit 1
fi

ok "All prerequisites met"

# ---------------------------------------------------------------------------
# Build WASMs
# ---------------------------------------------------------------------------
phase 2 "Build contracts"

log "Building WASMs..."
"$SCRIPT_DIR/build.sh"

if [ ! -f "$VAULT_WASM" ]; then
    fail "Vault WASM not found at $VAULT_WASM"
    exit 1
fi

if [ "$DEPLOY_STRATEGY" = true ] && [ ! -f "$STRATEGY_WASM" ]; then
    fail "Strategy WASM not found at $STRATEGY_WASM"
    exit 1
fi

VAULT_SIZE=$(wc -c < "$VAULT_WASM")
log "Vault WASM: $((VAULT_SIZE / 1024))KB ($VAULT_SIZE bytes)"

if [ "$DEPLOY_STRATEGY" = true ]; then
    STRATEGY_SIZE=$(wc -c < "$STRATEGY_WASM")
    log "Strategy WASM: $((STRATEGY_SIZE / 1024))KB ($STRATEGY_SIZE bytes)"
fi

ok "Build complete"

# ---------------------------------------------------------------------------
# Deploy or reuse asset token
# ---------------------------------------------------------------------------
phase 3 "Asset token"

if [ -n "$ASSET" ]; then
    log "Using provided asset contract: $ASSET"
    TOKEN_ID="$ASSET"
    validate_contract_id "Asset" "$TOKEN_ID"
else
    log "Wrapping native XLM as Stellar Asset Contract..."
    if TOKEN_STDOUT=$(stellar contract asset deploy \
        --asset native \
        --source-account "$SOURCE" \
        --network "$NETWORK" 2>"$STDERR_FILE"); then
        TOKEN_ID="$TOKEN_STDOUT"
    else
        STDERR_CONTENT=$(cat "$STDERR_FILE")
        if echo "$STDERR_CONTENT" | grep -q 'already exists'; then
            TOKEN_ID=$(echo "$STDERR_CONTENT" | grep -oE 'C[A-Z0-9]{55}' | head -1)
            if [ -z "$TOKEN_ID" ]; then
                fail "Token already exists but could not extract contract ID from:"
                fail "  $STDERR_CONTENT"
                exit 1
            fi

            # Verify extracted ID matches the canonical native XLM SAC
            if EXPECTED_TOKEN_ID=$(stellar contract asset id \
                --asset native \
                --network "$NETWORK" 2>/dev/null); then
                if [ "$TOKEN_ID" != "$EXPECTED_TOKEN_ID" ]; then
                    fail "Extracted token ID ($TOKEN_ID) does not match canonical native XLM SAC ($EXPECTED_TOKEN_ID)"
                    fail "This may indicate a CLI output format change. Use --asset $EXPECTED_TOKEN_ID explicitly."
                    exit 1
                fi
            else
                warn "Could not verify extracted token ID via 'stellar contract asset id'. Proceeding with: $TOKEN_ID"
            fi

            log "Native XLM SAC already deployed, reusing existing contract"
        else
            fail "Asset deploy failed: $STDERR_CONTENT"
            exit 1
        fi
    fi
    validate_contract_id "Token" "$TOKEN_ID"
fi

ok "Asset token: $TOKEN_ID"

# ---------------------------------------------------------------------------
# Deploy vault
# ---------------------------------------------------------------------------
phase 4 "Deploy vault contract"

log "Deploying August Vault..."
log "  Name:            $NAME"
log "  Symbol:          $SYMBOL"
log "  Asset:           $TOKEN_ID"
log "  Decimals Offset: $DECIMALS_OFFSET"
log "  Admin:           $SOURCE_ADDRESS"

if ! VAULT_ID=$(stellar contract deploy \
    --wasm "$VAULT_WASM" \
    --network "$NETWORK" \
    --source-account "$SOURCE" \
    -- \
    --name "$NAME" \
    --symbol "$SYMBOL" \
    --asset "$TOKEN_ID" \
    --decimals_offset "$DECIMALS_OFFSET" \
    --admin "$SOURCE_ADDRESS" 2>"$STDERR_FILE"); then
    fail "Vault deployment failed: $(cat "$STDERR_FILE")"
    fail "Check that the source account is funded and the token contract ID is valid."
    exit 1
fi

validate_contract_id "Vault" "$VAULT_ID"
ok "Vault deployed: $VAULT_ID"

# ---------------------------------------------------------------------------
# Deploy XLM strategy
# ---------------------------------------------------------------------------
STRATEGY_ID=""

if [ "$DEPLOY_STRATEGY" = true ]; then
    phase 5 "Deploy XLM strategy"

    CONTROLLER_ADDRESS="${CONTROLLER:-$SOURCE_ADDRESS}"

    log "Deploying XLM Strategy..."
    log "  Asset:      $TOKEN_ID"
    log "  Vault:      $VAULT_ID"
    log "  Controller: $CONTROLLER_ADDRESS"

    if ! STRATEGY_ID=$(stellar contract deploy \
        --wasm "$STRATEGY_WASM" \
        --network "$NETWORK" \
        --source-account "$SOURCE" \
        -- \
        --asset "$TOKEN_ID" \
        --vault "$VAULT_ID" \
        --controller "$CONTROLLER_ADDRESS" 2>"$STDERR_FILE"); then
        fail "Strategy deployment failed: $(cat "$STDERR_FILE")"
        exit 1
    fi

    validate_contract_id "Strategy" "$STRATEGY_ID"
    ok "Strategy deployed: $STRATEGY_ID"
fi

# ---------------------------------------------------------------------------
# Configure vault
# ---------------------------------------------------------------------------
phase 6 "Configure vault"

# Set operator
OPERATOR_ADDRESS="${OPERATOR:-$SOURCE_ADDRESS}"
log "Setting operator to $OPERATOR_ADDRESS..."

if ! invoke_vault set_operator \
    --admin "$SOURCE_ADDRESS" \
    --new_operator "$OPERATOR_ADDRESS" >/dev/null; then
    fail "Failed to set operator."
    exit 1
fi

ok "Operator set"

# Whitelist strategy
if [ -n "$STRATEGY_ID" ]; then
    log "Adding strategy as subaccount..."
    if ! invoke_vault add_subaccount \
        --admin "$SOURCE_ADDRESS" \
        --subaccount "$STRATEGY_ID" \
        --subaccount_type '{"Strategy":[]}' >/dev/null; then
        fail "Failed to add strategy as subaccount."
        exit 1
    fi

    ok "Strategy whitelisted as subaccount"
fi

# ---------------------------------------------------------------------------
# Verify deployment
# ---------------------------------------------------------------------------
phase 7 "Verify deployment"

if ! VAULT_NAME=$(invoke_vault name); then
    fail "Verification failed: could not read vault name. The contract may not have initialized correctly."
    exit 1
fi
if ! VAULT_SYMBOL=$(invoke_vault symbol); then
    fail "Verification failed: could not read vault symbol."
    exit 1
fi
if ! VAULT_TOTAL_ASSETS=$(invoke_vault total_assets); then
    fail "Verification failed: could not read total_assets."
    exit 1
fi

log "Vault name:         $VAULT_NAME"
log "Vault symbol:       $VAULT_SYMBOL"
log "Vault total_assets: $VAULT_TOTAL_ASSETS"

if [ -n "$STRATEGY_ID" ]; then
    if ! SUBACCOUNTS=$(invoke_vault get_subaccounts); then
        fail "Verification failed: could not read subaccounts."
        exit 1
    fi
    log "Subaccounts:        $SUBACCOUNTS"
fi

ok "Verification complete"

# ---------------------------------------------------------------------------
# Write frontend .env snippet
# ---------------------------------------------------------------------------
if [ -n "$ENV_FILE" ]; then
    phase 8 "Write frontend env file"

    cat > "$ENV_FILE" <<EOF
# Generated by deploy-all.sh on $(date -u +"%Y-%m-%dT%H:%M:%SZ")
NEXT_PUBLIC_DEFAULT_VAULT_CONTRACT_ID=$VAULT_ID
NEXT_PUBLIC_DEFAULT_DEPOSIT_TOKEN=$TOKEN_ID
EOF

    ok "Frontend env written to $ENV_FILE"
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}  Deployment Complete${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo "  Network:    $NETWORK"
echo "  Asset:      $TOKEN_ID"
echo "  Vault:      $VAULT_ID"
if [ -n "$STRATEGY_ID" ]; then
    echo "  Strategy:   $STRATEGY_ID"
    echo "  Controller: ${CONTROLLER_ADDRESS:-$SOURCE_ADDRESS}"
fi
echo "  Operator:   $OPERATOR_ADDRESS"
echo ""
echo "  Frontend .env:"
echo "    NEXT_PUBLIC_DEFAULT_VAULT_CONTRACT_ID=$VAULT_ID"
echo "    NEXT_PUBLIC_DEFAULT_DEPOSIT_TOKEN=$TOKEN_ID"
echo ""
