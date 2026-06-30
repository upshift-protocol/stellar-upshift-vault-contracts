#!/usr/bin/env bash
set -euo pipefail

echo "Running August Vault tests..."

cargo test --all -- --nocapture "$@"

echo ""
echo "All tests passed!"
