#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/deploy.sh <network> <source-identity> <asset-contract-id> [decimals-offset] [name] [symbol]
#   network:          testnet | mainnet
#   source:           Stellar identity name (configured via `stellar keys add`)
#   asset-contract:   Contract address of the underlying token
#   decimals-offset:  Virtual decimals offset (default: 6, min: 3, max: 10)
#   name:             Share token name (default: "August Vault Shares")
#   symbol:           Share token symbol (default: "avVAULT")
#
# Example:
#   ./scripts/deploy.sh testnet deployer CDLZ... 6
#   ./scripts/deploy.sh testnet deployer CDLZ... 6 "My Vault Shares" "avXLM"

NETWORK="${1:-testnet}"
SOURCE="${2:-deployer}"
ASSET="${3:?Error: asset contract ID is required}"
DECIMALS_OFFSET="${4:-6}"
NAME="${5:-August Vault Shares}"
SYMBOL="${6:-avVAULT}"

WASM_FILE="target/wasm32-unknown-unknown/release/august_vault.wasm"

echo "=== August Vault Deployment ==="
echo "  Network:         $NETWORK"
echo "  Source:           $SOURCE"
echo "  Asset:            $ASSET"
echo "  Decimals Offset:  $DECIMALS_OFFSET"
echo "  Name:             $NAME"
echo "  Symbol:           $SYMBOL"

# Validate decimals offset (contract requires 3-10)
if ! [[ "$DECIMALS_OFFSET" =~ ^[0-9]+$ ]] || [ "$DECIMALS_OFFSET" -lt 3 ] || [ "$DECIMALS_OFFSET" -gt 10 ]; then
    echo "Error: decimals-offset must be an integer between 3 and 10 (got: '$DECIMALS_OFFSET')"
    exit 1
fi

# Check prerequisites
if ! command -v stellar &> /dev/null; then
    echo "Error: stellar CLI is not installed."
    echo "Install it with: cargo install stellar-cli --locked"
    exit 1
fi

if [ ! -f "$WASM_FILE" ]; then
    echo "WASM not found. Building..."
    ./scripts/build.sh
fi

echo ""
echo "Deploying contract..."

CONTRACT_ID=$(stellar contract deploy \
    --wasm "$WASM_FILE" \
    --network "$NETWORK" \
    --source-account "$SOURCE" \
    -- \
    --name "$NAME" \
    --symbol "$SYMBOL" \
    --asset "$ASSET" \
    --decimals_offset "$DECIMALS_OFFSET")

echo ""
echo "=== Deployment Successful ==="
echo "  Contract ID: $CONTRACT_ID"
echo "  Network:     $NETWORK"
echo ""
echo "The vault is ready to accept deposits."
