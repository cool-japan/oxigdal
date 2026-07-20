#!/bin/bash
# GeoSentinel - Quick Start Script
#
# Serves the demo over HTTP (ES modules and WASM need a real origin).

set -e

GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

cd "$(dirname "$0")"

PORT="${PORT:-8080}"

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}GeoSentinel — Sentinel-2 change detection${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# Check if the WASM package exists (pkg is a symlink into crates/oxigdal-wasm)
if [ ! -f "pkg/oxigdal_wasm.js" ]; then
    echo -e "${YELLOW}WASM package not found. Building...${NC}"
    echo ""

    (cd ../.. && wasm-pack build crates/oxigdal-wasm --target web --out-dir pkg)

    if [ -f "pkg/oxigdal_wasm.js" ]; then
        echo -e "${GREEN}[ok] WASM package built successfully${NC}"
    else
        echo -e "${RED}[fail] Failed to build WASM package${NC}"
        exit 1
    fi
else
    echo -e "${GREEN}[ok] WASM package found${NC}"
fi

echo ""
echo -e "${BLUE}Starting local server...${NC}"
echo -e "${GREEN}Open your browser to: http://localhost:${PORT}${NC}"
echo -e "${YELLOW}Press Ctrl+C to stop the server${NC}"
echo ""

if command -v python3 &> /dev/null; then
    python3 -m http.server "$PORT"
elif command -v python &> /dev/null; then
    python -m http.server "$PORT"
elif command -v http-server &> /dev/null; then
    http-server -p "$PORT" -c-1
else
    echo -e "${RED}No suitable HTTP server found.${NC}"
    echo -e "${YELLOW}Please install one of:${NC}"
    echo "  - Python 3: https://www.python.org/"
    echo "  - Node.js http-server: npm install -g http-server"
    exit 1
fi
