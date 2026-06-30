#!/usr/bin/env bash
# =============================================================================
# August Vault — Post-Deployment Configuration
#
# Configures vault parameters on an already-deployed contract: AUM rate limits,
# operator address, and pause state.
#
# Usage:
#   ./scripts/configure.sh [options]
#
# Options:
#   --vault <contract-id>       Vault contract to configure (required)
#   --network <name>            testnet | mainnet (default: testnet)
#   --source <identity>         Stellar identity name (default: deployer)
#   --operator <address>        Set the operator address
#   --aum-increase-bps <n>     Set AUM increase limit in basis points (1-10000)
#   --aum-decrease-bps <n>     Set AUM decrease limit in basis points (1-10000)
#   --pause                     Pause the vault
#   --unpause                   Unpause the vault
#
# Examples:
#   ./scripts/configure.sh --vault CABC... --aum-increase-bps 1000 --aum-decrease-bps 500
#   ./scripts/configure.sh --vault CABC... --operator GDEF...
#   ./scripts/configure.sh --vault CABC... --network mainnet --source admin --pause
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
NETWORK="testnet"
SOURCE="deployer"
SET_OPERATOR=""
AUM_INCREASE_BPS=""
AUM_DECREASE_BPS=""
DO_PAUSE=""

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

log()  { echo -e "${BLUE}[CONFIG]${NC} $*"; }
ok()   { echo -e "${GREEN}[OK]${NC}     $*"; }
fail() { echo -e "${RED}[ERROR]${NC}  $*"; }
warn() { echo -e "${YELLOW}[WARN]${NC}   $*"; }

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

validate_bps() {
    local label="$1"
    local value="$2"
    if ! [[ "$value" =~ ^[0-9]+$ ]] || [ "$value" -lt 1 ] || [ "$value" -gt 10000 ]; then
        fail "$label must be an integer between 1 and 10000 (got: '$value')"
        exit 1
    fi
}

# ---------------------------------------------------------------------------
# Parse arguments
# ---------------------------------------------------------------------------
while [[ $# -gt 0 ]]; do
    case "$1" in
        --vault)            require_arg "$@"; VAULT_ID="$2";         shift 2 ;;
        --network)          require_arg "$@"; NETWORK="$2";          shift 2 ;;
        --source)           require_arg "$@"; SOURCE="$2";           shift 2 ;;
        --operator)         require_arg "$@"; SET_OPERATOR="$2";     shift 2 ;;
        --aum-increase-bps) require_arg "$@"; AUM_INCREASE_BPS="$2"; shift 2 ;;
        --aum-decrease-bps) require_arg "$@"; AUM_DECREASE_BPS="$2"; shift 2 ;;
        --pause)            DO_PAUSE="pause";   shift ;;
        --unpause)          DO_PAUSE="unpause"; shift ;;
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

validate_contract_id "Vault" "$VAULT_ID"

if [[ "$NETWORK" != "testnet" && "$NETWORK" != "mainnet" ]]; then
    fail "--network must be 'testnet' or 'mainnet' (got: '$NETWORK')"
    exit 1
fi

# Check that at least one action was requested
if [ -z "$SET_OPERATOR" ] && [ -z "$AUM_INCREASE_BPS" ] && [ -z "$AUM_DECREASE_BPS" ] && [ -z "$DO_PAUSE" ]; then
    fail "No configuration action specified."
    echo "Specify at least one of: --operator, --aum-increase-bps, --aum-decrease-bps, --pause, --unpause"
    echo "Run with --help for usage."
    exit 1
fi

# Validate --operator address format if provided
if [ -n "$SET_OPERATOR" ]; then
    if ! [[ "$SET_OPERATOR" =~ ^G[A-Z0-9]{55}$ ]]; then
        fail "--operator: invalid Stellar address: '$SET_OPERATOR' (expected G... public key)"
        exit 1
    fi
fi

# Validate BPS values if provided
if [ -n "$AUM_INCREASE_BPS" ]; then
    validate_bps "--aum-increase-bps" "$AUM_INCREASE_BPS"
fi
if [ -n "$AUM_DECREASE_BPS" ]; then
    validate_bps "--aum-decrease-bps" "$AUM_DECREASE_BPS"
fi

# AUM limits must be set together (the contract function takes both)
if { [ -n "$AUM_INCREASE_BPS" ] && [ -z "$AUM_DECREASE_BPS" ]; } || \
   { [ -z "$AUM_INCREASE_BPS" ] && [ -n "$AUM_DECREASE_BPS" ]; }; then
    fail "--aum-increase-bps and --aum-decrease-bps must be specified together"
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
echo -e "${BLUE}  August Vault Configuration${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
log "Vault:   $VAULT_ID"
log "Network: $NETWORK"
log "Source:  $SOURCE ($SOURCE_ADDRESS)"

ACTIONS=0

# ---------------------------------------------------------------------------
# Set operator
# ---------------------------------------------------------------------------
if [ -n "$SET_OPERATOR" ]; then
    echo ""
    log "Setting operator to $SET_OPERATOR..."

    if ! invoke_vault set_operator \
        --admin "$SOURCE_ADDRESS" \
        --new_operator "$SET_OPERATOR" >/dev/null; then
        fail "Failed to set operator. Possible causes:"
        fail "  - Source is not the vault admin"
        fail "  - Invalid operator address"
        exit 1
    fi

    # Verify
    if ! CURRENT_OPERATOR=$(invoke_vault get_operator); then
        warn "Could not verify operator change (read failed). Check manually."
    elif [ "$CURRENT_OPERATOR" = "$SET_OPERATOR" ]; then
        ok "Operator set to $SET_OPERATOR"
    else
        fail "Operator verification failed (expected=$SET_OPERATOR, got=$CURRENT_OPERATOR)"
        exit 1
    fi
    ACTIONS=$((ACTIONS + 1))
fi

# ---------------------------------------------------------------------------
# Set AUM limits
# ---------------------------------------------------------------------------
if [ -n "$AUM_INCREASE_BPS" ] && [ -n "$AUM_DECREASE_BPS" ]; then
    echo ""
    log "Setting AUM limits: increase=${AUM_INCREASE_BPS}bps, decrease=${AUM_DECREASE_BPS}bps..."

    if ! invoke_vault set_aum_limits \
        --admin "$SOURCE_ADDRESS" \
        --increase_bps "$AUM_INCREASE_BPS" \
        --decrease_bps "$AUM_DECREASE_BPS" >/dev/null; then
        fail "Failed to set AUM limits. Possible causes:"
        fail "  - Source is not the vault admin"
        fail "  - BPS values out of range (1-10000)"
        exit 1
    fi

    # Verify
    if ! CURRENT_INC=$(invoke_vault get_aum_increase_limit) || \
       ! CURRENT_DEC=$(invoke_vault get_aum_decrease_limit); then
        warn "Could not verify AUM limits (read failed). Check manually."
    elif [ "$CURRENT_INC" = "$AUM_INCREASE_BPS" ] && [ "$CURRENT_DEC" = "$AUM_DECREASE_BPS" ]; then
        ok "AUM limits set: +${AUM_INCREASE_BPS}bps / -${AUM_DECREASE_BPS}bps"
    else
        fail "AUM limits verification failed (expected=${AUM_INCREASE_BPS}/${AUM_DECREASE_BPS}, got=${CURRENT_INC}/${CURRENT_DEC})"
        exit 1
    fi
    ACTIONS=$((ACTIONS + 1))
fi

# ---------------------------------------------------------------------------
# Pause / Unpause
# ---------------------------------------------------------------------------
if [ -n "$DO_PAUSE" ]; then
    echo ""
    if [ "$DO_PAUSE" = "pause" ]; then
        EXPECT_PAUSED="true"
        LABEL="paused"
    else
        EXPECT_PAUSED="false"
        LABEL="unpaused"
    fi

    log "Setting vault to $LABEL..."
    if ! invoke_vault "$DO_PAUSE" --admin "$SOURCE_ADDRESS" >/dev/null; then
        fail "Failed to $DO_PAUSE vault. Possible causes:"
        fail "  - Source is not the vault admin"
        exit 1
    fi

    if ! IS_PAUSED=$(invoke_vault is_paused); then
        warn "Could not verify pause state (read failed). Check manually."
    elif [ "$IS_PAUSED" = "$EXPECT_PAUSED" ]; then
        ok "Vault $LABEL"
    else
        fail "Verification failed: expected is_paused=$EXPECT_PAUSED, got=$IS_PAUSED"
        exit 1
    fi
    ACTIONS=$((ACTIONS + 1))
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}  Configuration Complete ($ACTIONS action(s))${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
