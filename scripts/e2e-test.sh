#!/usr/bin/env bash
# =============================================================================
# August Vault + XLM Strategy — End-to-End Test Suite
#
# Deploys the vault + xlm-strategy WASMs to a local Stellar Quickstart network
# and runs the full lifecycle via `stellar contract invoke`.
#
# Usage:
#   ./scripts/e2e-test.sh          # Starts/stops Docker container automatically
#   ./scripts/e2e-test.sh --ci     # Skips Docker management (CI provides the network)
#
# Prerequisites:
#   - Docker (unless --ci)
#   - curl
#   - stellar CLI (cargo install stellar-cli --locked)
#   - Rust wasm32-unknown-unknown target (rustup target add wasm32-unknown-unknown)
# =============================================================================
set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

RPC_URL="${STELLAR_RPC_URL:-http://localhost:8000/soroban/rpc}"
NETWORK_PASSPHRASE="${STELLAR_NETWORK_PASSPHRASE:-Standalone Network ; February 2017}"
FRIENDBOT_URL="${STELLAR_FRIENDBOT_URL:-http://localhost:8000/friendbot}"
CONTAINER_NAME="stellar-e2e-test"
CI_MODE=false
DOCKER_MANAGED=false

VAULT_WASM_RAW="$PROJECT_ROOT/target/wasm32-unknown-unknown/release/august_vault.wasm"
STRATEGY_WASM_RAW="$PROJECT_ROOT/target/wasm32-unknown-unknown/release/xlm_strategy.wasm"
# Deploy the optimized WASMs. `stellar contract optimize` runs wasm-opt,
# which normalizes call_indirect encodings that rustc >= 1.95 emits with
# padded LEB128 bytes — soroban-core's validator rejects those as
# requiring the reference-types feature ("zero byte expected").
VAULT_WASM="$PROJECT_ROOT/target/wasm32-unknown-unknown/release/august_vault.optimized.wasm"
STRATEGY_WASM="$PROJECT_ROOT/target/wasm32-unknown-unknown/release/xlm_strategy.optimized.wasm"

# All amounts are in stroops (7 decimal places: 1 XLM = 10_000_000 stroops)
DEPOSIT_AMOUNT=1000000000       # 100 XLM — user deposit
MINT_SHARES=500000000           # 500M vault shares (~0.05 XLM worth at initial share price with offset=3)
DEPLOY_AMOUNT=500000000         # 50 XLM — capital deployed to strategy
PROFIT_AMOUNT=50000000          # 5 XLM — simulated strategy profit
PROTOCOL_DEPLOY_AMOUNT=200000000 # 20 XLM — strategy deploys to protocol
DECIMALS_OFFSET=3

# Identities
ADMIN_IDENTITY="e2e-admin"
OPERATOR_IDENTITY="e2e-operator"
USER_IDENTITY="e2e-user"
PROFIT_IDENTITY="e2e-profit"
CONTROLLER_IDENTITY="e2e-controller"
PROTOCOL_IDENTITY="e2e-protocol"

# Track pass/fail
TESTS_RUN=0
TESTS_PASSED=0

# Temp file for capturing stderr from invocations
STDERR_FILE=$(mktemp)
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log()   { echo -e "${BLUE}[E2E]${NC} $*"; }
ok()    { echo -e "${GREEN}[PASS]${NC} $*"; }
fail()  { echo -e "${RED}[FAIL]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }

phase() {
    echo ""
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${BLUE}  Phase $1: $2${NC}"
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
}

assert_eq() {
    local description="$1"
    local expected="$2"
    local actual="$3"
    TESTS_RUN=$((TESTS_RUN + 1))
    if [ "$expected" = "$actual" ]; then
        ok "$description (expected=$expected)"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        fail "$description: expected=$expected actual=$actual"
        exit 1
    fi
}

assert_gt() {
    local description="$1"
    local value="$2"
    local threshold="$3"
    TESTS_RUN=$((TESTS_RUN + 1))
    if ! [[ "$value" =~ ^-?[0-9]+$ ]]; then
        fail "$description: value is not a valid integer: '$value'"
        exit 1
    fi
    if ! [[ "$threshold" =~ ^-?[0-9]+$ ]]; then
        fail "$description: threshold is not a valid integer: '$threshold'"
        exit 1
    fi
    if [ "$value" -gt "$threshold" ]; then
        ok "$description (value=$value > $threshold)"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        fail "$description: value=$value not > $threshold"
        exit 1
    fi
}

assert_ge() {
    local description="$1"
    local value="$2"
    local threshold="$3"
    TESTS_RUN=$((TESTS_RUN + 1))
    if ! [[ "$value" =~ ^-?[0-9]+$ ]]; then
        fail "$description: value is not a valid integer: '$value'"
        exit 1
    fi
    if ! [[ "$threshold" =~ ^-?[0-9]+$ ]]; then
        fail "$description: threshold is not a valid integer: '$threshold'"
        exit 1
    fi
    if [ "$value" -ge "$threshold" ]; then
        ok "$description (value=$value >= $threshold)"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        fail "$description: value=$value not >= $threshold"
        exit 1
    fi
}

assert_contains() {
    local description="$1"
    local haystack="$2"
    local needle="$3"
    TESTS_RUN=$((TESTS_RUN + 1))
    if echo "$haystack" | grep -qF "$needle"; then
        ok "$description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        fail "$description: output does not contain '$needle'"
        fail "  Output was: $haystack"
        exit 1
    fi
}

assert_not_contains() {
    local description="$1"
    local haystack="$2"
    local needle="$3"
    TESTS_RUN=$((TESTS_RUN + 1))
    if echo "$haystack" | grep -qF "$needle"; then
        fail "$description: output should not contain '$needle'"
        fail "  Output was: $haystack"
        exit 1
    else
        ok "$description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    fi
}

# Expect an invocation to fail (non-zero exit). Usage:
#   assert_fails "description" <identity> <contract_id> <function> [args...]
assert_fails() {
    local description="$1"
    shift
    TESTS_RUN=$((TESTS_RUN + 1))
    if invoke_raw "$@" >/dev/null 2>&1; then
        fail "$description (invocation succeeded but should have failed)"
        exit 1
    else
        ok "$description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    fi
}

# Validate a string looks like a Stellar contract ID (C + 55 uppercase alphanumeric chars)
validate_contract_id() {
    local label="$1"
    local value="$2"
    if ! [[ "$value" =~ ^C[A-Z0-9]{55}$ ]]; then
        fail "$label: invalid contract ID: '$value'"
        exit 1
    fi
}

# Invoke a contract function and capture its return value.
# On failure, displays the stderr and exits.
# Usage: result=$(invoke_read <contract_id> <function> [args...])
invoke_read() {
    local contract_id="$1"
    local function_name="$2"
    shift 2
    local result
    if ! result=$(stellar contract invoke \
        --id "$contract_id" \
        --source-account "$ADMIN_IDENTITY" \
        --rpc-url "$RPC_URL" \
        --network-passphrase "$NETWORK_PASSPHRASE" \
        -- \
        "$function_name" "$@" 2>"$STDERR_FILE"); then
        fail "invoke $function_name on ${contract_id:0:10}... failed:"
        fail "  $(cat "$STDERR_FILE")"
        exit 1
    fi
    echo "$result" | tr -d '"'
}

# Invoke a contract function, discarding its return value.
# On failure, displays the stderr and exits.
invoke_mut() {
    local contract_id="$1"
    local function_name="$2"
    shift 2
    if ! stellar contract invoke \
        --id "$contract_id" \
        --source-account "$ADMIN_IDENTITY" \
        --rpc-url "$RPC_URL" \
        --network-passphrase "$NETWORK_PASSPHRASE" \
        -- \
        "$function_name" "$@" >/dev/null 2>"$STDERR_FILE"; then
        fail "invoke $function_name on ${contract_id:0:10}... failed:"
        fail "  $(cat "$STDERR_FILE")"
        exit 1
    fi
}

# Invoke a state-mutating contract function as a specific identity.
invoke_mut_as() {
    local identity="$1"
    local contract_id="$2"
    local function_name="$3"
    shift 3
    if ! stellar contract invoke \
        --id "$contract_id" \
        --source-account "$identity" \
        --rpc-url "$RPC_URL" \
        --network-passphrase "$NETWORK_PASSPHRASE" \
        -- \
        "$function_name" "$@" >/dev/null 2>"$STDERR_FILE"; then
        fail "invoke $function_name as $identity on ${contract_id:0:10}... failed:"
        fail "  $(cat "$STDERR_FILE")"
        exit 1
    fi
}

# Invoke a contract function as a specific identity and capture its return value.
invoke_read_as() {
    local identity="$1"
    local contract_id="$2"
    local function_name="$3"
    shift 3
    local result
    if ! result=$(stellar contract invoke \
        --id "$contract_id" \
        --source-account "$identity" \
        --rpc-url "$RPC_URL" \
        --network-passphrase "$NETWORK_PASSPHRASE" \
        -- \
        "$function_name" "$@" 2>"$STDERR_FILE"); then
        fail "invoke $function_name as $identity on ${contract_id:0:10}... failed:"
        fail "  $(cat "$STDERR_FILE")"
        exit 1
    fi
    echo "$result" | tr -d '"'
}

# Raw invoke (no output handling) — used for expected-failure tests
invoke_raw() {
    local identity="$1"
    local contract_id="$2"
    local function_name="$3"
    shift 3
    stellar contract invoke \
        --id "$contract_id" \
        --source-account "$identity" \
        --rpc-url "$RPC_URL" \
        --network-passphrase "$NETWORK_PASSPHRASE" \
        -- \
        "$function_name" "$@"
}

# Get the public key for an identity (fails with a labeled error if lookup fails)
get_address() {
    local addr
    if ! addr=$(stellar keys address "$1" 2>"$STDERR_FILE"); then
        fail "Could not resolve address for identity '$1': $(cat "$STDERR_FILE")"
        exit 1
    fi
    if [ -z "$addr" ]; then
        fail "Resolved empty address for identity '$1'"
        exit 1
    fi
    echo "$addr"
}

cleanup() {
    rm -f "$STDERR_FILE"
    if [ "$DOCKER_MANAGED" = true ]; then
        log "Stopping Docker container..."
        docker stop "$CONTAINER_NAME" 2>/dev/null || true
        docker rm "$CONTAINER_NAME" 2>/dev/null || true
    fi
    # Clean up test identities (ignore errors — best-effort cleanup)
    for id in "$ADMIN_IDENTITY" "$OPERATOR_IDENTITY" "$USER_IDENTITY" \
              "$PROFIT_IDENTITY" "$CONTROLLER_IDENTITY" "$PROTOCOL_IDENTITY" "e2e-new-admin"; do
        stellar keys rm "$id" 2>/dev/null || true
    done
}

# Fund an account via friendbot with retry (fixed 5s delay between attempts)
fund_account() {
    local identity="$1"
    local address="$2"
    local max_attempts=10
    local attempt=1
    while [ $attempt -le $max_attempts ]; do
        local response
        local http_code
        response=$(curl -s -w "\n%{http_code}" "$FRIENDBOT_URL?addr=$address" 2>&1) || true
        http_code=$(echo "$response" | tail -1)

        if [ "$http_code" = "200" ]; then
            return 0
        fi
        warn "Friendbot attempt $attempt/$max_attempts for $identity failed (HTTP $http_code)"
        sleep 5
        attempt=$((attempt + 1))
    done
    fail "Could not fund $identity after $max_attempts attempts"
    exit 1
}

# ---------------------------------------------------------------------------
# Parse arguments
# ---------------------------------------------------------------------------
for arg in "$@"; do
    case "$arg" in
        --ci) CI_MODE=true ;;
        *)
            fail "Unknown argument: $arg"
            fail "Usage: $0 [--ci]"
            exit 1
            ;;
    esac
done

# ---------------------------------------------------------------------------
# Phase 0: Prerequisites
# ---------------------------------------------------------------------------
phase 0 "Prerequisites check"

if ! command -v stellar &>/dev/null; then
    fail "stellar CLI not found. Install with: cargo install stellar-cli --locked"
    exit 1
fi
log "stellar CLI: $(stellar --version 2>/dev/null || echo 'unknown version')"

if ! command -v curl &>/dev/null; then
    fail "curl not found (required for health checks and friendbot funding)"
    exit 1
fi

if [ "$CI_MODE" = false ] && ! command -v docker &>/dev/null; then
    fail "Docker not found (required unless --ci mode)"
    exit 1
fi

if ! rustup target list --installed | grep -q wasm32-unknown-unknown; then
    fail "wasm32-unknown-unknown target not installed. Run: rustup target add wasm32-unknown-unknown"
    exit 1
fi

log "All prerequisites met."

# ---------------------------------------------------------------------------
# Phase 1: Start local network
# ---------------------------------------------------------------------------
phase 1 "Start local Stellar network"

if [ "$CI_MODE" = true ]; then
    log "CI mode: skipping Docker management (network provided by CI services)"
else
    # Stop any existing container
    docker stop "$CONTAINER_NAME" 2>/dev/null || true
    docker rm "$CONTAINER_NAME" 2>/dev/null || true

    # Pinned to an immutable quickstart build (protocol 25, core v26.1.0)
    # instead of the rolling :testing tag. :testing now ships a pre-release
    # Soroban host (protocol 27) that no released stellar-cli is compatible
    # with, so `stellar contract asset deploy` fails at simulation with
    # HostError: Error(Context, InternalError). Keep this in lockstep with the
    # stellar-cli version (v25.1.0): both must speak the same protocol.
    QUICKSTART_IMAGE="stellar/quickstart:v638-b1076.1-testing"
    log "Starting $QUICKSTART_IMAGE in standalone mode..."
    docker run -d \
        --name "$CONTAINER_NAME" \
        -p 8000:8000 \
        -e ENABLE_SOROBAN_RPC=true \
        -e NETWORK=local \
        "$QUICKSTART_IMAGE" \
        --local

    DOCKER_MANAGED=true
    log "Container '$CONTAINER_NAME' started."
fi

# ---------------------------------------------------------------------------
# Phase 2: Wait for network readiness
# ---------------------------------------------------------------------------
phase 2 "Wait for network readiness"

MAX_WAIT=120
WAITED=0
log "Waiting for RPC endpoint at $RPC_URL (up to ${MAX_WAIT}s)..."

while [ $WAITED -lt $MAX_WAIT ]; do
    if curl -sf "$RPC_URL" \
        -X POST \
        -H 'Content-Type: application/json' \
        -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
        2>/dev/null | grep -q '"status":"healthy"'; then
        break
    fi
    sleep 2
    WAITED=$((WAITED + 2))
    if [ $((WAITED % 10)) -eq 0 ]; then
        log "  Still waiting... (${WAITED}s)"
    fi
done

if [ $WAITED -ge $MAX_WAIT ]; then
    fail "Network did not become ready within ${MAX_WAIT}s"
    exit 1
fi

log "Network is healthy (waited ${WAITED}s)."

# Also wait for friendbot readiness (may lag behind RPC).
# Friendbot returns HTTP 400 (not 404/5xx) when called without ?addr=,
# so we wait for a non-5xx, non-connection-refused response. A 502/503
# means the reverse proxy is up but friendbot itself is still starting.
log "Checking friendbot readiness..."
FRIENDBOT_WAITED=0
FRIENDBOT_MAX=120
while [ $FRIENDBOT_WAITED -lt $FRIENDBOT_MAX ]; do
    HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$FRIENDBOT_URL" 2>/dev/null || echo "000")
    # Accept any non-000 (connection refused) and non-5xx (upstream not ready) code.
    if [ "$HTTP_CODE" != "000" ] && ! echo "$HTTP_CODE" | grep -q '^5'; then
        break
    fi
    sleep 2
    FRIENDBOT_WAITED=$((FRIENDBOT_WAITED + 2))
done
if [ "$HTTP_CODE" = "000" ]; then
    fail "Friendbot not ready within ${FRIENDBOT_MAX}s (connection refused)"
    exit 1
fi
if echo "$HTTP_CODE" | grep -q '^5'; then
    fail "Friendbot not ready within ${FRIENDBOT_MAX}s (last HTTP status: $HTTP_CODE)"
    exit 1
fi
log "Friendbot is ready (HTTP $HTTP_CODE, waited ${FRIENDBOT_WAITED}s)."

# ---------------------------------------------------------------------------
# Phase 3: Create test identities and fund via friendbot
# ---------------------------------------------------------------------------
phase 3 "Create test identities and fund accounts"

for identity in "$ADMIN_IDENTITY" "$OPERATOR_IDENTITY" "$USER_IDENTITY" \
                "$PROFIT_IDENTITY" "$CONTROLLER_IDENTITY" "$PROTOCOL_IDENTITY"; do
    # Remove if exists from a previous run
    stellar keys rm "$identity" 2>/dev/null || true
    if ! stellar keys generate "$identity"; then
        fail "Could not generate key for $identity"
        exit 1
    fi
    ADDRESS=$(get_address "$identity")
    log "Created $identity: $ADDRESS"

    fund_account "$identity" "$ADDRESS"
    log "Funded $identity"
done

# ---------------------------------------------------------------------------
# Phase 4: Build WASMs
# ---------------------------------------------------------------------------
phase 4 "Build WASM contracts"

log "Building vault + xlm-strategy..."
cd "$PROJECT_ROOT"
cargo build --target wasm32-unknown-unknown --release 2>&1

if [ ! -f "$VAULT_WASM_RAW" ]; then
    fail "Vault WASM not found at $VAULT_WASM_RAW"
    exit 1
fi

if [ ! -f "$STRATEGY_WASM_RAW" ]; then
    fail "Strategy WASM not found at $STRATEGY_WASM_RAW"
    exit 1
fi

log "Optimizing WASMs with stellar contract optimize..."
stellar contract optimize --wasm "$VAULT_WASM_RAW" --wasm-out "$VAULT_WASM" 2>&1 | tail -1
stellar contract optimize --wasm "$STRATEGY_WASM_RAW" --wasm-out "$STRATEGY_WASM" 2>&1 | tail -1

if [ ! -f "$VAULT_WASM" ] || [ ! -f "$STRATEGY_WASM" ]; then
    fail "Optimized WASMs not produced (stellar CLI optimize failed)"
    exit 1
fi

VAULT_SIZE=$(stat -f%z "$VAULT_WASM" 2>/dev/null || stat -c%s "$VAULT_WASM")
STRATEGY_SIZE=$(stat -f%z "$STRATEGY_WASM" 2>/dev/null || stat -c%s "$STRATEGY_WASM")
log "Vault WASM: $((VAULT_SIZE / 1024))KB ($VAULT_SIZE bytes)"
log "Strategy WASM: $((STRATEGY_SIZE / 1024))KB ($STRATEGY_SIZE bytes)"

# ---------------------------------------------------------------------------
# Phase 5: Wrap native XLM as Stellar Asset Contract
# ---------------------------------------------------------------------------
phase 5 "Wrap native XLM as Stellar Asset Contract"

ADMIN_ADDRESS=$(get_address "$ADMIN_IDENTITY")
OPERATOR_ADDRESS=$(get_address "$OPERATOR_IDENTITY")
USER_ADDRESS=$(get_address "$USER_IDENTITY")
PROFIT_ADDRESS=$(get_address "$PROFIT_IDENTITY")
CONTROLLER_ADDRESS=$(get_address "$CONTROLLER_IDENTITY")
PROTOCOL_ADDRESS=$(get_address "$PROTOCOL_IDENTITY")

log "Wrapping native XLM as a Stellar Asset Contract..."
TOKEN_ID=$(stellar contract asset deploy \
    --asset native \
    --source-account "$ADMIN_IDENTITY" \
    --rpc-url "$RPC_URL" \
    --network-passphrase "$NETWORK_PASSPHRASE" 2>"$STDERR_FILE") || {
    fail "Asset deploy failed: $(cat "$STDERR_FILE")"
    exit 1
}

validate_contract_id "Token deploy" "$TOKEN_ID"
log "Token (native XLM SAC): $TOKEN_ID"

# Verify token works by checking admin balance
ADMIN_BALANCE=$(invoke_read "$TOKEN_ID" balance --id "$ADMIN_ADDRESS")
assert_gt "Admin has XLM from friendbot funding" "$ADMIN_BALANCE" "0"

# ---------------------------------------------------------------------------
# Phase 6: Deploy vault contract
# ---------------------------------------------------------------------------
phase 6 "Deploy vault contract"

log "Deploying August Vault..."
VAULT_ID=$(stellar contract deploy \
    --wasm "$VAULT_WASM" \
    --source-account "$ADMIN_IDENTITY" \
    --rpc-url "$RPC_URL" \
    --network-passphrase "$NETWORK_PASSPHRASE" \
    -- \
    --name "E2E Test Vault" \
    --symbol "avTEST" \
    --asset "$TOKEN_ID" \
    --decimals_offset "$DECIMALS_OFFSET" \
    --admin "$ADMIN_ADDRESS" 2>"$STDERR_FILE") || {
    fail "Vault deploy failed: $(cat "$STDERR_FILE")"
    exit 1
}

validate_contract_id "Vault deploy" "$VAULT_ID"
log "Vault deployed: $VAULT_ID"

# Verify initial state
TOTAL_ASSETS=$(invoke_read "$VAULT_ID" total_assets)
assert_eq "Initial total_assets is 0" "0" "$TOTAL_ASSETS"

DEPLOYED_ASSETS=$(invoke_read "$VAULT_ID" get_deployed_assets)
assert_eq "Initial deployed_assets is 0" "0" "$DEPLOYED_ASSETS"

IS_PAUSED=$(invoke_read "$VAULT_ID" is_paused)
assert_eq "Vault starts unpaused" "false" "$IS_PAUSED"

VAULT_NAME=$(invoke_read "$VAULT_ID" name)
assert_eq "Vault name" "E2E Test Vault" "$VAULT_NAME"

VAULT_SYMBOL=$(invoke_read "$VAULT_ID" symbol)
assert_eq "Vault symbol" "avTEST" "$VAULT_SYMBOL"

VAULT_ADMIN=$(invoke_read "$VAULT_ID" get_admin)
assert_eq "Vault admin is correct" "$ADMIN_ADDRESS" "$VAULT_ADMIN"

# Set AUM limits to 100% so the test is not coupled to the vault's defaults.
invoke_mut "$VAULT_ID" set_aum_limits \
    --admin "$ADMIN_ADDRESS" \
    --increase_bps 10000 \
    --decrease_bps 10000

# Also relax cumulative window limits to 100%.
invoke_mut "$VAULT_ID" set_aum_window_limits \
    --admin "$ADMIN_ADDRESS" \
    --window_duration 86400 \
    --cumulative_increase_bps 10000 \
    --cumulative_decrease_bps 10000

# ---------------------------------------------------------------------------
# Phase 7: Deploy XLM strategy contract
# ---------------------------------------------------------------------------
phase 7 "Deploy XLM strategy contract"

log "Deploying XLM Strategy..."
STRATEGY_ID=$(stellar contract deploy \
    --wasm "$STRATEGY_WASM" \
    --source-account "$ADMIN_IDENTITY" \
    --rpc-url "$RPC_URL" \
    --network-passphrase "$NETWORK_PASSPHRASE" \
    -- \
    --asset "$TOKEN_ID" \
    --vault "$VAULT_ID" \
    --controller "$CONTROLLER_ADDRESS" 2>"$STDERR_FILE") || {
    fail "Strategy deploy failed: $(cat "$STDERR_FILE")"
    exit 1
}

validate_contract_id "Strategy deploy" "$STRATEGY_ID"
log "Strategy deployed: $STRATEGY_ID"

# Verify strategy initial state
STRATEGY_VAULT=$(invoke_read "$STRATEGY_ID" get_vault)
assert_eq "Strategy vault is correct" "$VAULT_ID" "$STRATEGY_VAULT"

STRATEGY_CONTROLLER=$(invoke_read "$STRATEGY_ID" get_controller)
assert_eq "Strategy controller is correct" "$CONTROLLER_ADDRESS" "$STRATEGY_CONTROLLER"

STRATEGY_ASSET=$(invoke_read "$STRATEGY_ID" get_asset)
assert_eq "Strategy asset is correct" "$TOKEN_ID" "$STRATEGY_ASSET"

STRATEGY_BALANCE=$(invoke_read "$STRATEGY_ID" get_balance)
assert_eq "Strategy initial balance is 0" "0" "$STRATEGY_BALANCE"

STRATEGY_DEPLOYED=$(invoke_read "$STRATEGY_ID" get_deployed_total)
assert_eq "Strategy initial deployed_total is 0" "0" "$STRATEGY_DEPLOYED"

# =====================================================================
# Phase 8: Vault test scenarios
# =====================================================================
phase 8 "Vault test scenarios"

# --- 8a: Set operator ---
echo ""
log "8a: Set operator"
invoke_mut "$VAULT_ID" set_operator \
    --admin "$ADMIN_ADDRESS" \
    --new_operator "$OPERATOR_ADDRESS"

CURRENT_OPERATOR=$(invoke_read "$VAULT_ID" get_operator)
assert_eq "Operator is set" "$OPERATOR_ADDRESS" "$CURRENT_OPERATOR"

# --- 8b: Add subaccount (strategy) ---
echo ""
log "8b: Add subaccount (strategy)"
invoke_mut "$VAULT_ID" add_subaccount \
    --admin "$ADMIN_ADDRESS" \
    --subaccount "$STRATEGY_ID" \
    --subaccount_type '{"Strategy":[]}'

SUBACCOUNTS=$(invoke_read "$VAULT_ID" get_subaccounts)
assert_contains "Strategy is in subaccounts list" "$SUBACCOUNTS" "$STRATEGY_ID"

# --- 8b2: Access control — admin functions reject non-admin ---
echo ""
log "8b2: Verify admin functions reject non-admin callers"

assert_fails "pause rejects non-admin" \
    "$USER_IDENTITY" "$VAULT_ID" pause --admin "$USER_ADDRESS"

assert_fails "set_operator rejects non-admin" \
    "$USER_IDENTITY" "$VAULT_ID" set_operator \
    --admin "$USER_ADDRESS" --new_operator "$USER_ADDRESS"

assert_fails "add_subaccount rejects non-admin" \
    "$USER_IDENTITY" "$VAULT_ID" add_subaccount \
    --admin "$USER_ADDRESS" --subaccount "$STRATEGY_ID"

assert_fails "set_aum_limits rejects non-admin" \
    "$USER_IDENTITY" "$VAULT_ID" set_aum_limits \
    --admin "$USER_ADDRESS" --increase_bps 5000 --decrease_bps 5000

# --- 8b3: Access control — operator functions reject non-operator ---
echo ""
log "8b3: Verify operator functions reject non-operator callers"

assert_fails "deposit_to_subaccount rejects non-operator" \
    "$USER_IDENTITY" "$VAULT_ID" deposit_to_subaccount \
    --operator "$USER_ADDRESS" --subaccount "$STRATEGY_ID" --amount 1000

assert_fails "update_deployed_assets rejects non-operator" \
    "$USER_IDENTITY" "$VAULT_ID" update_deployed_assets \
    --operator "$USER_ADDRESS" --amount 1000

assert_fails "withdraw_from_subaccount rejects non-operator" \
    "$USER_IDENTITY" "$VAULT_ID" withdraw_from_subaccount \
    --operator "$USER_ADDRESS" --subaccount "$STRATEGY_ID" --amount 1000

# --- 8c: Deposit into vault (user) ---
echo ""
log "8c: User deposits into vault"
invoke_mut_as "$USER_IDENTITY" "$VAULT_ID" deposit \
    --assets "$DEPOSIT_AMOUNT" \
    --receiver "$USER_ADDRESS" \
    --from "$USER_ADDRESS" \
    --operator "$USER_ADDRESS"

# --- 8d: Verify share balance + total_assets ---
echo ""
log "8d: Verify share balance and total_assets"
TOTAL_ASSETS=$(invoke_read "$VAULT_ID" total_assets)
assert_eq "Total assets equals deposit" "$DEPOSIT_AMOUNT" "$TOTAL_ASSETS"

# With decimals_offset=3, shares = assets * 10^3
SHARE_BALANCE=$(invoke_read "$VAULT_ID" balance --account "$USER_ADDRESS")
log "  share_balance='$SHARE_BALANCE'"
EXPECTED_SHARES=$((DEPOSIT_AMOUNT * 1000))
assert_eq "User share balance equals deposit * 10^offset" "$EXPECTED_SHARES" "$SHARE_BALANCE"

TOTAL_SUPPLY=$(invoke_read "$VAULT_ID" total_supply)
assert_eq "Total supply equals shares minted" "$SHARE_BALANCE" "$TOTAL_SUPPLY"

# --- 8d2: View functions (conversions & previews) ---
echo ""
log "8d2: Verify view functions"
# With offset=3, 1 asset = 1000 shares
CONVERTED_SHARES=$(invoke_read "$VAULT_ID" convert_to_shares --assets "$DEPOSIT_AMOUNT")
EXPECTED_CONVERTED=$((DEPOSIT_AMOUNT * 1000))
assert_eq "convert_to_shares at 1000:1 ratio" "$EXPECTED_CONVERTED" "$CONVERTED_SHARES"

# 1000 shares = 1 asset (integer division)
ONE_XLM_SHARES=$((10000000 * 1000))  # 1 XLM in shares (10M * 1000)
CONVERTED_ASSETS=$(invoke_read "$VAULT_ID" convert_to_assets --shares "$ONE_XLM_SHARES")
assert_eq "convert_to_assets at 1000:1 ratio" "10000000" "$CONVERTED_ASSETS"

# 1 XLM deposit → 1000 XLM-worth of shares
PREVIEW_DEP=$(invoke_read "$VAULT_ID" preview_deposit --assets 10000000)
assert_eq "preview_deposit 1 XLM" "$ONE_XLM_SHARES" "$PREVIEW_DEP"

# preview_mint: how many assets for a given number of shares
PREVIEW_MINT=$(invoke_read "$VAULT_ID" preview_mint --shares "$ONE_XLM_SHARES")
assert_eq "preview_mint 1 XLM in shares" "10000000" "$PREVIEW_MINT"

PREVIEW_WITHDRAW=$(invoke_read "$VAULT_ID" preview_withdraw --assets 10000000)
assert_eq "preview_withdraw 1 XLM" "$ONE_XLM_SHARES" "$PREVIEW_WITHDRAW"

PREVIEW_REDEEM=$(invoke_read "$VAULT_ID" preview_redeem --shares "$ONE_XLM_SHARES")
assert_eq "preview_redeem 1 XLM in shares" "10000000" "$PREVIEW_REDEEM"

MAX_DEP=$(invoke_read "$VAULT_ID" max_deposit --receiver "$USER_ADDRESS")
assert_eq "max_deposit is i128::MAX when unpaused" "170141183460469231731687303715884105727" "$MAX_DEP"

MAX_MINT_VAL=$(invoke_read "$VAULT_ID" max_mint --receiver "$USER_ADDRESS")
assert_eq "max_mint is i128::MAX when unpaused" "170141183460469231731687303715884105727" "$MAX_MINT_VAL"

MAX_WITHDRAW_VAL=$(invoke_read "$VAULT_ID" max_withdraw --owner "$USER_ADDRESS")
assert_gt "max_withdraw > 0 for user with shares" "$MAX_WITHDRAW_VAL" "0"

MAX_REDEEM_VAL=$(invoke_read "$VAULT_ID" max_redeem --owner "$USER_ADDRESS")
assert_gt "max_redeem > 0 for user with shares" "$MAX_REDEEM_VAL" "0"

VAULT_DECIMALS=$(invoke_read "$VAULT_ID" decimals)
assert_eq "Vault decimals is 10 (XLM 7 + offset 3)" "10" "$VAULT_DECIMALS"

# --- 8d3: Mint (exact shares) ---
echo ""
log "8d3: Mint exact shares"
SHARES_BEFORE_MINT=$(invoke_read "$VAULT_ID" balance --account "$USER_ADDRESS")
invoke_mut_as "$USER_IDENTITY" "$VAULT_ID" mint \
    --shares "$MINT_SHARES" \
    --receiver "$USER_ADDRESS" \
    --from "$USER_ADDRESS" \
    --operator "$USER_ADDRESS"

SHARES_AFTER_MINT=$(invoke_read "$VAULT_ID" balance --account "$USER_ADDRESS")
MINTED_SHARES=$((SHARES_AFTER_MINT - SHARES_BEFORE_MINT))
assert_eq "Mint produced exact shares" "$MINT_SHARES" "$MINTED_SHARES"

# --- 8e: Deploy capital to strategy (operator) ---
echo ""
log "8e: Deploy capital to strategy"
invoke_mut_as "$OPERATOR_IDENTITY" "$VAULT_ID" deposit_to_subaccount \
    --operator "$OPERATOR_ADDRESS" \
    --subaccount "$STRATEGY_ID" \
    --amount "$DEPLOY_AMOUNT"

# Strategy subaccount deposits no longer change deployed_assets (captured by get_balance)
DEPLOYED_ASSETS=$(invoke_read "$VAULT_ID" get_deployed_assets)
assert_eq "Deployed assets stays 0 after strategy deposit" "0" "$DEPLOYED_ASSETS"

# Verify strategy received the tokens (get_balance = idle + deployed_total)
STRATEGY_BALANCE=$(invoke_read "$STRATEGY_ID" get_balance)
assert_eq "Strategy balance matches deployed amount" "$DEPLOY_AMOUNT" "$STRATEGY_BALANCE"

# Verify get_strategy_balances view function
STRATEGY_BALANCES=$(invoke_read "$VAULT_ID" get_strategy_balances)
assert_eq "Vault strategy_balances matches" "$DEPLOY_AMOUNT" "$STRATEGY_BALANCES"

# --- 8f: Verify total_assets unchanged (local + strategy_balances + deployed_assets) ---
echo ""
log "8f: Verify total_assets unchanged after deployment"
# total = vault_local + strategy_balances + deployed_assets
# With offset=3, asset cost of minting MINT_SHARES shares ≈ MINT_SHARES / 1000
MINT_ASSET_COST=$((MINT_SHARES / 1000))
EXPECTED_TOTAL_AFTER_DEPLOY=$((DEPOSIT_AMOUNT + MINT_ASSET_COST))
TOTAL_ASSETS=$(invoke_read "$VAULT_ID" total_assets)
assert_eq "Total assets unchanged (local + strategy_balances)" "$EXPECTED_TOTAL_AFTER_DEPLOY" "$TOTAL_ASSETS"

# --- 8g: Donation detection (F1) — raw transfers don't inflate NAV ---
# Under F1, `get_balance()` returns `local_balance + deployed_total`, not
# `token.balance(self) + deployed_total`. A direct SEP-41 transfer into
# the strategy sits outside the tracker and must NOT count toward NAV —
# otherwise a griefer could manipulate share price by donating.
echo ""
log "8g: Donation detection — direct transfers excluded from get_balance"

invoke_mut_as "$PROFIT_IDENTITY" "$TOKEN_ID" transfer \
    --from "$PROFIT_ADDRESS" \
    --to "$STRATEGY_ID" \
    --amount "$PROFIT_AMOUNT"

STRATEGY_TOKEN_BALANCE=$(invoke_read "$TOKEN_ID" balance --id "$STRATEGY_ID")
assert_eq "Strategy raw token balance reflects donation" \
    "$((DEPLOY_AMOUNT + PROFIT_AMOUNT))" "$STRATEGY_TOKEN_BALANCE"

STRATEGY_BALANCE=$(invoke_read "$STRATEGY_ID" get_balance)
assert_eq "Strategy get_balance excludes donation (F1)" \
    "$DEPLOY_AMOUNT" "$STRATEGY_BALANCE"

# --- 8h: total_assets stays anchored to tracked balance, not raw tokens ---
echo ""
log "8h: total_assets excludes donation (NAV protected under F1)"
TOTAL_ASSETS=$(invoke_read "$VAULT_ID" total_assets)
assert_eq "Total assets unchanged by donation" \
    "$EXPECTED_TOTAL_AFTER_DEPLOY" "$TOTAL_ASSETS"

DEPLOYED_ASSETS=$(invoke_read "$VAULT_ID" get_deployed_assets)
assert_eq "Deployed assets unchanged after donation" "0" "$DEPLOYED_ASSETS"

# Controller recovers the donation, draining it back to the original sender.
log "8h.1: Controller recovers donation via recover_donation"
invoke_mut_as "$CONTROLLER_IDENTITY" "$STRATEGY_ID" recover_donation \
    --controller "$CONTROLLER_ADDRESS" \
    --recipient "$PROFIT_ADDRESS" \
    --amount "$PROFIT_AMOUNT"

STRATEGY_TOKEN_BALANCE_AFTER=$(invoke_read "$TOKEN_ID" balance --id "$STRATEGY_ID")
assert_eq "Strategy raw token balance restored after recover_donation" \
    "$DEPLOY_AMOUNT" "$STRATEGY_TOKEN_BALANCE_AFTER"

STRATEGY_BALANCE_AFTER=$(invoke_read "$STRATEGY_ID" get_balance)
assert_eq "Strategy get_balance still matches tracker after recover" \
    "$DEPLOY_AMOUNT" "$STRATEGY_BALANCE_AFTER"

# --- 8i: Withdraw tracked capital from strategy (operator recalls) ---
echo ""
log "8i: Withdraw tracked capital from strategy"
EXPECTED_STRATEGY_BALANCE="$DEPLOY_AMOUNT"
invoke_mut_as "$OPERATOR_IDENTITY" "$VAULT_ID" withdraw_from_subaccount \
    --operator "$OPERATOR_ADDRESS" \
    --subaccount "$STRATEGY_ID" \
    --amount "$EXPECTED_STRATEGY_BALANCE"

DEPLOYED_AFTER_WITHDRAW=$(invoke_read "$VAULT_ID" get_deployed_assets)
assert_eq "Deployed assets still 0 after strategy recall" "0" "$DEPLOYED_AFTER_WITHDRAW"

EXPECTED_TOTAL="$EXPECTED_TOTAL_AFTER_DEPLOY"
TOTAL_AFTER_RECALL=$(invoke_read "$VAULT_ID" total_assets)
assert_eq "Total assets unchanged after recall" "$EXPECTED_TOTAL" "$TOTAL_AFTER_RECALL"

# --- 8j2: Withdraw by asset amount (user) ---
echo ""
log "8j2: User withdraws a small amount by asset value"
WITHDRAW_ASSET_AMOUNT=10000000  # 1 XLM
# Measure from vault's perspective: the vault doesn't pay the tx fee, so its
# balance change equals the exact transferred amount. Measuring from the user's
# side would include the native-XLM tx fee (the SAC wraps native XLM), making
# the balance delta slightly less than the requested amount.
VAULT_BALANCE_BEFORE_W=$(invoke_read "$TOKEN_ID" balance --id "$VAULT_ID")
SHARES_BEFORE_W=$(invoke_read "$VAULT_ID" balance --account "$USER_ADDRESS")

invoke_mut_as "$USER_IDENTITY" "$VAULT_ID" withdraw \
    --assets "$WITHDRAW_ASSET_AMOUNT" \
    --receiver "$USER_ADDRESS" \
    --owner "$USER_ADDRESS" \
    --operator "$USER_ADDRESS"

VAULT_BALANCE_AFTER_W=$(invoke_read "$TOKEN_ID" balance --id "$VAULT_ID")
SHARES_AFTER_W=$(invoke_read "$VAULT_ID" balance --account "$USER_ADDRESS")
VAULT_OUTFLOW_W=$((VAULT_BALANCE_BEFORE_W - VAULT_BALANCE_AFTER_W))
SHARES_BURNED_W=$((SHARES_BEFORE_W - SHARES_AFTER_W))

assert_eq "Withdraw transferred correct asset amount" "$WITHDRAW_ASSET_AMOUNT" "$VAULT_OUTFLOW_W"
assert_gt "Withdraw burned shares" "$SHARES_BURNED_W" "0"

# --- 8j3: Remove subaccount ---
echo ""
log "8j3: Remove subaccount (no deployed funds)"
invoke_mut "$VAULT_ID" remove_subaccount \
    --admin "$ADMIN_ADDRESS" \
    --subaccount "$STRATEGY_ID"

SUBACCOUNTS_AFTER_REMOVE=$(invoke_read "$VAULT_ID" get_subaccounts)
assert_not_contains "Strategy removed from subaccounts list" "$SUBACCOUNTS_AFTER_REMOVE" "$STRATEGY_ID"

# --- 8k: Redeem shares (user) ---
echo ""
log "8k: User redeems all shares"
SHARES_TO_REDEEM=$(invoke_read "$VAULT_ID" balance --account "$USER_ADDRESS")
log "User has $SHARES_TO_REDEEM shares to redeem"

# Check how many assets the user will receive
PREVIEW=$(invoke_read "$VAULT_ID" preview_redeem --shares "$SHARES_TO_REDEEM")
log "Preview redeem: $PREVIEW assets for $SHARES_TO_REDEEM shares"

# Measure from the vault's side: the vault doesn't pay the tx fee, so its
# balance delta equals the exact assets transferred. The user's own balance
# delta includes the native-XLM tx fee (SAC wraps native XLM) and would
# under-report by ~tens-of-thousands of stroops.
VAULT_BALANCE_BEFORE_R=$(invoke_read "$TOKEN_ID" balance --id "$VAULT_ID")

invoke_mut_as "$USER_IDENTITY" "$VAULT_ID" redeem \
    --shares "$SHARES_TO_REDEEM" \
    --receiver "$USER_ADDRESS" \
    --owner "$USER_ADDRESS" \
    --operator "$USER_ADDRESS"

VAULT_BALANCE_AFTER_R=$(invoke_read "$TOKEN_ID" balance --id "$VAULT_ID")
RECEIVED=$((VAULT_BALANCE_BEFORE_R - VAULT_BALANCE_AFTER_R))

# --- 8l: Verify final balances ---
echo ""
log "8l: Verify final balances"
# Under F1, direct donations don't accrue to NAV (see 8g), so the redeem
# returns approximately what the user put in minus the earlier withdrawal —
# no simulated yield. A few stroops of share-rounding dust may be left
# behind in the vault.
TOTAL_USER_INPUT=$((DEPOSIT_AMOUNT + MINT_ASSET_COST - WITHDRAW_ASSET_AMOUNT))
DUST_TOLERANCE=100
assert_ge "User received ~their remaining deposit (no profit under F1)" \
    "$RECEIVED" "$((TOTAL_USER_INPUT - DUST_TOLERANCE))"
assert_ge "User did not receive more than their remaining deposit" \
    "$TOTAL_USER_INPUT" "$RECEIVED"

FINAL_SHARES=$(invoke_read "$VAULT_ID" balance --account "$USER_ADDRESS")
assert_eq "User has 0 shares after full redeem" "0" "$FINAL_SHARES"

FINAL_TOTAL=$(invoke_read "$VAULT_ID" total_assets)
# Vault should have negligible dust (due to rounding), but total assets ~ 0 or small
log "Vault residual total_assets: $FINAL_TOTAL (dust from rounding)"

# --- 8m: Pause and verify operations blocked ---
echo ""
log "8m: Pause vault and verify operations blocked"
invoke_mut "$VAULT_ID" pause --admin "$ADMIN_ADDRESS"

IS_PAUSED=$(invoke_read "$VAULT_ID" is_paused)
assert_eq "Vault is paused" "true" "$IS_PAUSED"

# All user operations should fail when paused
assert_fails "Deposit rejected while paused" \
    "$USER_IDENTITY" "$VAULT_ID" deposit \
    --assets 1000 --receiver "$USER_ADDRESS" --from "$USER_ADDRESS" --operator "$USER_ADDRESS"

assert_fails "Mint rejected while paused" \
    "$USER_IDENTITY" "$VAULT_ID" mint \
    --shares 1000 --receiver "$USER_ADDRESS" --from "$USER_ADDRESS" --operator "$USER_ADDRESS"

assert_fails "Withdraw rejected while paused" \
    "$USER_IDENTITY" "$VAULT_ID" withdraw \
    --assets 1000 --receiver "$USER_ADDRESS" --owner "$USER_ADDRESS" --operator "$USER_ADDRESS"

assert_fails "Redeem rejected while paused" \
    "$USER_IDENTITY" "$VAULT_ID" redeem \
    --shares 1000 --receiver "$USER_ADDRESS" --owner "$USER_ADDRESS" --operator "$USER_ADDRESS"

# max_* should return 0 when paused
MAX_DEP_PAUSED=$(invoke_read "$VAULT_ID" max_deposit --receiver "$USER_ADDRESS")
assert_eq "max_deposit is 0 when paused" "0" "$MAX_DEP_PAUSED"

MAX_MINT_PAUSED=$(invoke_read "$VAULT_ID" max_mint --receiver "$USER_ADDRESS")
assert_eq "max_mint is 0 when paused" "0" "$MAX_MINT_PAUSED"

MAX_WITHDRAW_PAUSED=$(invoke_read "$VAULT_ID" max_withdraw --owner "$USER_ADDRESS")
assert_eq "max_withdraw is 0 when paused" "0" "$MAX_WITHDRAW_PAUSED"

MAX_REDEEM_PAUSED=$(invoke_read "$VAULT_ID" max_redeem --owner "$USER_ADDRESS")
assert_eq "max_redeem is 0 when paused" "0" "$MAX_REDEEM_PAUSED"

# Re-add subaccount so we can test operator functions while paused
invoke_mut "$VAULT_ID" unpause --admin "$ADMIN_ADDRESS"
invoke_mut "$VAULT_ID" add_subaccount \
    --admin "$ADMIN_ADDRESS" \
    --subaccount "$STRATEGY_ID" \
    --subaccount_type '{"Strategy":[]}'
invoke_mut "$VAULT_ID" pause --admin "$ADMIN_ADDRESS"

# Operator deposit_to_subaccount should fail when paused
assert_fails "deposit_to_subaccount rejected while paused" \
    "$OPERATOR_IDENTITY" "$VAULT_ID" deposit_to_subaccount \
    --operator "$OPERATOR_ADDRESS" --subaccount "$STRATEGY_ID" --amount 1000

# Operator withdraw_from_subaccount should fail when paused
assert_fails "withdraw_from_subaccount rejected while paused" \
    "$OPERATOR_IDENTITY" "$VAULT_ID" withdraw_from_subaccount \
    --operator "$OPERATOR_ADDRESS" --subaccount "$STRATEGY_ID" --amount 1000

# update_deployed_assets should SUCCEED when paused (intentional — AUM reconciliation
# must remain available during emergencies)
TESTS_RUN=$((TESTS_RUN + 1))
if invoke_raw "$OPERATOR_IDENTITY" "$VAULT_ID" update_deployed_assets \
    --operator "$OPERATOR_ADDRESS" \
    --amount 0 >/dev/null 2>&1; then
    ok "update_deployed_assets works while paused (by design)"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    fail "update_deployed_assets should succeed while paused"
    exit 1
fi

# --- 8n: Unpause and verify operations resume ---
echo ""
log "8n: Unpause vault and verify operations resume"
invoke_mut "$VAULT_ID" unpause --admin "$ADMIN_ADDRESS"

IS_PAUSED=$(invoke_read "$VAULT_ID" is_paused)
assert_eq "Vault is unpaused" "false" "$IS_PAUSED"

# Small deposit should work after unpause
invoke_mut_as "$USER_IDENTITY" "$VAULT_ID" deposit \
    --assets 10000000 \
    --receiver "$USER_ADDRESS" \
    --from "$USER_ADDRESS" \
    --operator "$USER_ADDRESS"

TESTS_RUN=$((TESTS_RUN + 1))
NEW_SHARES=$(invoke_read "$VAULT_ID" balance --account "$USER_ADDRESS")
if [[ "$NEW_SHARES" =~ ^[0-9]+$ ]] && [ "$NEW_SHARES" -gt 0 ]; then
    ok "Deposit works after unpause (shares=$NEW_SHARES)"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    fail "Deposit should work after unpause (shares=$NEW_SHARES)"
    exit 1
fi

# --- 8o: AUM limits enforcement ---
echo ""
log "8o: Verify AUM rate limits"

# Set restrictive limits (1% increase, 1% decrease)
invoke_mut "$VAULT_ID" set_aum_limits \
    --admin "$ADMIN_ADDRESS" \
    --increase_bps 100 \
    --decrease_bps 100

AUM_INC=$(invoke_read "$VAULT_ID" get_aum_increase_limit)
assert_eq "AUM increase limit is 100 bps" "100" "$AUM_INC"

AUM_DEC=$(invoke_read "$VAULT_ID" get_aum_decrease_limit)
assert_eq "AUM decrease limit is 100 bps" "100" "$AUM_DEC"

# Reset to 100% for remaining tests
invoke_mut "$VAULT_ID" set_aum_limits \
    --admin "$ADMIN_ADDRESS" \
    --increase_bps 10000 \
    --decrease_bps 10000

# --- 8o2: Cumulative AUM window limits ---
echo ""
log "8o2: Verify cumulative AUM window limits"

# Read current window config (relaxed to 100% in setup)
WINDOW_DUR=$(invoke_read "$VAULT_ID" get_aum_window_duration)
assert_eq "Window duration is 86400 (24h)" "86400" "$WINDOW_DUR"

WINDOW_INC=$(invoke_read "$VAULT_ID" get_aum_window_inc_limit)
assert_eq "Window increase limit is 10000 bps (relaxed)" "10000" "$WINDOW_INC"

WINDOW_DEC=$(invoke_read "$VAULT_ID" get_aum_window_dec_limit)
assert_eq "Window decrease limit is 10000 bps (relaxed)" "10000" "$WINDOW_DEC"

# Set custom window limits
invoke_mut "$VAULT_ID" set_aum_window_limits \
    --admin "$ADMIN_ADDRESS" \
    --window_duration 43200 \
    --cumulative_increase_bps 2000 \
    --cumulative_decrease_bps 1000

WINDOW_DUR=$(invoke_read "$VAULT_ID" get_aum_window_duration)
assert_eq "Custom window duration is 43200 (12h)" "43200" "$WINDOW_DUR"

WINDOW_INC=$(invoke_read "$VAULT_ID" get_aum_window_inc_limit)
assert_eq "Custom cumulative increase limit is 2000 bps" "2000" "$WINDOW_INC"

WINDOW_DEC=$(invoke_read "$VAULT_ID" get_aum_window_dec_limit)
assert_eq "Custom cumulative decrease limit is 1000 bps" "1000" "$WINDOW_DEC"

# Non-admin cannot set window limits
assert_fails "set_aum_window_limits rejects non-admin" \
    "$USER_IDENTITY" "$VAULT_ID" set_aum_window_limits \
    --admin "$USER_ADDRESS" --window_duration 86400 \
    --cumulative_increase_bps 5000 --cumulative_decrease_bps 5000

# Reset to 100% window limits for remaining tests
invoke_mut "$VAULT_ID" set_aum_window_limits \
    --admin "$ADMIN_ADDRESS" \
    --window_duration 86400 \
    --cumulative_increase_bps 10000 \
    --cumulative_decrease_bps 10000

# --- 8p: Upgrade contract and verify state preservation ---
echo ""
log "8p: Upload new WASM and upgrade contract"

# Record state before upgrade
PRE_UPGRADE_NAME=$(invoke_read "$VAULT_ID" name)
PRE_UPGRADE_SYMBOL=$(invoke_read "$VAULT_ID" symbol)
PRE_UPGRADE_TOTAL_ASSETS=$(invoke_read "$VAULT_ID" total_assets)
PRE_UPGRADE_TOTAL_SUPPLY=$(invoke_read "$VAULT_ID" total_supply)
PRE_UPGRADE_OPERATOR=$(invoke_read "$VAULT_ID" get_operator)
PRE_UPGRADE_SUBACCOUNTS=$(invoke_read "$VAULT_ID" get_subaccounts)
PRE_UPGRADE_USER_SHARES=$(invoke_read "$VAULT_ID" balance --account "$USER_ADDRESS")

log "State before upgrade:"
log "  name=$PRE_UPGRADE_NAME symbol=$PRE_UPGRADE_SYMBOL"
log "  total_assets=$PRE_UPGRADE_TOTAL_ASSETS total_supply=$PRE_UPGRADE_TOTAL_SUPPLY"
log "  user_shares=$PRE_UPGRADE_USER_SHARES"

# Install WASM (upload to ledger) — returns the hash
WASM_HASH=$(stellar contract install \
    --wasm "$VAULT_WASM" \
    --source-account "$ADMIN_IDENTITY" \
    --rpc-url "$RPC_URL" \
    --network-passphrase "$NETWORK_PASSPHRASE" 2>"$STDERR_FILE") || {
    fail "WASM install failed: $(cat "$STDERR_FILE")"
    exit 1
}
log "WASM hash: $WASM_HASH"

# Upgrade the vault (admin-only operation)
invoke_mut "$VAULT_ID" upgrade \
    --new_wasm_hash "$WASM_HASH" \
    --operator "$ADMIN_ADDRESS"

log "Upgrade invocation succeeded"

# Verify state is preserved after upgrade
POST_UPGRADE_NAME=$(invoke_read "$VAULT_ID" name)
POST_UPGRADE_SYMBOL=$(invoke_read "$VAULT_ID" symbol)
POST_UPGRADE_TOTAL_ASSETS=$(invoke_read "$VAULT_ID" total_assets)
POST_UPGRADE_TOTAL_SUPPLY=$(invoke_read "$VAULT_ID" total_supply)
POST_UPGRADE_OPERATOR=$(invoke_read "$VAULT_ID" get_operator)
POST_UPGRADE_SUBACCOUNTS=$(invoke_read "$VAULT_ID" get_subaccounts)
POST_UPGRADE_USER_SHARES=$(invoke_read "$VAULT_ID" balance --account "$USER_ADDRESS")

assert_eq "Name preserved after upgrade" "$PRE_UPGRADE_NAME" "$POST_UPGRADE_NAME"
assert_eq "Symbol preserved after upgrade" "$PRE_UPGRADE_SYMBOL" "$POST_UPGRADE_SYMBOL"
assert_eq "Total assets preserved after upgrade" "$PRE_UPGRADE_TOTAL_ASSETS" "$POST_UPGRADE_TOTAL_ASSETS"
assert_eq "Total supply preserved after upgrade" "$PRE_UPGRADE_TOTAL_SUPPLY" "$POST_UPGRADE_TOTAL_SUPPLY"
assert_eq "Operator preserved after upgrade" "$PRE_UPGRADE_OPERATOR" "$POST_UPGRADE_OPERATOR"
assert_eq "Subaccounts preserved after upgrade" "$PRE_UPGRADE_SUBACCOUNTS" "$POST_UPGRADE_SUBACCOUNTS"
assert_eq "User shares preserved after upgrade" "$PRE_UPGRADE_USER_SHARES" "$POST_UPGRADE_USER_SHARES"

# --- 8q: Verify vault still functions after upgrade ---
echo ""
log "8q: Verify vault operations work after upgrade"

# User should be able to redeem their remaining shares
if [ "$POST_UPGRADE_USER_SHARES" -gt 0 ]; then
    USER_BALANCE_BEFORE_REDEEM=$(invoke_read "$TOKEN_ID" balance --id "$USER_ADDRESS")
    invoke_mut_as "$USER_IDENTITY" "$VAULT_ID" redeem \
        --shares "$POST_UPGRADE_USER_SHARES" \
        --receiver "$USER_ADDRESS" \
        --owner "$USER_ADDRESS" \
        --operator "$USER_ADDRESS"
    USER_BALANCE_AFTER_REDEEM=$(invoke_read "$TOKEN_ID" balance --id "$USER_ADDRESS")
    REDEEMED=$((USER_BALANCE_AFTER_REDEEM - USER_BALANCE_BEFORE_REDEEM))

    TESTS_RUN=$((TESTS_RUN + 1))
    if [ "$REDEEMED" -gt 0 ]; then
        ok "Redeem works after upgrade (received=$REDEEMED)"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        fail "Redeem after upgrade returned 0"
        exit 1
    fi
fi

# --- 8r: Verify upgrade rejected for non-admin ---
echo ""
log "8r: Verify upgrade rejected for non-admin"

assert_fails "Upgrade rejected for non-admin" \
    "$USER_IDENTITY" "$VAULT_ID" upgrade \
    --new_wasm_hash "$WASM_HASH" --operator "$USER_ADDRESS"

# --- 8s: Vault extend_ttl ---
echo ""
log "8s: Vault extend_ttl"
invoke_mut "$VAULT_ID" extend_ttl
TESTS_RUN=$((TESTS_RUN + 1))
ok "Vault extend_ttl succeeded"
TESTS_PASSED=$((TESTS_PASSED + 1))

# =====================================================================
# Phase 9: XLM Strategy test scenarios
# =====================================================================
phase 9 "XLM Strategy test scenarios"

# Re-deposit into vault so we have funds to work with for strategy tests
echo ""
log "9a: Prepare — deposit into vault for strategy tests"
invoke_mut_as "$USER_IDENTITY" "$VAULT_ID" deposit \
    --assets "$DEPOSIT_AMOUNT" \
    --receiver "$USER_ADDRESS" \
    --from "$USER_ADDRESS" \
    --operator "$USER_ADDRESS"

# Deploy capital to strategy via vault
echo ""
log "9b: Deploy capital to strategy via vault"
invoke_mut_as "$OPERATOR_IDENTITY" "$VAULT_ID" deposit_to_subaccount \
    --operator "$OPERATOR_ADDRESS" \
    --subaccount "$STRATEGY_ID" \
    --amount "$DEPLOY_AMOUNT"

STRATEGY_BALANCE=$(invoke_read "$STRATEGY_ID" get_balance)
assert_eq "Strategy holds deployed capital" "$DEPLOY_AMOUNT" "$STRATEGY_BALANCE"

# --- 9c: deploy_to_protocol ---
echo ""
log "9c: Controller deploys from strategy to protocol"
invoke_mut_as "$CONTROLLER_IDENTITY" "$STRATEGY_ID" deploy_to_protocol \
    --controller "$CONTROLLER_ADDRESS" \
    --protocol "$PROTOCOL_ADDRESS" \
    --amount "$PROTOCOL_DEPLOY_AMOUNT"

# get_balance() = idle + deployed_total = (DEPLOY-PROTOCOL) + PROTOCOL = DEPLOY (unchanged)
STRATEGY_BALANCE_AFTER_DEPLOY=$(invoke_read "$STRATEGY_ID" get_balance)
assert_eq "Strategy get_balance unchanged after deploy_to_protocol (idle+deployed)" "$DEPLOY_AMOUNT" "$STRATEGY_BALANCE_AFTER_DEPLOY"

STRATEGY_DEPLOYED_TOTAL=$(invoke_read "$STRATEGY_ID" get_deployed_total)
assert_eq "Strategy deployed_total tracks protocol deployment" "$PROTOCOL_DEPLOY_AMOUNT" "$STRATEGY_DEPLOYED_TOTAL"

PROTOCOL_BALANCE=$(invoke_read "$TOKEN_ID" balance --id "$PROTOCOL_ADDRESS")
assert_ge "Protocol received tokens" "$PROTOCOL_BALANCE" "$PROTOCOL_DEPLOY_AMOUNT"

# --- 9d: recall_from_protocol ---
# `recall_from_protocol` requires both controller auth AND protocol auth
# (for the inner token.transfer(protocol → strategy)). The CLI signs only
# as the --source-account, so in production this needs a multi-sig flow.
# Under F1, a raw transfer from the protocol back to the strategy would
# be classified as a donation and NOT reduce `deployed_total`, so we must
# exercise the real API. We temporarily rotate the controller to the
# protocol address so a single identity can satisfy both auths.
echo ""
log "9d: Recall from protocol via recall_from_protocol"

RECALL_AMOUNT=100000000  # 10 XLM

invoke_mut_as "$CONTROLLER_IDENTITY" "$STRATEGY_ID" set_controller \
    --current "$CONTROLLER_ADDRESS" \
    --new_controller "$PROTOCOL_ADDRESS"

invoke_mut_as "$PROTOCOL_IDENTITY" "$STRATEGY_ID" recall_from_protocol \
    --controller "$PROTOCOL_ADDRESS" \
    --protocol "$PROTOCOL_ADDRESS" \
    --amount "$RECALL_AMOUNT"

invoke_mut_as "$PROTOCOL_IDENTITY" "$STRATEGY_ID" set_controller \
    --current "$PROTOCOL_ADDRESS" \
    --new_controller "$CONTROLLER_ADDRESS"

# Recall moves capital from deployed_total (bookkeeping) into local_balance
# (vault-tracked idle); get_balance = local_balance + deployed_total is
# unchanged — a recall is a transfer, not yield.
STRATEGY_BALANCE_AFTER_RECALL=$(invoke_read "$STRATEGY_ID" get_balance)
assert_eq "Strategy get_balance unchanged after recall" \
    "$DEPLOY_AMOUNT" "$STRATEGY_BALANCE_AFTER_RECALL"

STRATEGY_DEPLOYED_AFTER_RECALL=$(invoke_read "$STRATEGY_ID" get_deployed_total)
assert_eq "deployed_total decremented by recall amount" \
    "$((PROTOCOL_DEPLOY_AMOUNT - RECALL_AMOUNT))" "$STRATEGY_DEPLOYED_AFTER_RECALL"

# --- 9e: deploy_to_protocol access control ---
echo ""
log "9e: Strategy access control"

# Non-controller cannot deploy
assert_fails "deploy_to_protocol rejects non-controller" \
    "$USER_IDENTITY" "$STRATEGY_ID" deploy_to_protocol \
    --controller "$USER_ADDRESS" --protocol "$PROTOCOL_ADDRESS" --amount 1000

# Cannot deploy to self (strategy address)
assert_fails "deploy_to_protocol rejects self as target" \
    "$CONTROLLER_IDENTITY" "$STRATEGY_ID" deploy_to_protocol \
    --controller "$CONTROLLER_ADDRESS" --protocol "$STRATEGY_ID" --amount 1000

# Cannot deploy to vault
assert_fails "deploy_to_protocol rejects vault as target" \
    "$CONTROLLER_IDENTITY" "$STRATEGY_ID" deploy_to_protocol \
    --controller "$CONTROLLER_ADDRESS" --protocol "$VAULT_ID" --amount 1000

# Cannot deploy zero or negative amount
assert_fails "deploy_to_protocol rejects zero amount" \
    "$CONTROLLER_IDENTITY" "$STRATEGY_ID" deploy_to_protocol \
    --controller "$CONTROLLER_ADDRESS" --protocol "$PROTOCOL_ADDRESS" --amount 0

# Cannot deploy more than idle balance (deploy_to_protocol checks token balance, not get_balance)
CURRENT_IDLE_BALANCE=$(invoke_read "$TOKEN_ID" balance --id "$STRATEGY_ID")
OVER_AMOUNT=$((CURRENT_IDLE_BALANCE + 1))
assert_fails "deploy_to_protocol rejects insufficient balance" \
    "$CONTROLLER_IDENTITY" "$STRATEGY_ID" deploy_to_protocol \
    --controller "$CONTROLLER_ADDRESS" --protocol "$PROTOCOL_ADDRESS" --amount "$OVER_AMOUNT"

# Non-vault cannot call deposit on strategy
assert_fails "strategy deposit rejects non-vault caller" \
    "$USER_IDENTITY" "$STRATEGY_ID" deposit \
    --from "$USER_ADDRESS" --amount 1000

# Non-vault cannot call withdraw on strategy
assert_fails "strategy withdraw rejects non-vault caller" \
    "$USER_IDENTITY" "$STRATEGY_ID" withdraw \
    --to "$USER_ADDRESS" --amount 1000

# --- 9f: set_controller ---
echo ""
log "9f: Controller rotation"

# Non-controller cannot rotate
assert_fails "set_controller rejects non-controller" \
    "$USER_IDENTITY" "$STRATEGY_ID" set_controller \
    --current "$USER_ADDRESS" --new_controller "$USER_ADDRESS"

# Cannot set controller to vault
assert_fails "set_controller rejects vault as new controller" \
    "$CONTROLLER_IDENTITY" "$STRATEGY_ID" set_controller \
    --current "$CONTROLLER_ADDRESS" --new_controller "$VAULT_ID"

# Cannot set controller to strategy itself
assert_fails "set_controller rejects strategy as new controller" \
    "$CONTROLLER_IDENTITY" "$STRATEGY_ID" set_controller \
    --current "$CONTROLLER_ADDRESS" --new_controller "$STRATEGY_ID"

# Valid controller rotation
invoke_mut_as "$CONTROLLER_IDENTITY" "$STRATEGY_ID" set_controller \
    --current "$CONTROLLER_ADDRESS" \
    --new_controller "$ADMIN_ADDRESS"

NEW_CTRL=$(invoke_read "$STRATEGY_ID" get_controller)
assert_eq "Controller rotated to admin" "$ADMIN_ADDRESS" "$NEW_CTRL"

# Old controller can no longer deploy
assert_fails "Old controller cannot deploy after rotation" \
    "$CONTROLLER_IDENTITY" "$STRATEGY_ID" deploy_to_protocol \
    --controller "$CONTROLLER_ADDRESS" --protocol "$PROTOCOL_ADDRESS" --amount 1000

# Rotate back for remaining tests
invoke_mut "$STRATEGY_ID" set_controller \
    --current "$ADMIN_ADDRESS" \
    --new_controller "$CONTROLLER_ADDRESS"

RESTORED_CTRL=$(invoke_read "$STRATEGY_ID" get_controller)
assert_eq "Controller restored" "$CONTROLLER_ADDRESS" "$RESTORED_CTRL"

# --- 9g: Full lifecycle — recall remaining protocol funds, withdraw to vault ---
echo ""
log "9g: Full lifecycle — recall remaining protocol funds, withdraw to vault"

# Recall the rest via recall_from_protocol (same controller-rotation trick
# as 9d so a single identity satisfies both auths).
REMAINING_AT_PROTOCOL=$((PROTOCOL_DEPLOY_AMOUNT - RECALL_AMOUNT))
if [ "$REMAINING_AT_PROTOCOL" -gt 0 ]; then
    invoke_mut_as "$CONTROLLER_IDENTITY" "$STRATEGY_ID" set_controller \
        --current "$CONTROLLER_ADDRESS" \
        --new_controller "$PROTOCOL_ADDRESS"

    invoke_mut_as "$PROTOCOL_IDENTITY" "$STRATEGY_ID" recall_from_protocol \
        --controller "$PROTOCOL_ADDRESS" \
        --protocol "$PROTOCOL_ADDRESS" \
        --amount "$REMAINING_AT_PROTOCOL"

    invoke_mut_as "$PROTOCOL_IDENTITY" "$STRATEGY_ID" set_controller \
        --current "$PROTOCOL_ADDRESS" \
        --new_controller "$CONTROLLER_ADDRESS"
fi

STRATEGY_DEPLOYED_AFTER_FULL_RECALL=$(invoke_read "$STRATEGY_ID" get_deployed_total)
assert_eq "deployed_total fully drained by recall" "0" "$STRATEGY_DEPLOYED_AFTER_FULL_RECALL"

# All recalled capital is now in local_balance, so the vault can withdraw it.
STRATEGY_IDLE=$(invoke_read "$TOKEN_ID" balance --id "$STRATEGY_ID")
assert_gt "Strategy has idle tokens to withdraw" "$STRATEGY_IDLE" "0"

invoke_mut_as "$OPERATOR_IDENTITY" "$VAULT_ID" withdraw_from_subaccount \
    --operator "$OPERATOR_ADDRESS" \
    --subaccount "$STRATEGY_ID" \
    --amount "$STRATEGY_IDLE"

STRATEGY_IDLE_AFTER=$(invoke_read "$TOKEN_ID" balance --id "$STRATEGY_ID")
assert_eq "Strategy idle balance is 0 after withdrawal" "0" "$STRATEGY_IDLE_AFTER"

STRATEGY_REMAINING=$(invoke_read "$STRATEGY_ID" get_balance)
assert_eq "Strategy get_balance is 0 after full recall + withdraw" "0" "$STRATEGY_REMAINING"

VAULT_DEPLOYED=$(invoke_read "$VAULT_ID" get_deployed_assets)
assert_eq "Vault deployed_assets is 0 (strategy ops don't change it)" "0" "$VAULT_DEPLOYED"

# --- 9h: Strategy extend_ttl ---
echo ""
log "9h: Strategy extend_ttl"
invoke_mut "$STRATEGY_ID" extend_ttl
TESTS_RUN=$((TESTS_RUN + 1))
ok "Strategy extend_ttl succeeded"
TESTS_PASSED=$((TESTS_PASSED + 1))

# =====================================================================
# Phase 9.5: Additional vault admin scenarios
# =====================================================================
phase "9.5" "Additional vault admin scenarios"

# --- Two-step admin transfer (C-1) ---
echo ""
log "9.5a: Two-step admin transfer"

# Create a new identity for the new admin
NEW_ADMIN_IDENTITY="e2e-new-admin"
stellar keys rm "$NEW_ADMIN_IDENTITY" 2>/dev/null || true
stellar keys generate "$NEW_ADMIN_IDENTITY" || { fail "Could not generate $NEW_ADMIN_IDENTITY key"; exit 1; }
NEW_ADMIN_ADDRESS=$(get_address "$NEW_ADMIN_IDENTITY")
fund_account "$NEW_ADMIN_IDENTITY" "$NEW_ADMIN_ADDRESS"
log "Created and funded new-admin: $NEW_ADMIN_ADDRESS"

# Propose new admin
DEADLINE=$(($(date +%s) + 604800))  # 1 week from now
invoke_mut "$VAULT_ID" propose_admin \
    --admin "$ADMIN_ADDRESS" \
    --new_admin "$NEW_ADMIN_ADDRESS" \
    --deadline "$DEADLINE"

PENDING=$(invoke_read "$VAULT_ID" get_pending_admin)
assert_contains "Pending admin is set" "$PENDING" "$NEW_ADMIN_ADDRESS"

# Non-pending address cannot accept (auth will fail for wrong signer)
TESTS_RUN=$((TESTS_RUN + 1))
if invoke_raw "$USER_IDENTITY" "$VAULT_ID" accept_admin >/dev/null 2>&1; then
    fail "accept_admin should reject wrong signer"
    exit 1
else
    ok "accept_admin correctly rejects wrong signer"
    TESTS_PASSED=$((TESTS_PASSED + 1))
fi

# Pending admin accepts
invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" accept_admin

NEW_ADMIN_CHECK=$(invoke_read "$VAULT_ID" get_admin)
assert_eq "Admin transferred to new address" "$NEW_ADMIN_ADDRESS" "$NEW_ADMIN_CHECK"

# Old admin cannot perform admin actions anymore
TESTS_RUN=$((TESTS_RUN + 1))
if invoke_raw "$ADMIN_IDENTITY" "$VAULT_ID" pause \
    --admin "$ADMIN_ADDRESS" >/dev/null 2>&1; then
    fail "Old admin should no longer be able to pause"
    exit 1
else
    ok "Old admin correctly rejected after transfer"
    TESTS_PASSED=$((TESTS_PASSED + 1))
fi

# New admin can pause
invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" pause \
    --admin "$NEW_ADMIN_ADDRESS"
IS_PAUSED=$(invoke_read "$VAULT_ID" is_paused)
assert_eq "New admin can pause vault" "true" "$IS_PAUSED"

# --- Pause restricts update_deployed_assets increases (H-2) ---
echo ""
log "9.5b: Pause restricts update_deployed_assets increases"

# Set deployed_assets to a baseline, then pause and test restrictions.
invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" unpause \
    --admin "$NEW_ADMIN_ADDRESS"

# Fund vault and deploy to strategy (for later tests)
invoke_mut_as "$USER_IDENTITY" "$VAULT_ID" deposit \
    --assets 100000000 \
    --receiver "$USER_ADDRESS" \
    --from "$USER_ADDRESS" \
    --operator "$USER_ADDRESS"

invoke_mut_as "$OPERATOR_IDENTITY" "$VAULT_ID" deposit_to_subaccount \
    --operator "$OPERATOR_ADDRESS" \
    --subaccount "$STRATEGY_ID" \
    --amount 50000000

# Set deployed_assets to 50M (strategy deposits don't change it in the new model)
invoke_mut_as "$OPERATOR_IDENTITY" "$VAULT_ID" update_deployed_assets \
    --operator "$OPERATOR_ADDRESS" \
    --amount 50000000

DEPLOYED_BEFORE_PAUSE=$(invoke_read "$VAULT_ID" get_deployed_assets)
log "Deployed before pause: $DEPLOYED_BEFORE_PAUSE"

invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" pause \
    --admin "$NEW_ADMIN_ADDRESS"

# Decrease should succeed while paused
DECREASED_AMOUNT=$((DEPLOYED_BEFORE_PAUSE - 1000000))
invoke_mut_as "$OPERATOR_IDENTITY" "$VAULT_ID" update_deployed_assets \
    --operator "$OPERATOR_ADDRESS" \
    --amount "$DECREASED_AMOUNT"

DEPLOYED_AFTER_DECREASE=$(invoke_read "$VAULT_ID" get_deployed_assets)
assert_eq "Decrease reconciliation works while paused" "$DECREASED_AMOUNT" "$DEPLOYED_AFTER_DECREASE"

# Increase should be blocked while paused
INCREASED_AMOUNT=$((DECREASED_AMOUNT + 2000000))
TESTS_RUN=$((TESTS_RUN + 1))
if invoke_raw "$OPERATOR_IDENTITY" "$VAULT_ID" update_deployed_assets \
    --operator "$OPERATOR_ADDRESS" \
    --amount "$INCREASED_AMOUNT" >/dev/null 2>&1; then
    fail "update_deployed_assets increase should be blocked while paused"
    exit 1
else
    ok "update_deployed_assets increase correctly blocked while paused"
    TESTS_PASSED=$((TESTS_PASSED + 1))
fi

# Unpause for remaining tests
invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" unpause \
    --admin "$NEW_ADMIN_ADDRESS"

# --- Subaccount ops exempt from AUM rate limits (H-3) ---
echo ""
log "9.5c: Subaccount deposit/withdraw not subject to AUM rate limits"

# Set tight AUM limits — these should NOT affect subaccount operations
invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" set_aum_limits \
    --admin "$NEW_ADMIN_ADDRESS" \
    --increase_bps 500 \
    --decrease_bps 500

CURRENT_DEPLOYED=$(invoke_read "$VAULT_ID" get_deployed_assets)
log "Current deployed: $CURRENT_DEPLOYED"

# Strategy deposits/withdrawals don't change deployed_assets and aren't rate-limited.
# Verify a large deposit + withdrawal succeed without AUM rate limit issues.
LARGE_DEPOSIT=10000000  # 1 XLM
invoke_mut_as "$OPERATOR_IDENTITY" "$VAULT_ID" deposit_to_subaccount \
    --operator "$OPERATOR_ADDRESS" \
    --subaccount "$STRATEGY_ID" \
    --amount "$LARGE_DEPOSIT"

DEPLOYED_AFTER=$(invoke_read "$VAULT_ID" get_deployed_assets)
assert_eq "Strategy deposit doesn't change deployed_assets" "$CURRENT_DEPLOYED" "$DEPLOYED_AFTER"

# Large withdrawal should also SUCCEED
invoke_mut_as "$OPERATOR_IDENTITY" "$VAULT_ID" withdraw_from_subaccount \
    --operator "$OPERATOR_ADDRESS" \
    --subaccount "$STRATEGY_ID" \
    --amount "$LARGE_DEPOSIT"

DEPLOYED_AFTER_WITHDRAW=$(invoke_read "$VAULT_ID" get_deployed_assets)
assert_eq "Strategy withdrawal doesn't change deployed_assets" "$CURRENT_DEPLOYED" "$DEPLOYED_AFTER_WITHDRAW"

# Restore relaxed limits for cleanup
invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" set_aum_limits \
    --admin "$NEW_ADMIN_ADDRESS" \
    --increase_bps 10000 \
    --decrease_bps 10000

invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" set_aum_window_limits \
    --admin "$NEW_ADMIN_ADDRESS" \
    --window_duration 86400 \
    --cumulative_increase_bps 10000 \
    --cumulative_decrease_bps 10000

# --- Cancel admin proposal ---
echo ""
log "9.5d: Cancel admin proposal"

ANOTHER_ADMIN_ADDRESS=$(get_address "$USER_IDENTITY")
DEADLINE=$(($(date +%s) + 604800))  # 1 week from now
invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" propose_admin \
    --admin "$NEW_ADMIN_ADDRESS" \
    --new_admin "$ANOTHER_ADMIN_ADDRESS" \
    --deadline "$DEADLINE"

PENDING=$(invoke_read "$VAULT_ID" get_pending_admin)
assert_contains "Pending admin set for cancellation test" "$PENDING" "$ANOTHER_ADMIN_ADDRESS"

invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" cancel_admin_proposal \
    --admin "$NEW_ADMIN_ADDRESS"

PENDING_AFTER_CANCEL=$(invoke_read "$VAULT_ID" get_pending_admin)
TESTS_RUN=$((TESTS_RUN + 1))
# After cancellation, get_pending_admin should return None/null
if [ "$PENDING_AFTER_CANCEL" = "null" ] || [ -z "$PENDING_AFTER_CANCEL" ]; then
    ok "Admin proposal cancelled successfully"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    fail "Pending admin should be None after cancellation, got: $PENDING_AFTER_CANCEL"
    exit 1
fi

# ---------------------------------------------------------------------------
# Phase 9.6: Wallet subaccount lifecycle
# ---------------------------------------------------------------------------
# Exercises the full wallet lifecycle under the strict accounting model:
#
#   - `update_wallet_deployed(operator, wallet, new_tracked)` is the sole
#     entry point for recognising gains/losses on a Wallet. It moves both
#     the per-wallet tracker AND the aggregate `deployed_assets` by the
#     same delta atomically, subject to AUM rate limits and pause
#     semantics. Keeping the two in sync is load-bearing:
#     `withdraw_from_subaccount` rejects any pull that exceeds the
#     per-wallet tracker (panics with `WalletOverWithdraw` #23), which
#     prevents over-pulls from silently consuming other wallets' share
#     of `deployed_assets`.
#
#   - `remove_wallet_and_reconcile(admin, operator, wallet, new_total)`
#     is the atomic combined op: write down the aggregate AND remove the
#     wallet in one transaction. Closes the pricing window of the older
#     two-step flow (`update_deployed_assets` + `remove_subaccount`).
#
#   - `seed_wallet_net_deployed` overwrites the tracker for a Wallet
#     without touching `deployed_assets` (admin-only). Useful when
#     registering a wallet that already holds capital recorded elsewhere
#     in the aggregate, or for manual accounting corrections. Unlike
#     `update_wallet_deployed` (which moves both tracker and aggregate
#     together), seeding leaves the aggregate alone — the caller must
#     ensure the recognised value is already present.

phase "9.6" "Wallet subaccount lifecycle"

# --- 9.6a: Create an ephemeral wallet identity and register it ---
echo ""
log "9.6a: Register a Wallet subaccount"

WALLET_IDENTITY="e2e-wallet"
stellar keys rm "$WALLET_IDENTITY" 2>/dev/null || true
stellar keys generate "$WALLET_IDENTITY" || { fail "Could not generate $WALLET_IDENTITY key"; exit 1; }
WALLET_ADDRESS=$(get_address "$WALLET_IDENTITY")
fund_account "$WALLET_IDENTITY" "$WALLET_ADDRESS"
log "Created and funded wallet: $WALLET_ADDRESS"

invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" add_subaccount \
    --admin "$NEW_ADMIN_ADDRESS" \
    --subaccount "$WALLET_ADDRESS" \
    --subaccount_type '{"Wallet":[]}'

SUBACCOUNTS=$(invoke_read "$VAULT_ID" get_subaccounts)
assert_contains "Wallet is in subaccounts list" "$SUBACCOUNTS" "$WALLET_ADDRESS"

WALLET_TYPE=$(invoke_read "$VAULT_ID" get_subaccount_type --subaccount "$WALLET_ADDRESS")
assert_contains "Subaccount type reports 'Wallet'" "$WALLET_TYPE" "Wallet"

# Freshly registered wallet has no tracker entry (reads as 0).
TRACKER_INIT=$(invoke_read "$VAULT_ID" get_wallet_net_deployed --subaccount "$WALLET_ADDRESS")
assert_eq "Tracker starts at 0 for fresh wallet" "0" "$TRACKER_INIT"

# --- 9.6b: Deposit into the wallet ---
echo ""
log "9.6b: Operator deposits capital into the wallet subaccount"

WALLET_DEPOSIT_AMOUNT=10000000  # 1 XLM in stroops
DEPLOYED_BEFORE_WALLET=$(invoke_read "$VAULT_ID" get_deployed_assets)
log "deployed_assets before wallet deposit: $DEPLOYED_BEFORE_WALLET"

WALLET_BALANCE_BEFORE=$(invoke_read "$TOKEN_ID" balance --id "$WALLET_ADDRESS")

invoke_mut_as "$OPERATOR_IDENTITY" "$VAULT_ID" deposit_to_subaccount \
    --operator "$OPERATOR_ADDRESS" \
    --subaccount "$WALLET_ADDRESS" \
    --amount "$WALLET_DEPOSIT_AMOUNT"

WALLET_BALANCE_AFTER=$(invoke_read "$TOKEN_ID" balance --id "$WALLET_ADDRESS")
WALLET_INFLOW=$((WALLET_BALANCE_AFTER - WALLET_BALANCE_BEFORE))
assert_eq "Wallet received exactly the deposited amount" "$WALLET_DEPOSIT_AMOUNT" "$WALLET_INFLOW"

DEPLOYED_AFTER_DEPOSIT=$(invoke_read "$VAULT_ID" get_deployed_assets)
EXPECTED_DEPLOYED_AFTER=$((DEPLOYED_BEFORE_WALLET + WALLET_DEPOSIT_AMOUNT))
assert_eq "deployed_assets incremented by wallet deposit" \
    "$EXPECTED_DEPLOYED_AFTER" "$DEPLOYED_AFTER_DEPOSIT"

TRACKER_AFTER_DEPOSIT=$(invoke_read "$VAULT_ID" get_wallet_net_deployed --subaccount "$WALLET_ADDRESS")
assert_eq "Per-wallet tracker matches the deposit" \
    "$WALLET_DEPOSIT_AMOUNT" "$TRACKER_AFTER_DEPOSIT"

# --- 9.6b.2: Exercise withdraw_from_subaccount on the Wallet ---
# The Wallet path is pull-model: the wallet owner must `approve` the
# vault as spender, then the operator invokes `withdraw_from_subaccount`
# which routes through `transfer_from`. Both `deployed_assets` and the
# per-wallet tracker decrement by `actual_received`. The pull must stay
# within the tracker; otherwise `WalletOverWithdraw`
# (#23) fires. After asserting the decrement, redeposit the same amount
# so the downstream assertions in 9.6c–9.6d operate on the original state.
echo ""
log "9.6b.2: Operator partial-withdraws via transfer_from; tracker/deployed both decrement"

WALLET_PARTIAL_PULL=3000000  # 0.3 XLM in stroops

# The token caps `live_until` at roughly current_ledger + max_instance_ttl.
# Query the latest ledger and add a buffer large enough to outlast the rest
# of the test but well inside the cap.
LATEST_LEDGER=$(curl -sf "$RPC_URL" \
    -X POST \
    -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"getLatestLedger"}' \
    | grep -oE '"sequence":[0-9]+' | head -1 | cut -d: -f2)
if [ -z "$LATEST_LEDGER" ]; then
    fail "Could not fetch latest ledger for approve expiration"
    exit 1
fi
APPROVE_EXPIRATION_LEDGER=$((LATEST_LEDGER + 100000))

invoke_mut_as "$WALLET_IDENTITY" "$TOKEN_ID" approve \
    --from "$WALLET_ADDRESS" \
    --spender "$VAULT_ID" \
    --amount "$WALLET_PARTIAL_PULL" \
    --expiration_ledger "$APPROVE_EXPIRATION_LEDGER"

invoke_mut_as "$OPERATOR_IDENTITY" "$VAULT_ID" withdraw_from_subaccount \
    --operator "$OPERATOR_ADDRESS" \
    --subaccount "$WALLET_ADDRESS" \
    --amount "$WALLET_PARTIAL_PULL"

DEPLOYED_AFTER_PULL=$(invoke_read "$VAULT_ID" get_deployed_assets)
EXPECTED_DEPLOYED_AFTER_PULL=$((DEPLOYED_AFTER_DEPOSIT - WALLET_PARTIAL_PULL))
assert_eq "deployed_assets decremented by partial pull" \
    "$EXPECTED_DEPLOYED_AFTER_PULL" "$DEPLOYED_AFTER_PULL"

TRACKER_AFTER_PULL=$(invoke_read "$VAULT_ID" get_wallet_net_deployed --subaccount "$WALLET_ADDRESS")
EXPECTED_TRACKER_AFTER_PULL=$((WALLET_DEPOSIT_AMOUNT - WALLET_PARTIAL_PULL))
assert_eq "Tracker decremented by partial pull" \
    "$EXPECTED_TRACKER_AFTER_PULL" "$TRACKER_AFTER_PULL"

# Restore state for 9.6c by re-depositing the pulled amount. After this,
# deployed_assets and tracker both match their post-9.6b values, so the
# downstream assertions that reference DEPLOYED_AFTER_DEPOSIT /
# WALLET_DEPOSIT_AMOUNT remain valid without modification.
invoke_mut_as "$OPERATOR_IDENTITY" "$VAULT_ID" deposit_to_subaccount \
    --operator "$OPERATOR_ADDRESS" \
    --subaccount "$WALLET_ADDRESS" \
    --amount "$WALLET_PARTIAL_PULL"

DEPLOYED_AFTER_REDEPOSIT=$(invoke_read "$VAULT_ID" get_deployed_assets)
assert_eq "deployed_assets restored after redeposit" \
    "$DEPLOYED_AFTER_DEPOSIT" "$DEPLOYED_AFTER_REDEPOSIT"

TRACKER_AFTER_REDEPOSIT=$(invoke_read "$VAULT_ID" get_wallet_net_deployed --subaccount "$WALLET_ADDRESS")
assert_eq "Tracker restored after redeposit" \
    "$WALLET_DEPOSIT_AMOUNT" "$TRACKER_AFTER_REDEPOSIT"

# --- 9.6c: Operator recognises a gain on the wallet's capital ---
echo ""
log "9.6c: Operator recognises a 0.5 XLM gain via update_wallet_deployed"

WALLET_GAIN_AMOUNT=5000000  # 0.5 XLM in stroops
WALLET_VALUE_WITH_GAIN=$((WALLET_DEPOSIT_AMOUNT + WALLET_GAIN_AMOUNT))
DEPLOYED_WITH_GAIN=$((DEPLOYED_AFTER_DEPOSIT + WALLET_GAIN_AMOUNT))

# Admin runbook step: the 5M gain (9.6c) and the 15M write-off (9.6d) can
# both exceed the default 10%/5% AUM rate limits when the baseline
# deployed_assets is small. Widen the limits here so phase 9.6 is
# self-contained AND the test explicitly exercises the documented
# "widen → operate → restore" admin flow. The prior limits are captured
# so the restore at the end of the phase is exact.
PRIOR_INCREASE_BPS=$(invoke_read "$VAULT_ID" get_aum_increase_limit)
PRIOR_DECREASE_BPS=$(invoke_read "$VAULT_ID" get_aum_decrease_limit)
log "Captured prior AUM limits: +${PRIOR_INCREASE_BPS} / -${PRIOR_DECREASE_BPS} bps"

invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" set_aum_limits \
    --admin "$NEW_ADMIN_ADDRESS" \
    --increase_bps 10000 \
    --decrease_bps 10000

# The tracker AND aggregate must move together on a gain recognition,
# otherwise a later withdraw_from_subaccount would reject with
# WalletOverWithdraw. `update_wallet_deployed` moves both atomically.
invoke_mut_as "$OPERATOR_IDENTITY" "$VAULT_ID" update_wallet_deployed \
    --operator "$OPERATOR_ADDRESS" \
    --subaccount "$WALLET_ADDRESS" \
    --new_tracked "$WALLET_VALUE_WITH_GAIN"

DEPLOYED_AFTER_GAIN=$(invoke_read "$VAULT_ID" get_deployed_assets)
assert_eq "deployed_assets reflects the recognised gain" \
    "$DEPLOYED_WITH_GAIN" "$DEPLOYED_AFTER_GAIN"

TRACKER_AFTER_GAIN=$(invoke_read "$VAULT_ID" get_wallet_net_deployed --subaccount "$WALLET_ADDRESS")
assert_eq "Tracker moved in lockstep with deployed_assets" \
    "$WALLET_VALUE_WITH_GAIN" "$TRACKER_AFTER_GAIN"

# --- 9.6d: Atomic remove + reconcile — closes the pricing window ---
echo ""
log "9.6d: Atomic remove_wallet_and_reconcile"

# `remove_wallet_and_reconcile` requires BOTH admin (whitelist authority)
# and operator (AUM authority) auth. The `stellar contract invoke
# --source-account` flag signs only one identity; on production networks
# with distinct admin/operator keys, callers must construct a multi-sig
# transaction out-of-app (Lab, CLI). For this e2e we take the
# single-key configuration: temporarily rotate the operator to
# NEW_ADMIN so the single-signer invoke works (the contract skips the
# duplicate `require_auth` when admin == operator per the I-3 fix).
# After the atomic call, we restore the original operator.
CAPTURED_OPERATOR_ADDRESS="$OPERATOR_ADDRESS"
invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" set_operator \
    --admin "$NEW_ADMIN_ADDRESS" \
    --new_operator "$NEW_ADMIN_ADDRESS"

invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" remove_wallet_and_reconcile \
    --admin "$NEW_ADMIN_ADDRESS" \
    --operator "$NEW_ADMIN_ADDRESS" \
    --subaccount "$WALLET_ADDRESS" \
    --new_deployed_total "$DEPLOYED_BEFORE_WALLET"

# Restore operator immediately so subsequent operator-only phases
# (if any were added later) keep working.
invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" set_operator \
    --admin "$NEW_ADMIN_ADDRESS" \
    --new_operator "$CAPTURED_OPERATOR_ADDRESS"

SUBACCOUNTS_AFTER_REMOVE=$(invoke_read "$VAULT_ID" get_subaccounts)
assert_not_contains "Wallet removed from subaccounts list" \
    "$SUBACCOUNTS_AFTER_REMOVE" "$WALLET_ADDRESS"

DEPLOYED_AFTER_REMOVE=$(invoke_read "$VAULT_ID" get_deployed_assets)
assert_eq "deployed_assets reconciled atomically to pre-wallet baseline" \
    "$DEPLOYED_BEFORE_WALLET" "$DEPLOYED_AFTER_REMOVE"

TRACKER_AFTER_REMOVE=$(invoke_read "$VAULT_ID" get_wallet_net_deployed --subaccount "$WALLET_ADDRESS")
assert_eq "Per-wallet tracker cleared on removal" "0" "$TRACKER_AFTER_REMOVE"

# Restore the AUM limits captured before 9.6c. Final half of the
# documented "widen → operate → restore" admin flow — ensures phase 9.6
# leaves the limits exactly as it found them rather than silently
# widening them for the rest of the test run.
invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" set_aum_limits \
    --admin "$NEW_ADMIN_ADDRESS" \
    --increase_bps "$PRIOR_INCREASE_BPS" \
    --decrease_bps "$PRIOR_DECREASE_BPS"

RESTORED_INCREASE_BPS=$(invoke_read "$VAULT_ID" get_aum_increase_limit)
RESTORED_DECREASE_BPS=$(invoke_read "$VAULT_ID" get_aum_decrease_limit)
assert_eq "AUM increase limit restored to pre-9.6c value" \
    "$PRIOR_INCREASE_BPS" "$RESTORED_INCREASE_BPS"
assert_eq "AUM decrease limit restored to pre-9.6c value" \
    "$PRIOR_DECREASE_BPS" "$RESTORED_DECREASE_BPS"

# --- 9.6f: seed_wallet_net_deployed round-trip + guards ---
echo ""
log "9.6f: seed_wallet_net_deployed round-trip and guard assertions"

# Re-add the wallet so the seed function has a valid target.
invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" add_subaccount \
    --admin "$NEW_ADMIN_ADDRESS" \
    --subaccount "$WALLET_ADDRESS" \
    --subaccount_type '{"Wallet":[]}'

# Seed the tracker to a specific value (simulates a post-upgrade backfill).
SEED_AMOUNT=42000000  # arbitrary, chosen not to collide with prior values
invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" seed_wallet_net_deployed \
    --admin "$NEW_ADMIN_ADDRESS" \
    --subaccount "$WALLET_ADDRESS" \
    --amount "$SEED_AMOUNT"

SEEDED_TRACKER=$(invoke_read "$VAULT_ID" get_wallet_net_deployed --subaccount "$WALLET_ADDRESS")
assert_eq "Tracker reflects the seeded value" "$SEED_AMOUNT" "$SEEDED_TRACKER"

# Guard: negative amounts rejected.
assert_fails "seed_wallet_net_deployed rejects negative amount" \
    "$NEW_ADMIN_IDENTITY" "$VAULT_ID" seed_wallet_net_deployed \
    --admin "$NEW_ADMIN_ADDRESS" \
    --subaccount "$WALLET_ADDRESS" \
    --amount "-1"

# Guard: non-admin rejected.
assert_fails "seed_wallet_net_deployed rejects non-admin" \
    "$USER_IDENTITY" "$VAULT_ID" seed_wallet_net_deployed \
    --admin "$USER_ADDRESS" \
    --subaccount "$WALLET_ADDRESS" \
    --amount "$SEED_AMOUNT"

# Guard: seeding a Strategy subaccount rejected (STRATEGY_ID is still
# whitelisted from phase 9.5 setup).
assert_fails "seed_wallet_net_deployed rejects Strategy subaccount" \
    "$NEW_ADMIN_IDENTITY" "$VAULT_ID" seed_wallet_net_deployed \
    --admin "$NEW_ADMIN_ADDRESS" \
    --subaccount "$STRATEGY_ID" \
    --amount "$SEED_AMOUNT"

# Guard: seeding an unregistered address rejected.
UNREGISTERED_ADDRESS=$(get_address "$PROFIT_IDENTITY")
assert_fails "seed_wallet_net_deployed rejects unregistered address" \
    "$NEW_ADMIN_IDENTITY" "$VAULT_ID" seed_wallet_net_deployed \
    --admin "$NEW_ADMIN_ADDRESS" \
    --subaccount "$UNREGISTERED_ADDRESS" \
    --amount "$SEED_AMOUNT"

# --- 9.6g: Cleanup ---
echo ""
log "9.6g: Remove the re-added wallet and delete ephemeral identity"

# 9.6f seeded the wallet's tracker to a non-zero value, so the plain
# `remove_subaccount` path would trip the `WalletTrackerNotZero` (#24)
# guard. Use `remove_wallet_and_reconcile` to close the position atomically
# — same single-key operator-rotation workaround used in 9.6d.
CAPTURED_OPERATOR_ADDRESS_96G=$(invoke_read "$VAULT_ID" get_operator)
invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" set_operator \
    --admin "$NEW_ADMIN_ADDRESS" \
    --new_operator "$NEW_ADMIN_ADDRESS"

DEPLOYED_BEFORE_96G=$(invoke_read "$VAULT_ID" get_deployed_assets)
# Pass the current aggregate as `new_deployed_total` so delta = 0 and the
# call bypasses AUM rate/cumulative-window checks. 9.6d already drew
# heavily on the decrease window; writing down another 42M here would
# trip the cumulative limit. This is cleanup-only — the residual 42M in
# the aggregate is untested because no assertions follow 9.6g.
invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" remove_wallet_and_reconcile \
    --admin "$NEW_ADMIN_ADDRESS" \
    --operator "$NEW_ADMIN_ADDRESS" \
    --subaccount "$WALLET_ADDRESS" \
    --new_deployed_total "$DEPLOYED_BEFORE_96G"

invoke_mut_as "$NEW_ADMIN_IDENTITY" "$VAULT_ID" set_operator \
    --admin "$NEW_ADMIN_ADDRESS" \
    --new_operator "$CAPTURED_OPERATOR_ADDRESS_96G"

SUBACCOUNTS_FINAL=$(invoke_read "$VAULT_ID" get_subaccounts)
assert_not_contains "Wallet removed at end of phase 9.6" \
    "$SUBACCOUNTS_FINAL" "$WALLET_ADDRESS"

stellar keys rm "$WALLET_IDENTITY" 2>/dev/null || true

# Clean up new admin identity
stellar keys rm "$NEW_ADMIN_IDENTITY" 2>/dev/null || true

# ---------------------------------------------------------------------------
# Phase 10: Summary
# ---------------------------------------------------------------------------
phase 10 "Summary"

echo ""
if [ "$TESTS_PASSED" -eq "$TESTS_RUN" ]; then
    echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${GREEN}  ALL $TESTS_RUN TESTS PASSED${NC}"
    echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
else
    echo -e "${RED}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${RED}  FAILED: $TESTS_PASSED/$TESTS_RUN tests passed${NC}"
    echo -e "${RED}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    exit 1
fi
