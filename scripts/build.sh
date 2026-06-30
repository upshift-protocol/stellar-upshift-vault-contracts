#!/usr/bin/env bash
set -euo pipefail

echo "Building August Vault + XLM strategy..."

# Build both contracts
cargo build --target wasm32-unknown-unknown --release

OUT_DIR="target/wasm32-unknown-unknown/release"
CONTRACTS=(august_vault xlm_strategy)

report_size() {
    local label="$1" file="$2"
    local size; size=$(wc -c < "$file")
    echo "  $label: $((size / 1024))KB ($size bytes) — $file"
}

for name in "${CONTRACTS[@]}"; do
    raw="$OUT_DIR/$name.wasm"
    if [ ! -f "$raw" ]; then
        echo "Error: WASM file not found at $raw" >&2
        exit 1
    fi
    report_size "raw" "$raw"
done

# Optimization is required for any on-chain deploy: rustc >= 1.95 emits
# call_indirect with padded LEB128 bytes that stellar-core's validator
# rejects as "reference-types not enabled". `stellar contract optimize`
# runs wasm-opt, which normalizes the encoding.
if ! command -v stellar &>/dev/null; then
    echo ""
    echo "  Warning: stellar CLI not found — skipping optimization."
    echo "  Deploy scripts require the optimized WASM; install stellar CLI"
    echo "  with 'cargo install stellar-cli --locked' before deploying."
    exit 0
fi

echo ""
echo "Optimizing WASMs..."
for name in "${CONTRACTS[@]}"; do
    raw="$OUT_DIR/$name.wasm"
    opt="$OUT_DIR/$name.optimized.wasm"
    stellar contract optimize --wasm "$raw" --wasm-out "$opt" >/dev/null
    report_size "optimized" "$opt"
done
