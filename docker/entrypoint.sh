#!/bin/bash
# docker run ... gz-websocket-jetty [world.sdf] [extra gz-sim-server flags...]
set -e
WORLD=${1:-empty.sdf}
shift || true
echo "Starting Gazebo Jetty (gz-sim server) with world: $WORLD"
exec /usr/libexec/gz/sim10/gz-sim-server -s -r -v "${GZ_VERBOSITY:-3}" "$WORLD" "$@"
