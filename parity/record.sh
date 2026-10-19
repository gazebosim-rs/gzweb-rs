#!/usr/bin/env bash
# Records a corpus from real Gazebo (the gz-websocket-jetty image, see
# Dockerfile.gazebo).
#   parity/record.sh <world.sdf> [corpus-name] [extra gz-parity record flags...]
# Writes tests/corpus/<corpus-name>/ (default: the world file's basename).
# RECORD_PORT (default 9010) is the host port; GZ_IMAGE and GZ_RELEASE name the
# image and the release written to the manifest's source.gazebo.
set -euo pipefail

WORLD=${1:?usage: parity/record.sh <world.sdf> [corpus-name] [flags...]}
NAME=${2:-$(basename "$WORLD" .sdf)}
shift $(( $# >= 2 ? 2 : 1 ))
ROOT=$(cd "$(dirname "$0")/.." && pwd)
PORT=${RECORD_PORT:-9010}
IMAGE=${GZ_IMAGE:-gz-websocket-jetty}
RELEASE=${GZ_RELEASE:-jetty}
CONTAINER=gzweb-rs-record-$NAME

cargo build -q -p gz-parity --manifest-path "$ROOT/Cargo.toml"
docker run -d --rm --name "$CONTAINER" -p "$PORT:9002" "$IMAGE" "$WORLD" >/dev/null
trap 'docker stop -t 2 "$CONTAINER" >/dev/null 2>&1 || true' EXIT

# Wait for a websocket upgrade (HTTP 101), not just a TCP accept: docker's
# port proxy accepts before the container listens, and Jetty's websocket
# system only starts once the world, and any Fuel models it includes, has
# loaded. Up to 5 minutes.
ws_up() {
  [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 1 -H 'Connection: Upgrade' \
    -H 'Upgrade: websocket' -H 'Sec-WebSocket-Version: 13' \
    -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' "http://127.0.0.1:$PORT/" || true)" = 101 ]
}
for _ in $(seq 1 300); do
  ws_up && break
  sleep 1
done
ws_up || { echo "the websocket server on port $PORT did not start" >&2; docker logs "$CONTAINER" 2>&1 | tail -20 >&2; exit 1; }
# Let the simulation run a while before recording, as the earlier
# recordings did.
sleep "${RECORD_WARMUP_S:-30}"

"$ROOT/target/debug/gz-parity" record --url "ws://127.0.0.1:$PORT" \
  --out "$ROOT/tests/corpus/$NAME" --world-file "$WORLD" \
  --gazebo "$RELEASE" --max-stream-frames 100 --timeout-ms 60000 "$@"
