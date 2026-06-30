#!/usr/bin/env bash
# =============================================================================
# August Vault — Add Strategy Subaccount
#
# Whitelists a strategy contract as a vault subaccount. The strategy must
# implement the IStrategy interface (deposit/withdraw). The contract performs
# a zero-amount smoke-test during registration, and the script verifies the
# strategy was successfully added.
#
# Usage:
#   ./scripts/add-strategy.sh [options]
#
# Options:
#   --vault <contract-id>      Vault contract (required)
#   --strategy <contract-id>   Strategy contract to whitelist (required)
#   --network <name>           testnet | mainnet (default: testnet)
#   --source <identity>        Stellar identity name (default: deployer)
#
# Examples:
#   ./scripts/add-strategy.sh --vault CABC... --strategy CDEF...
#   ./scripts/add-strategy.sh --vault CABC... --strategy CDEF... --network mainnet --source admin
#
# Prerequisites:
#   - stellar CLI (cargo install stellar-cli --locked)
#   - A funded identity with admin access to the vault
# =============================================================================
set -euo pipefail

# ---------------------------------------------------------------------------
# Defaults
# ---------------------------------------------------------------------------
VAULT_ID=""
STRATEGY_ID=""
NETWORK="testnet"
SOURCE="deployer"

# Temp file for capturing stderr
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

log()  { echo -e "${BLUE}[STRATEGY]${NC} $*"; }
ok()   { echo -e "${GREEN}[OK]${NC}       $*"; }
fail() { echo -e "${RED}[ERROR]${NC}    $*"; }
warn() { echo -e "${YELLOW}[WARN]${NC}     $*"; }

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
        --vault)    require_arg "$@"; VAULT_ID="$2";    shift 2 ;;
        --strategy) require_arg "$@"; STRATEGY_ID="$2"; shift 2 ;;
        --network)  require_arg "$@"; NETWORK="$2";     shift 2 ;;
        --source)   require_arg "$@"; SOURCE="$2";      shift 2 ;;
        -h|--help)
            sed -n '2,/^# ===/p' "$0" | grep '^#' | sed 's/^# \?//'
            exit 0 ;;
        *)
            fail "Unknown option: $1"
            echo "Run with --help for usage."
            exit 1 ;;
    esac
done

if [ -z "$VAULT_ID" ]; then
    fail "--vault is required"
    echo "Run with --help for usage."
    exit 1
fi

if [ -z "$STRATEGY_ID" ]; then
    fail "--strategy is required"
    echo "Run with --help for usage."
    exit 1
fi

validate_contract_id "Vault" "$VAULT_ID"
validate_contract_id "Strategy" "$STRATEGY_ID"

if [[ "$NETWORK" != "testnet" && "$NETWORK" != "mainnet" ]]; then
    fail "--network must be 'testnet' or 'mainnet' (got: '$NETWORK')"
    exit 1
fi

# ---------------------------------------------------------------------------
# Prerequisites
# ---------------------------------------------------------------------------
if ! command -v stellar &> /dev/null; then
    fail "stellar CLI not found. Install with: cargo install stellar-cli --locked"
    exit 1
fi

if ! stellar keys address "$SOURCE" &>/dev/null; then
    fail "Identity '$SOURCE' not found."
    exit 1
fi

SOURCE_ADDRESS=$(stellar keys address "$SOURCE")

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Add Strategy Subaccount${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
log "Vault:    $VAULT_ID"
log "Strategy: $STRATEGY_ID"
log "Network:  $NETWORK"
log "Source:   $SOURCE ($SOURCE_ADDRESS)"

# ---------------------------------------------------------------------------
# Check current subaccounts
# ---------------------------------------------------------------------------
echo ""
log "Checking current subaccounts..."
if ! CURRENT_SUBS=$(invoke_vault get_subaccounts); then
    fail "Could not read current subaccounts — check vault ID and network connectivity."
    exit 1
fi

if echo "$CURRENT_SUBS" | grep -qF "$STRATEGY_ID"; then
    fail "Strategy $STRATEGY_ID is already a subaccount"
    exit 1
fi

log "Current subaccounts: $CURRENT_SUBS"

# ---------------------------------------------------------------------------
# Add strategy
# ---------------------------------------------------------------------------
echo ""
log "Adding strategy as subaccount..."
log "  (The contract performs an on-chain IStrategy interface smoke-test during this call)"

if ! invoke_vault add_subaccount \
    --admin "$SOURCE_ADDRESS" \
    --subaccount "$STRATEGY_ID" \
    --subaccount_type '{"Strategy":[]}' >/dev/null; then
    fail "Failed to add strategy. Possible causes:"
    fail "  - Strategy does not implement IStrategy interface (deposit/withdraw)"
    fail "  - Vault is paused"
    fail "  - Maximum subaccounts (10) reached"
    fail "  - Source is not the vault admin"
    exit 1
fi

# ---------------------------------------------------------------------------
# Verify
# ---------------------------------------------------------------------------
echo ""
log "Verifying..."
if ! UPDATED_SUBS=$(invoke_vault get_subaccounts); then
    warn "Could not verify (read failed). The strategy was likely added — check manually."
elif echo "$UPDATED_SUBS" | grep -qF "$STRATEGY_ID"; then
    ok "Strategy successfully whitelisted"
else
    fail "Verification failed: strategy not found in subaccounts after add"
    exit 1
fi

log "Subaccounts: $UPDATED_SUBS"

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}  Strategy Added Successfully${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo "  Vault:    $VAULT_ID"
echo "  Strategy: $STRATEGY_ID"
echo "  Network:  $NETWORK"
echo ""
echo "  The operator can now deploy capital to this strategy with:"
echo "    stellar contract invoke --id $VAULT_ID --network $NETWORK --source-account <operator> \\"
echo "      -- deposit_to_subaccount --operator <OPERATOR_ADDR> --subaccount $STRATEGY_ID --amount <AMOUNT>"
echo ""
