#!/usr/bin/env bash
# Reproduces spikes/collada/corpus/ from Gazebo Fuel (meshes only; textures are
# not needed for the parse comparison, only their referenced paths).
set -euo pipefail
cd "$(dirname "$0")"
BASE=https://fuel.gazebosim.org/1.0
fetch() { # owner model path
  local owner="$1" model="$2" path="$3"
  local slug; slug=$(echo "$model" | tr ' ' '_')
  local out="corpus/${slug}/${path#/}"
  mkdir -p "$(dirname "$out")"
  [ -s "$out" ] && return 0
  local m; m=$(python3 -c 'import urllib.parse,sys;print(urllib.parse.quote(sys.argv[1]))' "$model")
  echo "GET $owner/$model$path"
  curl -sfL "$BASE/$owner/models/$m/tip/files$path" -o "$out"
}
fetch OpenRobotics "Construction Cone"    /meshes/construction_cone.dae
fetch OpenRobotics "Gazebo"               /meshes/gazebo.dae
fetch OpenRobotics "X2 UGV"               /meshes/x2_chassis.dae
fetch OpenRobotics "X2 UGV"               /meshes/x2_wheel_left.dae
fetch OpenRobotics "X2 UGV"               /meshes/led.dae
fetch OpenRobotics "Rescue Randy"         /meshes/rescue_randy.dae
fetch OpenRobotics "Pine Tree"            /meshes/pine_tree.dae
fetch OpenRobotics "Backpack"             /meshes/Backpack.dae
fetch OpenRobotics "Urban Station"        /meshes/station.dae
fetch OpenRobotics "Cave Straight Type A" /meshes/straight.dae
fetch OpenRobotics "Fire Hose"            /meshes/connector.dae
fetch OpenRobotics "Cordless Drill"       /meshes/cordless_drill.dae
fetch OpenRobotics "Coke Can"             /meshes/coke_can.dae
fetch OpenRobotics "Pickup"               /meshes/pickup.dae
