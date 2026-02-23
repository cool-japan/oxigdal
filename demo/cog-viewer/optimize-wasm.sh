#!/bin/bash
#
# WASM Optimization Script
# Optimizes the WASM bundle for production deployment
#

set -e

echo "================================================"
echo "OxiGDAL WASM Optimization Script"
echo "================================================"
echo ""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Directories
WASM_CRATE_DIR="../../crates/oxigdal-wasm"
PKG_DIR="../pkg"
DEMO_DIR="."

echo "Step 1: Building WASM package (release mode)"
echo "-------------------------------------------"
cd "$WASM_CRATE_DIR"
wasm-pack build --target web --release --out-dir "$PKG_DIR"
cd -

# Check if build succeeded
if [ ! -f "$PKG_DIR/oxigdal_wasm_bg.wasm" ]; then
    echo -e "${RED}Error: WASM build failed${NC}"
    exit 1
fi

echo -e "${GREEN}✓ WASM build complete${NC}"
echo ""

# Get original size
ORIGINAL_SIZE=$(du -h "$PKG_DIR/oxigdal_wasm_bg.wasm" | awk '{print $1}')
echo "Original WASM size: $ORIGINAL_SIZE"
echo ""

# Step 2: Optimize with wasm-opt
echo "Step 2: Optimizing with wasm-opt"
echo "--------------------------------"

if command -v wasm-opt &> /dev/null; then
    echo "Running wasm-opt with -Oz (optimize for size)..."
    wasm-opt -Oz \
        -o "$PKG_DIR/oxigdal_wasm_bg.wasm.opt" \
        "$PKG_DIR/oxigdal_wasm_bg.wasm"

    # Replace original with optimized
    mv "$PKG_DIR/oxigdal_wasm_bg.wasm.opt" "$PKG_DIR/oxigdal_wasm_bg.wasm"

    OPTIMIZED_SIZE=$(du -h "$PKG_DIR/oxigdal_wasm_bg.wasm" | awk '{print $1}')
    echo -e "${GREEN}✓ WASM optimization complete${NC}"
    echo "Optimized WASM size: $OPTIMIZED_SIZE"
else
    echo -e "${YELLOW}⚠ wasm-opt not found, skipping optimization${NC}"
    echo "Install binaryen for size optimization:"
    echo "  brew install binaryen  # macOS"
    echo "  apt install binaryen   # Debian/Ubuntu"
fi

echo ""

# Step 3: Optional gzip compression test
echo "Step 3: Testing gzip compression"
echo "--------------------------------"

if command -v gzip &> /dev/null; then
    # Create temporary gzipped version
    gzip -k -9 "$PKG_DIR/oxigdal_wasm_bg.wasm"
    GZIPPED_SIZE=$(du -h "$PKG_DIR/oxigdal_wasm_bg.wasm.gz" | awk '{print $1}')
    echo "Gzipped size: $GZIPPED_SIZE"
    rm "$PKG_DIR/oxigdal_wasm_bg.wasm.gz"
else
    echo -e "${YELLOW}⚠ gzip not found, skipping compression test${NC}"
fi

echo ""

# Step 4: Bundle analysis
echo "Step 4: Bundle analysis"
echo "----------------------"

echo "Package contents:"
echo ""
ls -lh "$PKG_DIR" | grep -E '\.(wasm|js|ts|json)$'

echo ""
echo "Total package size:"
du -sh "$PKG_DIR"

echo ""

# Step 5: Verification
echo "Step 5: Verification"
echo "-------------------"

# Check if essential files exist
REQUIRED_FILES=(
    "oxigdal_wasm.js"
    "oxigdal_wasm_bg.wasm"
    "oxigdal_wasm.d.ts"
    "package.json"
)

ALL_PRESENT=true
for file in "${REQUIRED_FILES[@]}"; do
    if [ -f "$PKG_DIR/$file" ]; then
        echo -e "${GREEN}✓${NC} $file"
    else
        echo -e "${RED}✗${NC} $file (missing)"
        ALL_PRESENT=false
    fi
done

echo ""

if [ "$ALL_PRESENT" = true ]; then
    echo -e "${GREEN}================================================"
    echo "✓ WASM optimization complete!"
    echo -e "================================================${NC}"
    echo ""
    echo "Summary:"
    echo "  Original size: $ORIGINAL_SIZE"
    if command -v wasm-opt &> /dev/null; then
        echo "  Optimized size: $OPTIMIZED_SIZE"
    fi
    if command -v gzip &> /dev/null; then
        echo "  Gzipped size: $GZIPPED_SIZE"
    fi
    echo ""
    echo "Next steps:"
    echo "  1. Test the demo locally: ./run.sh"
    echo "  2. Deploy to production"
    exit 0
else
    echo -e "${RED}================================================"
    echo "✗ Optimization failed - missing files"
    echo -e "================================================${NC}"
    exit 1
fi
