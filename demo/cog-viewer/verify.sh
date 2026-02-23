#!/bin/bash
# OxiGDAL COG Viewer - Verification Script
#
# This script verifies that all required files are in place

set -e

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}OxiGDAL COG Viewer - Verification${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# Track overall status
all_good=true

# Check function
check_file() {
    if [ -f "$1" ]; then
        echo -e "${GREEN}✓${NC} $1"
    else
        echo -e "${RED}✗${NC} $1 ${RED}(MISSING)${NC}"
        all_good=false
    fi
}

check_dir() {
    if [ -d "$1" ]; then
        echo -e "${GREEN}✓${NC} $1/"
    else
        echo -e "${RED}✗${NC} $1/ ${RED}(MISSING)${NC}"
        all_good=false
    fi
}

echo "Checking demo files..."
check_file "index.html"
check_file "main.js"
check_file "style.css"
check_file "package.json"
check_file "README.md"
check_file "run.sh"
check_file ".gitignore"

echo ""
echo "Checking WASM package..."
check_dir "../pkg"
check_file "../pkg/oxigdal_wasm.js"
check_file "../pkg/oxigdal_wasm_bg.wasm"

echo ""
echo "Checking browser compatibility..."

# Check if files use ES6 modules
if grep -q "type=\"module\"" index.html; then
    echo -e "${GREEN}✓${NC} ES6 modules configured"
else
    echo -e "${YELLOW}⚠${NC} ES6 modules not detected"
fi

# Check Leaflet integration
if grep -q "leaflet" index.html; then
    echo -e "${GREEN}✓${NC} Leaflet library included"
else
    echo -e "${RED}✗${NC} Leaflet library not found"
    all_good=false
fi

# Check WASM import
if grep -q "import.*oxigdal_wasm" main.js; then
    echo -e "${GREEN}✓${NC} WASM module import found"
else
    echo -e "${RED}✗${NC} WASM module import not found"
    all_good=false
fi

echo ""
echo "Checking for common issues..."

# Check for absolute paths (should use relative)
if grep -q "file://" main.js index.html 2>/dev/null; then
    echo -e "${YELLOW}⚠${NC} Absolute file paths detected (use relative paths)"
fi

# Check for localhost URLs
if grep -q "localhost" main.js index.html 2>/dev/null; then
    echo -e "${YELLOW}⚠${NC} Localhost URLs detected (may not work in production)"
fi

echo ""
echo "File size summary..."
if [ -f "../pkg/oxigdal_wasm_bg.wasm" ]; then
    wasm_size=$(stat -f%z "../pkg/oxigdal_wasm_bg.wasm" 2>/dev/null || stat -c%s "../pkg/oxigdal_wasm_bg.wasm" 2>/dev/null)
    wasm_kb=$((wasm_size / 1024))
    echo "  WASM binary: ${wasm_kb} KB"
fi

if [ -f "../pkg/oxigdal_wasm.js" ]; then
    js_size=$(stat -f%z "../pkg/oxigdal_wasm.js" 2>/dev/null || stat -c%s "../pkg/oxigdal_wasm.js" 2>/dev/null)
    js_kb=$((js_size / 1024))
    echo "  JS bindings: ${js_kb} KB"
fi

echo ""
echo "=========================================="
if [ "$all_good" = true ]; then
    echo -e "${GREEN}✓ All checks passed!${NC}"
    echo ""
    echo "Ready to run:"
    echo "  ./run.sh"
    echo ""
    echo "Or manually:"
    echo "  python3 -m http.server 8080"
    exit 0
else
    echo -e "${RED}✗ Some checks failed${NC}"
    echo ""
    echo "To build WASM package:"
    echo "  cd ../../crates/oxigdal-wasm"
    echo "  wasm-pack build --target web --out-dir ../../demo/pkg"
    exit 1
fi
