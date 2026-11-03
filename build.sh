#!/bin/bash
set -e

# The wasm-bindgen CLI must match the wasm-bindgen crate version in Cargo.lock.
REQUIRED_VERSION=$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"$/\1/p')

install_wasm_bindgen() {
    echo "Installing wasm-bindgen-cli v$REQUIRED_VERSION..."
    cargo install wasm-bindgen-cli --version $REQUIRED_VERSION
}

if ! command -v wasm-bindgen &> /dev/null; then
    install_wasm_bindgen
else
    INSTALLED_VERSION=$(wasm-bindgen --version | awk '{print $2}')
    if [ "$INSTALLED_VERSION" != "$REQUIRED_VERSION" ]; then
        echo "wasm-bindgen version mismatch. Installed: $INSTALLED_VERSION, Required: $REQUIRED_VERSION"
        install_wasm_bindgen
    fi
fi

echo "Building WASM binary..."
cargo build -p gzweb --target wasm32-unknown-unknown --release

# The function names are 40 MB of the 87 (3.5 MB gzipped, D-120); without
# them a browser's stack trace shows wasm-function[N]. KEEP_WASM_NAMES=1
# keeps them, for debugging.
NAMES=(--remove-name-section)
[ "${KEEP_WASM_NAMES:-0}" = 1 ] && NAMES=()

echo "Generating JS bindings..."
wasm-bindgen --out-dir ./target/wasm32-unknown-unknown/release/ --target web "${NAMES[@]}" ./target/wasm32-unknown-unknown/release/gzweb.wasm

# The payload budget (D-120); a build that keeps the names is over it.
if [ "${KEEP_WASM_NAMES:-0}" = 1 ]; then
    scripts/wasm-budget.sh || true
else
    scripts/wasm-budget.sh
fi

# A separate maintained GL shader configuration; never mix PBR WebGPU/GL layouts.
mkdir -p ./target/wasm32-unknown-unknown/release/webgpu
cp ./target/wasm32-unknown-unknown/release/gzweb{.js,_bg.wasm} ./target/wasm32-unknown-unknown/release/webgpu/
echo "Building maintained WebGL2 fallback..."
cargo build -p gzweb --target wasm32-unknown-unknown --release --no-default-features
mkdir -p ./target/wasm32-unknown-unknown/release/webgl2
wasm-bindgen --out-dir ./target/wasm32-unknown-unknown/release/webgl2 --target web "${NAMES[@]}" ./target/wasm32-unknown-unknown/release/gzweb.wasm
if [ "${KEEP_WASM_NAMES:-0}" = 1 ]; then
    scripts/wasm-budget.sh ./target/wasm32-unknown-unknown/release/webgl2/gzweb_bg.wasm || true
else
    scripts/wasm-budget.sh ./target/wasm32-unknown-unknown/release/webgl2/gzweb_bg.wasm
fi
echo "Both renderer bundles complete."
