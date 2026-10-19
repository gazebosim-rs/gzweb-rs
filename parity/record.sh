#!/usr/bin/env bash
# Records a corpus from real Gazebo (the gz-websocket-jetty image, see
# Dockerfile.gazebo).
#   parity/record.sh <world.sdf> [corpus-name] [extra gz-parity record flags...]
# Writes tests/corpus/<corpus-name>/ (default: the world file's basename).
# RECORD_PORT (default 9010) is the host port, bound to 127.0.0.1;
# RECORD_WARMUP_S (default 30) how long the world runs before recording;
# GZ_IMAGE the image. The manifest's source.gazebo is the image's release
# label and gz-sim version (e.g. "jetty 10.5.0"); GZ_RELEASE overrides it.
# Fuel models are cached in the docker volume GZ_FUEL_VOLUME (default
# gz-fuel).
set -euo pipefail

WORLD=${1:?usage: parity/record.sh <world.sdf> [corpus-name] [flags...]}
NAME=${2:-$(basename "$WORLD" .sdf)}
shift $(( $# >= 2 ? 2 : 1 ))
ROOT=$(cd "$(dirname "$0")/.." && pwd)
PORT=${RECORD_PORT:-9010}
IMAGE=${GZ_IMAGE:-gz-websocket-jetty}
FUEL=${GZ_FUEL_VOLUME:-gz-fuel}
CONTAINER=gzweb-rs-record-$NAME

# Record what actually runs: the image's release label and gz-sim version.
if [ -z "${GZ_RELEASE:-}" ]; then
  release=$(docker image inspect -f '{{index .Config.Labels "org.gazebosim.release"}}' "$IMAGE")
  version=$(docker run --rm --entrypoint sh "$IMAGE" -c '"$GZ_SIM_SERVER" --version')
  GZ_RELEASE="${release:-unknown} $version"
fi

cargo build -q -p gz-parity --manifest-path "$ROOT/Cargo.toml"
# No --rm: a server that dies at startup keeps its logs until the trap.
docker run -d --name "$CONTAINER" -p "127.0.0.1:$PORT:9002" \
  -v "$FUEL:/home/gazebo/.gz/fuel" "$IMAGE" "$WORLD" >/dev/null
trap 'docker rm -f "$CONTAINER" >/dev/null 2>&1 || true' EXIT

fail() {
  echo "$1" >&2
  docker logs "$CONTAINER" 2>&1 | tail -20 >&2
  exit 1
}

# Wait for a websocket upgrade (HTTP 101), not just a TCP accept: docker's
# port proxy accepts before the container listens, and Jetty's websocket
# system only starts once the world, and any Fuel models it includes, has
# loaded. Up to 5 minutes, unless the server exits first (a bad world path,
# for one).
ws_up() {
  [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 1 -H 'Connection: Upgrade' \
    -H 'Upgrade: websocket' -H 'Sec-WebSocket-Version: 13' \
    -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' "http://127.0.0.1:$PORT/" || true)" = 101 ]
}
for _ in $(seq 1 300); do
  ws_up && break
  [ "$(docker inspect -f '{{.State.Running}}' "$CONTAINER" 2>/dev/null)" = true ] ||
    fail "the Gazebo server exited before its websocket server started"
  sleep 1
done
ws_up || fail "the websocket server on port $PORT did not start"
# Let the simulation run before recording. The wait starts once the world
# has loaded, so sim time at the first recorded frame is about this long
# (more for Fuel worlds). Dynamic worlds drift: record those with a short
# warmup (heightmap used RECORD_WARMUP_S=5; see .agent/PARITY.md).
sleep "${RECORD_WARMUP_S:-30}"

"$ROOT/target/debug/gz-parity" record --url "ws://127.0.0.1:$PORT" \
  --out "$ROOT/tests/corpus/$NAME" --world-file "$WORLD" \
  --gazebo "$GZ_RELEASE" --max-stream-frames 100 --timeout-ms 60000 "$@"
