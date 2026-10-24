# Builds gz-sim's websocket server system on its own, against the installed
# Jetty libraries, from a gz-sim source tree (GZ_SIM_SOURCE) with
# binary-requests.patch and client-publish.patch applied. Dockerfile.gazebo
# uses it; see there.
cmake_minimum_required(VERSION 3.22)
project(gz-sim-websocket-server-system CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

set(GZ_SIM_SOURCE "" CACHE PATH "A gz-sim source tree")
set(SYSTEM_DIR ${GZ_SIM_SOURCE}/src/systems/websocket_server)

find_package(gz-sim REQUIRED)
find_package(gz-common REQUIRED COMPONENTS graphics)
find_package(gz-fuel_tools REQUIRED)
find_package(gz-msgs REQUIRED)
find_package(gz-plugin REQUIRED COMPONENTS register)
find_package(gz-transport REQUIRED)
find_package(libwebsockets REQUIRED)

add_library(gz-sim-websocket-server-system SHARED ${SYSTEM_DIR}/WebsocketServer.cc)
target_include_directories(gz-sim-websocket-server-system PRIVATE ${SYSTEM_DIR})
target_link_libraries(gz-sim-websocket-server-system
  PRIVATE
    gz-sim::gz-sim
    websockets_shared
    gz-common::gz-common
    gz-common::graphics
    gz-fuel_tools::gz-fuel_tools
    gz-msgs::core
    gz-plugin::register
    gz-transport::core
)
