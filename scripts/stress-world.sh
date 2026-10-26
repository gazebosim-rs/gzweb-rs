#!/usr/bin/env bash
# A world for measuring the client at scale (UI_PLAN M10e: a 5,000-entity
# Scene tree at 60 fps), written to stdout.
#
#   scripts/stress-world.sh [MODELS] [--dynamic] > target/stress.sdf
#
# MODELS (default 1250) models in a square grid, each a base and an arm
# link (a box visual each) and the revolute joint between them: four tree
# entities a model, so the default's tree has 5,000 entities plus the sun
# and the ground plane's two. The models are static unless --dynamic, so
# the server sends no pose stream; with it, each base is fixed to the world
# by a joint (a fifth entity) and each arm swings, so the server streams
# every link's pose.
#
# Serve it with the Jetty image (README "Running Gazebo (Docker)"):
#
#   docker run --rm -p 127.0.0.1:9040:9002 -v "$PWD/target:/w:ro" \
#     gz-websocket-jetty-pub /w/stress.sdf
set -euo pipefail

models=1250
static=true
for arg in "$@"; do
  case "$arg" in
    --dynamic) static=false ;;
    '' | *[!0-9]*)
      echo "usage: scripts/stress-world.sh [MODELS] [--dynamic]" >&2
      exit 2
      ;;
    *) models=$arg ;;
  esac
done

# The grid's side, and the spacing between models in centimetres.
side=1
while ((side * side < models)); do side=$((side + 1)); done
step=150
# Centimetres as metres.
metres() {
  local cm=${1#-} sign=
  [ "$1" = "$cm" ] || sign=-
  printf '%s%d.%02d' "$sign" $((cm / 100)) $((cm % 100))
}

cat <<EOF
<?xml version="1.0"?>
<sdf version="1.10">
  <world name="stress">
    <light type="directional" name="sun">
      <cast_shadows>true</cast_shadows>
      <pose>0 0 10 0 0 0</pose>
      <diffuse>0.8 0.8 0.8 1</diffuse>
      <direction>-0.5 0.1 -0.9</direction>
    </light>
    <model name="ground_plane">
      <static>true</static>
      <link name="link">
        <collision name="collision">
          <geometry><plane><normal>0 0 1</normal><size>200 200</size></plane></geometry>
        </collision>
        <visual name="visual">
          <geometry><plane><normal>0 0 1</normal><size>200 200</size></plane></geometry>
          <material><diffuse>0.8 0.8 0.8 1</diffuse></material>
        </visual>
      </link>
    </model>
EOF

for ((i = 0; i < models; i++)); do
  x=$(metres $(((i % side - side / 2) * step)))
  y=$(metres $(((i / side - side / 2) * step)))
  fixed=
  if [ "$static" = false ]; then
    fixed='<joint name="fixed" type="fixed"><parent>world</parent><child>base</child></joint>'
  fi
  cat <<EOF
    <model name="unit_$i">
      <static>$static</static>
      <pose>$x $y 0 0 0 0</pose>
      <link name="base">
        <pose>0 0 0.25 0 0 0</pose>
        <visual name="visual">
          <geometry><box><size>0.5 0.5 0.5</size></box></geometry>
          <material><diffuse>0.3 0.5 0.8 1</diffuse></material>
        </visual>
      </link>
      <link name="arm">
        <pose>0 0 0.75 0 0 0</pose>
        <visual name="visual">
          <geometry><box><size>0.1 0.1 0.5</size></box></geometry>
          <material><diffuse>0.9 0.6 0.2 1</diffuse></material>
        </visual>
      </link>
      <joint name="hinge" type="revolute">
        <parent>base</parent>
        <child>arm</child>
        <pose>0 0 -0.25 0 0 0</pose>
        <axis><xyz>1 0 0</xyz></axis>
      </joint>
      $fixed
    </model>
EOF
done

cat <<EOF
  </world>
</sdf>
EOF
