#!/usr/bin/env bash

# OxiGDAL Demo Build Script
# This script builds the WASM package for the demo application

set -e  # Exit on error

echo "=========================================="
echo "OxiGDAL Demo Build Script"
echo "=========================================="
echo ""

# Check if wasm-pack is installed
if ! command -v wasm-pack &> /dev/null; then
    echo "Error: wasm-pack is not installed"
    echo "Install it with: cargo install wasm-pack"
    exit 1
fi

# Navigate to the wasm crate directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_CRATE_DIR="${SCRIPT_DIR}/../crates/oxigdal-wasm"
OUTPUT_DIR="${SCRIPT_DIR}/pkg"

echo "Building WASM package..."
echo "Source: ${WASM_CRATE_DIR}"
echo "Output: ${OUTPUT_DIR}"
echo ""

# Build mode (default: debug, pass --release for release build)
BUILD_MODE="${1:-debug}"

if [ "$BUILD_MODE" = "--release" ] || [ "$BUILD_MODE" = "release" ]; then
    echo "Building in RELEASE mode (optimized)..."
    cd "${WASM_CRATE_DIR}"
    wasm-pack build --target web --release --out-dir "${OUTPUT_DIR}"
else
    echo "Building in DEBUG mode (faster compilation)..."
    cd "${WASM_CRATE_DIR}"
    wasm-pack build --target web --out-dir "${OUTPUT_DIR}"
fi

echo ""
echo "=========================================="
echo "Build complete!"
echo "=========================================="
echo ""
echo "To run the demo locally:"
echo "  cd ${SCRIPT_DIR}"
echo "  python3 -m http.server 8080"
echo ""
echo "Then open: http://localhost:8080"
echo ""
