#!/usr/bin/env bash
# The project's quality gate, shared by pre-commit and CI.
#
#   scripts/check.sh            all checks (what CI runs)
#   scripts/check.sh fmt        rustfmt --check
#   scripts/check.sh clippy     clippy, native (all targets) and wasm32, -D warnings
#   scripts/check.sh doc        rustdoc, -D warnings
#   scripts/check.sh deny       cargo-deny: licences, bans, sources (offline);
#                               CHECK_ADVISORIES=1 adds the RustSec database
#   scripts/check.sh coverage   every test under cargo-llvm-cov; fails below
#                               COVERAGE_MIN % line coverage (default 95)
#
# Set CHECK_LOCKED=1 (CI does) to also require Cargo.lock to be up to date.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

COVERAGE_MIN=${COVERAGE_MIN:-95}
# Third-party code vendored as-is (mesh-loader's COLLADA parsers); its own
# tests and the corpus tests exercise it, but it isn't held to the minimum.
COVERAGE_IGNORE='crates/gz-collada/src/vendor/'
LOCKED=()
[ "${CHECK_LOCKED:-0}" = 1 ] && LOCKED=(--locked)
# Crates that build for the browser.
WASM_CRATES=(-p gz-core -p gz-bevy -p gzweb)

need_cargo() { # <subcommand> <how to install>
  cargo "$1" --version >/dev/null 2>&1 || {
    echo "check.sh: 'cargo $1' is missing; install it with: $2" >&2
    exit 2
  }
}

step() { printf '\n== %s\n' "$*"; }

fmt() {
  step "rustfmt"
  cargo fmt --all -- --check
}

clippy() {
  step "clippy (native, all targets)"
  cargo clippy "${LOCKED[@]}" --workspace --all-targets -- -D warnings
  step "clippy (wasm32)"
  if ! rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown; then
    echo "check.sh: wasm32 target missing; run: rustup target add wasm32-unknown-unknown" >&2
    exit 2
  fi
  cargo clippy "${LOCKED[@]}" "${WASM_CRATES[@]}" --target wasm32-unknown-unknown -- -D warnings
}

doc() {
  step "rustdoc"
  RUSTDOCFLAGS="-D warnings" cargo doc "${LOCKED[@]}" --workspace --no-deps
}

deny() {
  need_cargo deny "cargo install cargo-deny --locked"
  local checks=(licenses bans sources)
  [ "${CHECK_ADVISORIES:-0}" = 1 ] && checks+=(advisories)
  step "cargo-deny (${checks[*]})"
  cargo deny check "${checks[@]}"
}

coverage() {
  need_cargo llvm-cov "rustup component add llvm-tools-preview && cargo install cargo-llvm-cov --locked"
  step "tests + line coverage (minimum ${COVERAGE_MIN}%)"
  mkdir -p target/coverage
  # Includes the headless end-to-end tests, which run the instrumented
  # gzweb binary as a child process.
  cargo llvm-cov "${LOCKED[@]}" --workspace --ignore-filename-regex "$COVERAGE_IGNORE" \
    --lcov --output-path target/coverage/lcov.info
  # Prints the per-file table, then fails if the total is under the minimum.
  cargo llvm-cov report --summary-only --ignore-filename-regex "$COVERAGE_IGNORE" \
    --fail-under-lines "$COVERAGE_MIN"
}

case "${1:-all}" in
  fmt) fmt ;;
  clippy) clippy ;;
  doc) doc ;;
  deny) deny ;;
  coverage) coverage ;;
  all) fmt; clippy; doc; deny; coverage ;;
  *) echo "usage: scripts/check.sh [fmt|clippy|doc|deny|coverage|all]" >&2; exit 2 ;;
esac
