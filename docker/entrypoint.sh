#!/bin/bash
# docker run ... gz-websocket-jetty [world.sdf] [extra gz-sim-server flags...]
#
# GZ_SIM_SERVER_CONFIG_PATH (set in the image) replaces playback_server.config
# too, so `--playback <log>` would load Physics instead of LogPlayback; see
# README.md "Running Gazebo (Docker)".
set -e
WORLD=${1:-empty.sdf}
shift || true
echo "Starting Gazebo Jetty (gz-sim server) with world: $WORLD"
exec "${GZ_SIM_SERVER:-/usr/libexec/gz/sim10/gz-sim-server}" -s -r -v "${GZ_VERBOSITY:-3}" "$WORLD" "$@"
