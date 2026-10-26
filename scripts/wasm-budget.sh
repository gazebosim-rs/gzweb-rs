#!/usr/bin/env bash
# The web build's payload budget (design/ui/DECISIONS.md D-120): the
# release gzweb_bg.wasm, gzipped as a server sends it, must stay within
# WASM_BUDGET_GZ bytes (default 14,000,000). build.sh runs it last.
#
#   scripts/wasm-budget.sh [path/to/gzweb_bg.wasm]
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
WASM=${1:-$ROOT/target/wasm32-unknown-unknown/release/gzweb_bg.wasm}
BUDGET=${WASM_BUDGET_GZ:-14000000}

if [ ! -f "$WASM" ]; then
  echo "wasm-budget.sh: no $WASM (run ./build.sh first)" >&2
  exit 2
fi

raw=$(stat -c %s "$WASM")
gz=$(gzip -9 -c "$WASM" | wc -c)
mb() { awk -v b="$1" 'BEGIN { printf "%.2f MB", b / 1e6 }'; }
echo "gzweb_bg.wasm: $(mb "$raw") raw, $(mb "$gz") gzipped (budget $(mb "$BUDGET"))"
if [ "$gz" -gt "$BUDGET" ]; then
  echo "wasm-budget.sh: over the budget by $(mb $((gz - BUDGET))) (D-120)" >&2
  exit 1
fi
