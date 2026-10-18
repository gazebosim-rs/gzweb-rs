#!/usr/bin/env bash
# Reproduce the spike: fetch corpus, run both loaders, compare, run Bevy test.
set -euo pipefail
cd "$(dirname "$0")"
./fetch.sh
[ -d js/node_modules ] || (cd js && npm ci --no-audit --no-fund)
cargo build --release -q
mkdir -p out
FILES=$(find corpus ../../crates/gz-collada/tests/data -name '*.dae' | sort)
./target/release/collada_spike --runs 5 $FILES > out/rust.jsonl
node js/ref.mjs $FILES > out/js.jsonl 2> out/js.stderr
python3 -I compare.py out/rust.jsonl out/js.jsonl > out/compare.md
cargo test --release -q -- --nocapture
echo "see out/compare.md"
