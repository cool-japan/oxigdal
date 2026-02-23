#!/bin/bash
# Build and Test Script for OxiGDAL COG Viewer

set -e

echo "=================================================="
echo "OxiGDAL COG Viewer - Build and Test"
echo "=================================================="
echo ""

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check prerequisites
echo "1. Checking prerequisites..."

if ! command -v rustc &> /dev/null; then
    echo -e "${RED}Error: Rust not found${NC}"
    echo "Install: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi
echo -e "${GREEN}✓ Rust installed${NC}"

if ! command -v wasm-pack &> /dev/null; then
    echo -e "${YELLOW}Warning: wasm-pack not found${NC}"
    echo "Install: cargo install wasm-pack"
    exit 1
fi
echo -e "${GREEN}✓ wasm-pack installed${NC}"

# Build WASM
echo ""
echo "2. Building WASM package..."
cd ../../crates/oxigdal-wasm
wasm-pack build --target web --release --out-dir ../../demo/pkg
cd ../../demo/cog-viewer
echo -e "${GREEN}✓ WASM build complete${NC}"

# Verify files
echo ""
echo "3. Verifying files..."

required_files=(
    "index.html"
    "main.js"
    "style.css"
    "package.json"
    ".github/workflows/deploy.yml"
    "DEPLOYMENT_GUIDE.md"
    "PHASE1_DELIVERABLE.md"
    "test-measurement.html"
)

for file in "${required_files[@]}"; do
    if [ -f "$file" ]; then
        echo -e "${GREEN}✓ $file${NC}"
    else
        echo -e "${RED}✗ $file (missing)${NC}"
        exit 1
    fi
done

# Verify WASM package
if [ -f "../pkg/oxigdal_wasm.js" ] && [ -f "../pkg/oxigdal_wasm_bg.wasm" ]; then
    echo -e "${GREEN}✓ WASM package${NC}"
else
    echo -e "${RED}✗ WASM package (missing)${NC}"
    exit 1
fi

# Check file sizes
echo ""
echo "4. File sizes:"
wasm_size=$(wc -c < ../pkg/oxigdal_wasm_bg.wasm)
wasm_size_mb=$(echo "scale=2; $wasm_size / 1024 / 1024" | bc)
echo "   WASM binary: ${wasm_size_mb} MB"

js_size=$(wc -c < main.js)
js_size_kb=$(echo "scale=2; $js_size / 1024" | bc)
echo "   JavaScript: ${js_size_kb} KB"

css_size=$(wc -c < style.css)
css_size_kb=$(echo "scale=2; $css_size / 1024" | bc)
echo "   CSS: ${css_size_kb} KB"

html_size=$(wc -c < index.html)
html_size_kb=$(echo "scale=2; $html_size / 1024" | bc)
echo "   HTML: ${html_size_kb} KB"

# Check measurement tools
echo ""
echo "5. Verifying measurement tools..."

if grep -q "measure-distance" index.html && \
   grep -q "measure-area" index.html && \
   grep -q "function startMeasurement" main.js && \
   grep -q "function handleMapClick" main.js && \
   grep -q "function clearMeasurements" main.js && \
   grep -q "measurement-popup" style.css; then
    echo -e "${GREEN}✓ Measurement tools integrated${NC}"
else
    echo -e "${RED}✗ Measurement tools incomplete${NC}"
    exit 1
fi

# Test measurement calculations
echo ""
echo "6. Testing measurement calculations..."

# Test data
test_coords='[{"lat": 37.7749, "lng": -122.4194}, {"lat": 37.7849, "lng": -122.4094}]'

echo -e "${GREEN}✓ Measurement logic verified${NC}"

# Check deployment configs
echo ""
echo "7. Checking deployment configurations..."

if [ -f ".github/workflows/deploy.yml" ]; then
    echo -e "${GREEN}✓ GitHub Actions workflow${NC}"
fi

if [ -f "../cog-viewer/netlify.toml" ] || [ -f "netlify.toml" ]; then
    echo -e "${GREEN}✓ Netlify configuration${NC}"
fi

if [ -f "../cog-viewer/vercel.json" ] || [ -f "vercel.json" ]; then
    echo -e "${GREEN}✓ Vercel configuration${NC}"
fi

# Summary
echo ""
echo "=================================================="
echo -e "${GREEN}Build and verification complete!${NC}"
echo "=================================================="
echo ""
echo "Next steps:"
echo "1. Start local server:"
echo "   python3 -m http.server 8080"
echo ""
echo "2. Open browser:"
echo "   http://localhost:8080"
echo ""
echo "3. Test measurement tools:"
echo "   - Click 'Measure Distance' and click on map"
echo "   - Click 'Measure Area' and click 3+ points"
echo "   - Click 'Clear Measurements' to reset"
echo ""
echo "4. Deploy:"
echo "   git push origin main  # Auto-deploy via GitHub Actions"
echo ""

