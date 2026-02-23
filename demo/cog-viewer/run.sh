#!/bin/bash
# OxiGDAL COG Viewer - Quick Start Script
#
# This script builds the WASM package (if needed) and starts a local server

set -e

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}OxiGDAL Advanced COG Viewer${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# Check if WASM package exists
if [ ! -f "../pkg/oxigdal_wasm.js" ]; then
    echo -e "${YELLOW}WASM package not found. Building...${NC}"
    echo ""

    cd ../../crates/oxigdal-wasm
    wasm-pack build --target web --out-dir ../../demo/pkg

    if [ $? -eq 0 ]; then
        echo -e "${GREEN}✓ WASM package built successfully${NC}"
    else
        echo -e "${RED}✗ Failed to build WASM package${NC}"
        exit 1
    fi

    cd ../../demo/cog-viewer
else
    echo -e "${GREEN}✓ WASM package found${NC}"
fi

echo ""
echo -e "${BLUE}Starting local server...${NC}"
echo -e "${GREEN}Open your browser to: http://localhost:8080${NC}"
echo -e "${YELLOW}Press Ctrl+C to stop the server${NC}"
echo ""

# Try Python first, then Node.js http-server
if command -v python3 &> /dev/null; then
    python3 -m http.server 8080
elif command -v python &> /dev/null; then
    python -m http.server 8080
elif command -v http-server &> /dev/null; then
    http-server -p 8080 -c-1
else
    echo -e "${RED}No suitable HTTP server found.${NC}"
    echo -e "${YELLOW}Please install one of:${NC}"
    echo "  - Python 3: https://www.python.org/"
    echo "  - Node.js http-server: npm install -g http-server"
    exit 1
fi
