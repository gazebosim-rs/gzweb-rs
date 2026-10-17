#!/usr/bin/env bash
# Records a corpus from real Gazebo (the gz-websocket image, see Dockerfile.gazebo).
#   parity/record.sh <world.sdf> [corpus-name] [extra gz-parity record flags...]
# Writes tests/corpus/<corpus-name>/ (default: the world file's basename).
set -euo pipefail

WORLD=${1:?usage: parity/record.sh <world.sdf> [corpus-name] [flags...]}
NAME=${2:-$(basename "$WORLD" .sdf)}
shift $(( $# >= 2 ? 2 : 1 ))
ROOT=$(cd "$(dirname "$0")/.." && pwd)
PORT=${RECORD_PORT:-9002}
IMAGE=${GZ_IMAGE:-gz-websocket}
CONTAINER=gzweb-rs-record-$NAME

cargo build -q -p gz-parity --manifest-path "$ROOT/Cargo.toml"
docker run -d --rm --name "$CONTAINER" -p "$PORT:9002" "$IMAGE" "$WORLD" >/dev/null
trap 'docker stop -t 2 "$CONTAINER" >/dev/null 2>&1 || true' EXIT

for _ in $(seq 1 120); do
  (echo >/dev/tcp/127.0.0.1/"$PORT") 2>/dev/null && break
  sleep 0.5
done
# The websocket server accepts before the world is loaded; give it time.
sleep "${RECORD_WARMUP_S:-30}"

"$ROOT/target/debug/gz-parity" record --url "ws://127.0.0.1:$PORT" \
  --out "$ROOT/tests/corpus/$NAME" --world-file "$WORLD" \
  --max-stream-frames 100 --timeout-ms 60000 "$@"
