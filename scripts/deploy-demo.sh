#!/bin/bash
# OxiGDAL Demo Deployment Script
# Automates deployment to various platforms
# Author: COOLJAPAN OU (Team Kitasan)
# License: Apache 2.0 / MIT

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
DEMO_DIR="$PROJECT_ROOT/demo"
WASM_PKG_SRC="$PROJECT_ROOT/crates/oxigdal-wasm/pkg"
WASM_PKG_DEST="$DEMO_DIR/pkg"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Print colored message
print_info() {
    echo -e "${BLUE}ℹ${NC} $1"
}

print_success() {
    echo -e "${GREEN}✓${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}⚠${NC} $1"
}

print_error() {
    echo -e "${RED}✗${NC} $1"
}

# Print header
echo "================================================"
echo "  OxiGDAL Demo Deployment"
echo "  Phase 1: Browser Breakthrough"
echo "================================================"
echo ""

# Check if we're in the right directory
if [ ! -f "$PROJECT_ROOT/Cargo.toml" ]; then
    print_error "Not in OxiGDAL project root"
    exit 1
fi

# Step 1: Build WASM package
print_info "Step 1: Building WASM package..."
cd "$PROJECT_ROOT/crates/oxigdal-wasm"

if ! wasm-pack build --target web --release; then
    print_error "WASM build failed"
    exit 1
fi

print_success "WASM package built successfully"
WASM_SIZE=$(du -h "$WASM_PKG_SRC/oxigdal_wasm_bg.wasm" | cut -f1)
print_info "Package size: $WASM_SIZE"

# Step 2: Copy to demo directory
print_info "Step 2: Copying WASM package to demo..."
mkdir -p "$WASM_PKG_DEST"
cp -r "$WASM_PKG_SRC"/* "$WASM_PKG_DEST/"
print_success "WASM package copied to demo/pkg/"

# Step 3: Verify demo files
print_info "Step 3: Verifying demo files..."
REQUIRED_FILES=(
    "$DEMO_DIR/index.html"
    "$DEMO_DIR/cog-viewer/index.html"
    "$DEMO_DIR/cog-viewer/main.js"
    "$DEMO_DIR/cog-viewer/style.css"
    "$WASM_PKG_DEST/oxigdal_wasm_bg.wasm"
    "$WASM_PKG_DEST/oxigdal_wasm.js"
)

ALL_PRESENT=true
for file in "${REQUIRED_FILES[@]}"; do
    if [ ! -f "$file" ]; then
        print_error "Missing file: $file"
        ALL_PRESENT=false
    fi
done

if [ "$ALL_PRESENT" = true ]; then
    print_success "All required files present"
else
    exit 1
fi

# Step 4: Deployment target selection
echo ""
echo "Select deployment target:"
echo "  1) GitHub Pages (recommended)"
echo "  2) Netlify"
echo "  3) Vercel"
echo "  4) Local test server"
echo "  5) Skip deployment (build only)"
echo ""
read -p "Enter choice [1-5]: " DEPLOY_CHOICE

case $DEPLOY_CHOICE in
    1)
        print_info "Deploying to GitHub Pages..."
        cd "$PROJECT_ROOT"

        # Check if gh CLI is available
        if ! command -v gh &> /dev/null; then
            print_warning "GitHub CLI (gh) not installed"
            print_info "Install: https://cli.github.com/"
            print_info "Or manually push to main branch to trigger workflow"
            exit 0
        fi

        # Check if workflow exists
        if [ ! -f "$PROJECT_ROOT/.github/workflows/deploy-pages.yml" ]; then
            print_warning "GitHub Pages workflow not found"
            print_info "Create .github/workflows/deploy-pages.yml first"
            exit 1
        fi

        print_info "Workflow will be triggered on push to main"
        print_success "To deploy: git push origin main"
        ;;

    2)
        print_info "Deploying to Netlify..."

        if ! command -v netlify &> /dev/null; then
            print_error "Netlify CLI not installed"
            print_info "Install: npm install -g netlify-cli"
            exit 1
        fi

        cd "$DEMO_DIR"
        netlify deploy --prod --dir .
        print_success "Deployed to Netlify"
        ;;

    3)
        print_info "Deploying to Vercel..."

        if ! command -v vercel &> /dev/null; then
            print_error "Vercel CLI not installed"
            print_info "Install: npm install -g vercel"
            exit 1
        fi

        cd "$DEMO_DIR"
        vercel --prod
        print_success "Deployed to Vercel"
        ;;

    4)
        print_info "Starting local test server..."
        cd "$DEMO_DIR"
        print_success "Server starting at http://localhost:8000"
        print_info "Press Ctrl+C to stop"
        echo ""
        python3 -m http.server 8000
        ;;

    5)
        print_success "Build complete, skipping deployment"
        ;;

    *)
        print_error "Invalid choice"
        exit 1
        ;;
esac

echo ""
echo "================================================"
print_success "Deployment process complete!"
echo "================================================"
echo ""
print_info "Next steps:"
echo "  - Test the deployed demo"
echo "  - Verify WASM initialization"
echo "  - Check browser console for errors"
echo "  - Test on mobile devices"
echo ""
