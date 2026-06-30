#!/usr/bin/env bash
# =============================================================================
# August Vault — Contract Upgrade
#
# Builds (optionally), installs the WASM to the ledger, displays hashes for
# operator verification, and upgrades the vault contract in a single workflow.
#
# Usage:
#   ./scripts/upgrade.sh [options]
#
# Options:
#   --vault <contract-id>    Vault contract to upgrade (required)
#   --network <name>         testnet | mainnet (default: testnet)
#   --source <identity>      Stellar identity name (default: deployer)
#   --wasm <path>            Path to WASM file (default: build output)
#   --skip-build             Skip building WASM (use existing binary)
#   --skip-verify            Skip post-upgrade state verification
#   --yes                    Skip confirmation prompt (for CI)
#
# Examples:
#   ./scripts/upgrade.sh --vault CABC... --network testnet --source deployer
#   ./scripts/upgrade.sh --vault CABC... --network mainnet --source admin --yes
#   ./scripts/upgrade.sh --vault CABC... --wasm ./my-build.wasm --skip-build
#
# Prerequisites:
#   - stellar CLI (cargo install stellar-cli --locked)
#   - Rust wasm32-unknown-unknown target (unless --skip-build)
#   - A funded identity with admin access to the vault
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# ---------------------------------------------------------------------------
# Defaults
# ---------------------------------------------------------------------------
VAULT_ID=""
NETWORK="testnet"
SOURCE="deployer"
# Upload the optimized WASM produced by build.sh. Raw rustc output
# (since 1.95) trips stellar-core's "reference-types not enabled" check
# because call_indirect is emitted with padded LEB128 bytes.
WASM_FILE="$PROJECT_ROOT/target/wasm32-unknown-unknown/release/august_vault.optimized.wasm"
SKIP_BUILD=false
SKIP_VERIFY=false
AUTO_YES=false

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

log()  { echo -e "${BLUE}[UPGRADE]${NC} $*"; }
ok()   { echo -e "${GREEN}[OK]${NC}      $*"; }
fail() { echo -e "${RED}[ERROR]${NC}   $*"; }
warn() { echo -e "${YELLOW}[WARN]${NC}    $*"; }

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
        --vault)       require_arg "$@"; VAULT_ID="$2";   shift 2 ;;
        --network)     require_arg "$@"; NETWORK="$2";    shift 2 ;;
        --source)      require_arg "$@"; SOURCE="$2";     shift 2 ;;
        --wasm)        require_arg "$@"; WASM_FILE="$2";  shift 2 ;;
        --skip-build)  SKIP_BUILD=true;  shift ;;
        --skip-verify) SKIP_VERIFY=true; shift ;;
        --yes)         AUTO_YES=true;    shift ;;
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

if [[ "$NETWORK" != "testnet" && "$NETWORK" != "mainnet" ]]; then
    fail "--network must be 'testnet' or 'mainnet' (got: '$NETWORK')"
    exit 1
fi

validate_contract_id "Vault" "$VAULT_ID"

# ---------------------------------------------------------------------------
# Step 1: Prerequisites
# ---------------------------------------------------------------------------
phase 1 "Check prerequisites"

if ! command -v stellar &> /dev/null; then
    fail "stellar CLI not found. Install with: cargo install stellar-cli --locked"
    exit 1
fi
log "stellar CLI: $(stellar --version 2>/dev/null || echo 'unknown')"

if ! stellar keys address "$SOURCE" &>/dev/null; then
    fail "Identity '$SOURCE' not found."
    exit 1
fi

SOURCE_ADDRESS=$(stellar keys address "$SOURCE")
log "Source identity: $SOURCE ($SOURCE_ADDRESS)"
log "Vault:          $VAULT_ID"
log "Network:        $NETWORK"

ok "Prerequisites met"

# ---------------------------------------------------------------------------
# Step 2: Build WASM
# ---------------------------------------------------------------------------
phase 2 "Build WASM"

if [ "$SKIP_BUILD" = true ]; then
    log "Skipping build (--skip-build)"
else
    log "Building WASM..."
    "$SCRIPT_DIR/build.sh"
fi

if [ ! -f "$WASM_FILE" ]; then
    fail "WASM file not found at $WASM_FILE"
    exit 1
fi

WASM_SIZE=$(wc -c < "$WASM_FILE")
log "WASM file: $WASM_FILE"
log "WASM size: $((WASM_SIZE / 1024))KB ($WASM_SIZE bytes)"

ok "WASM ready"

# ---------------------------------------------------------------------------
# Step 3: Compute and display WASM hash
# ---------------------------------------------------------------------------
phase 3 "WASM hash computation"

# Compute SHA-256 of the WASM binary for operator verification
if command -v shasum &> /dev/null; then
    WASM_SHA256=$(shasum -a 256 "$WASM_FILE" | awk '{print $1}')
elif command -v sha256sum &> /dev/null; then
    WASM_SHA256=$(sha256sum "$WASM_FILE" | awk '{print $1}')
else
    if [ "$NETWORK" = "mainnet" ]; then
        fail "Cannot compute SHA-256 hash (install shasum or sha256sum)."
        fail "Refusing to proceed with mainnet upgrade without hash verification."
        exit 1
    fi
    warn "Neither shasum nor sha256sum found — skipping SHA-256 display"
    WASM_SHA256="(unavailable)"
fi

log "WASM SHA-256: $WASM_SHA256"

ok "Hash computed — WASM will be installed to the ledger after confirmation"

# ---------------------------------------------------------------------------
# Step 4: Capture pre-upgrade state (skipped with --skip-verify)
# ---------------------------------------------------------------------------
if [ "$SKIP_VERIFY" = false ]; then
    phase 4 "Capture pre-upgrade state"

    capture_or_fail() {
        local label="$1"
        shift
        local value
        if ! value=$(invoke_vault "$@"); then
            fail "Cannot read pre-upgrade state ($label). Use --skip-verify to skip verification."
            exit 1
        fi
        echo "$value"
    }

    PRE_NAME=$(capture_or_fail "name" name)
    PRE_SYMBOL=$(capture_or_fail "symbol" symbol)
    PRE_TOTAL_ASSETS=$(capture_or_fail "total_assets" total_assets)
    PRE_TOTAL_SUPPLY=$(capture_or_fail "total_supply" total_supply)
    PRE_OPERATOR=$(capture_or_fail "get_operator" get_operator)
    PRE_PAUSED=$(capture_or_fail "is_paused" is_paused)
    PRE_SUBACCOUNTS=$(capture_or_fail "get_subaccounts" get_subaccounts)
    PRE_DEPLOYED=$(capture_or_fail "get_deployed_assets" get_deployed_assets)
    PRE_AUM_INC=$(capture_or_fail "get_aum_increase_limit" get_aum_increase_limit)
    PRE_AUM_DEC=$(capture_or_fail "get_aum_decrease_limit" get_aum_decrease_limit)

    log "Pre-upgrade state:"
    log "  Name:             $PRE_NAME"
    log "  Symbol:           $PRE_SYMBOL"
    log "  Total assets:     $PRE_TOTAL_ASSETS"
    log "  Total supply:     $PRE_TOTAL_SUPPLY"
    log "  Deployed assets:  $PRE_DEPLOYED"
    log "  Operator:         $PRE_OPERATOR"
    log "  Paused:           $PRE_PAUSED"
    log "  AUM limits:       +${PRE_AUM_INC}bps / -${PRE_AUM_DEC}bps"
    log "  Subaccounts:      $PRE_SUBACCOUNTS"

    ok "State captured"
fi

# ---------------------------------------------------------------------------
# Step 5: Confirmation
# ---------------------------------------------------------------------------
phase 5 "Confirm upgrade"

echo ""
echo -e "${YELLOW}  You are about to upgrade the vault contract.${NC}"
echo ""
echo "  Vault:     $VAULT_ID"
echo "  Network:   $NETWORK"
echo "  SHA-256:   $WASM_SHA256"
echo ""

if [ "$AUTO_YES" = false ]; then
    read -rp "  Proceed with upgrade? [y/N] " confirm
    if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
        log "Upgrade cancelled by user. No on-chain changes were made."
        exit 0
    fi
else
    if [ "$NETWORK" = "mainnet" ]; then
        warn "Running unattended upgrade on MAINNET (--yes)"
    fi
fi

# ---------------------------------------------------------------------------
# Step 6: Install WASM and execute upgrade
# ---------------------------------------------------------------------------
phase 6 "Install WASM and execute upgrade"

# Install WASM to the ledger (returns the on-chain hash).
# This is done after confirmation so that cancelling avoids the ledger write fee.
log "Installing WASM to ledger..."
if ! WASM_HASH=$(stellar contract install \
    --wasm "$WASM_FILE" \
    --network "$NETWORK" \
    --source-account "$SOURCE" 2>"$STDERR_FILE"); then
    fail "WASM install failed: $(cat "$STDERR_FILE")"
    exit 1
fi

log "On-chain WASM hash: $WASM_HASH"

log "Calling upgrade on vault..."

# Use the pre-upgrade operator (if captured) to preserve the existing operator.
# Passing SOURCE_ADDRESS here would silently change the operator on-chain.
if [ "$SKIP_VERIFY" = false ]; then
    UPGRADE_OPERATOR="$PRE_OPERATOR"
else
    # Without pre-upgrade state, we must use SOURCE_ADDRESS; warn the user.
    warn "Pre-upgrade state not captured (--skip-verify). Using source address as operator."
    warn "If the vault has a different operator, pass it explicitly or run without --skip-verify."
    UPGRADE_OPERATOR="$SOURCE_ADDRESS"
fi

if ! invoke_vault upgrade \
    --new_wasm_hash "$WASM_HASH" \
    --operator "$UPGRADE_OPERATOR" >/dev/null; then
    fail "Upgrade invocation failed (WASM hash: $WASM_HASH). Possible causes:"
    fail "  - Source is not the vault admin"
    fail "  - Network connectivity issue"
    fail "The WASM was installed to the ledger but the vault has NOT been upgraded."
    fail "It is safe to retry (the installed WASM will be reused)."
    exit 1
fi

ok "Upgrade executed successfully"

# ---------------------------------------------------------------------------
# Step 7: Post-upgrade verification
# ---------------------------------------------------------------------------
if [ "$SKIP_VERIFY" = false ]; then
    phase 7 "Post-upgrade verification"

    VERIFY_FAILED=false

    verify() {
        local label="$1"
        local expected="$2"
        local actual="$3"
        if [ "$expected" = "$actual" ]; then
            ok "$label preserved"
        else
            fail "$label changed: '$expected' -> '$actual'"
            VERIFY_FAILED=true
        fi
    }

    post_read_or_fail() {
        local label="$1"
        shift
        local value
        if ! value=$(invoke_vault "$@"); then
            fail "CRITICAL: Cannot read vault state after upgrade ($label)."
            fail "The contract may be broken. Investigate immediately."
            fail "Vault: $VAULT_ID | Network: $NETWORK | WASM hash: $WASM_HASH"
            exit 1
        fi
        echo "$value"
    }

    POST_NAME=$(post_read_or_fail "name" name)
    POST_SYMBOL=$(post_read_or_fail "symbol" symbol)
    POST_TOTAL_ASSETS=$(post_read_or_fail "total_assets" total_assets)
    POST_TOTAL_SUPPLY=$(post_read_or_fail "total_supply" total_supply)
    POST_OPERATOR=$(post_read_or_fail "get_operator" get_operator)
    POST_PAUSED=$(post_read_or_fail "is_paused" is_paused)
    POST_SUBACCOUNTS=$(post_read_or_fail "get_subaccounts" get_subaccounts)
    POST_DEPLOYED=$(post_read_or_fail "get_deployed_assets" get_deployed_assets)
    POST_AUM_INC=$(post_read_or_fail "get_aum_increase_limit" get_aum_increase_limit)
    POST_AUM_DEC=$(post_read_or_fail "get_aum_decrease_limit" get_aum_decrease_limit)

    verify "Name"             "$PRE_NAME"           "$POST_NAME"
    verify "Symbol"           "$PRE_SYMBOL"         "$POST_SYMBOL"
    verify "Total assets"     "$PRE_TOTAL_ASSETS"   "$POST_TOTAL_ASSETS"
    verify "Total supply"     "$PRE_TOTAL_SUPPLY"   "$POST_TOTAL_SUPPLY"
    verify "Deployed assets"  "$PRE_DEPLOYED"        "$POST_DEPLOYED"
    verify "Operator"         "$PRE_OPERATOR"        "$POST_OPERATOR"
    verify "Paused"           "$PRE_PAUSED"          "$POST_PAUSED"
    verify "AUM increase bps" "$PRE_AUM_INC"         "$POST_AUM_INC"
    verify "AUM decrease bps" "$PRE_AUM_DEC"         "$POST_AUM_DEC"
    verify "Subaccounts"      "$PRE_SUBACCOUNTS"     "$POST_SUBACCOUNTS"

    if [ "$VERIFY_FAILED" = true ]; then
        echo ""
        fail "State verification FAILED — some values changed unexpectedly."
        fail "Review the changes above carefully."
        exit 1
    fi

    ok "All state preserved after upgrade"
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}  Upgrade Complete${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo "  Vault:     $VAULT_ID"
echo "  Network:   $NETWORK"
echo "  WASM hash: $WASM_HASH"
echo "  SHA-256:   $WASM_SHA256"
echo ""
