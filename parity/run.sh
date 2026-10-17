#!/usr/bin/env bash
# Runs the gzweb vs gzweb-rs parity check for one recorded corpus.
#   parity/run.sh <corpus-name> [--stream-limit N]
# Outputs land in target/parity/<corpus-name>/. See .agent/PARITY.md.
set -euo pipefail

CORPUS=${1:?usage: parity/run.sh <corpus-name> [--stream-limit N]}
shift
ROOT=$(cd "$(dirname "$0")/.." && pwd)
CORPUS_DIR=$ROOT/tests/corpus/$CORPUS
OUT=$ROOT/target/parity/$CORPUS
PORT=${PARITY_PORT:-9100}
ALLOW=$ROOT/tests/parity/allowlist.toml
GZWEB_DIR=${GZWEB_DIR:-$ROOT/../gzweb}
mkdir -p "$OUT"

cargo build -q -p gz-parity -p gzweb --manifest-path "$ROOT/Cargo.toml"
PARITY=$ROOT/target/debug/gz-parity
GZWEB_RS=$ROOT/target/debug/gzweb

if [ ! -f "$GZWEB_DIR/dist/gzweb.module.js" ]; then
  echo "gzweb is not built: (cd $GZWEB_DIR && npm ci && npm run build)" >&2
  exit 2
fi
if [ ! -d "$ROOT/parity/js/node_modules" ]; then
  (cd "$ROOT/parity/js" && npm ci --silent)
fi

replay() { # <client-log>
  "$PARITY" replay --corpus "$CORPUS_DIR" --port "$PORT" --client-log "$1" --connections 1 "$@" \
    >"$1.replay.txt" 2>&1 &
  REPLAY_PID=$!
  for _ in $(seq 1 100); do
    (echo >/dev/tcp/127.0.0.1/"$PORT") 2>/dev/null && return 0
    sleep 0.05
  done
  echo "replay server did not start" >&2
  return 1
}
EXTRA=("$@")
replay_args() { [ ${#EXTRA[@]} -gt 0 ] && printf '%s\n' "${EXTRA[@]}"; true; }
trap '[ -n "${REPLAY_PID:-}" ] && kill "$REPLAY_PID" 2>/dev/null || true' EXIT

status=0

echo "== gzweb (reference)"
replay "$OUT/gzweb.client.log" $(replay_args)
node "$ROOT/parity/js/snapshot.mjs" --url "ws://127.0.0.1:$PORT" --gzweb "$GZWEB_DIR" \
  --out "$OUT/gzweb.json" --screenshot "$OUT/gzweb.png" --console-log "$OUT/gzweb.console.txt" \
  || status=$?
kill "$REPLAY_PID" 2>/dev/null || true; wait "$REPLAY_PID" 2>/dev/null || true

echo "== gzweb-rs"
replay "$OUT/rs.client.log" $(replay_args)
RUST_LOG=${RUST_LOG:-warn} "$GZWEB_RS" --url "ws://127.0.0.1:$PORT" --snapshot "$OUT/rs.json" \
  --settle-ms 1000 --timeout-ms 60000 >"$OUT/rs.log" 2>&1 || status=$?
kill "$REPLAY_PID" 2>/dev/null || true; wait "$REPLAY_PID" 2>/dev/null || true
[ $status -ne 0 ] && { echo "a snapshot step failed (exit $status); see $OUT" >&2; exit $status; }

echo "== compare snapshots"
"$PARITY" compare --expected "$OUT/gzweb.json" --actual "$OUT/rs.json" --allow "$ALLOW" \
  --report "$OUT/snapshot-report.json" || status=1
echo "== compare protocol"
"$PARITY" compare-protocol "$OUT/gzweb.client.log" "$OUT/rs.client.log" \
  --report "$OUT/protocol-report.json" || status=1
exit $status
