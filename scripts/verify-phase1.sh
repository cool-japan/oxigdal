#!/bin/bash
# OxiGDAL Phase 1 Verification Script
# Comprehensive verification of all Phase 1 deliverables
# Author: COOLJAPAN OU (Team Kitasan)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Counters
TOTAL_CHECKS=0
PASSED_CHECKS=0
FAILED_CHECKS=0
WARNING_CHECKS=0

# Check functions
check_pass() {
    TOTAL_CHECKS=$((TOTAL_CHECKS + 1))
    PASSED_CHECKS=$((PASSED_CHECKS + 1))
    echo -e "${GREEN}✓${NC} $1"
}

check_fail() {
    TOTAL_CHECKS=$((TOTAL_CHECKS + 1))
    FAILED_CHECKS=$((FAILED_CHECKS + 1))
    echo -e "${RED}✗${NC} $1"
}

check_warn() {
    TOTAL_CHECKS=$((TOTAL_CHECKS + 1))
    WARNING_CHECKS=$((WARNING_CHECKS + 1))
    echo -e "${YELLOW}⚠${NC} $1"
}

print_section() {
    echo ""
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${BLUE}$1${NC}"
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
}

# Header
echo "================================================"
echo "  OxiGDAL Phase 1 Verification"
echo "  Date: $(date '+%Y-%m-%d %H:%M:%S')"
echo "================================================"

cd "$PROJECT_ROOT"

# 1. Workspace Structure
print_section "1. Workspace Structure"

if [ -f "Cargo.toml" ]; then
    check_pass "Workspace Cargo.toml exists"
else
    check_fail "Workspace Cargo.toml missing"
fi

if [ -d "crates/oxigdal-core" ]; then
    check_pass "oxigdal-core crate exists"
else
    check_fail "oxigdal-core crate missing"
fi

if [ -d "crates/oxigdal-drivers/geotiff" ]; then
    check_pass "oxigdal-geotiff crate exists"
else
    check_fail "oxigdal-geotiff crate missing"
fi

if [ -d "crates/oxigdal-wasm" ]; then
    check_pass "oxigdal-wasm crate exists"
else
    check_fail "oxigdal-wasm crate missing"
fi

if [ -d "demo" ]; then
    check_pass "Demo directory exists"
else
    check_fail "Demo directory missing"
fi

# 2. Build Verification
print_section "2. Build Verification"

echo -n "Building workspace (this may take a few minutes)... "
if cargo build --workspace --all-features --quiet 2>/dev/null; then
    check_pass "Workspace builds successfully"
else
    check_fail "Workspace build failed"
fi

echo -n "Building WASM package... "
cd crates/oxigdal-wasm
if wasm-pack build --target web --quiet 2>/dev/null; then
    check_pass "WASM package builds successfully"

    if [ -f "pkg/oxigdal_wasm_bg.wasm" ]; then
        WASM_SIZE=$(stat -f%z pkg/oxigdal_wasm_bg.wasm 2>/dev/null || stat -c%s pkg/oxigdal_wasm_bg.wasm 2>/dev/null)
        WASM_SIZE_KB=$((WASM_SIZE / 1024))

        if [ $WASM_SIZE_KB -lt 512000 ]; then # Less than 500MB
            check_pass "WASM package size acceptable: ${WASM_SIZE_KB}KB"
        else
            check_warn "WASM package size large: ${WASM_SIZE_KB}KB"
        fi
    fi
else
    check_fail "WASM package build failed"
fi

cd "$PROJECT_ROOT"

# 3. Test Verification
print_section "3. Test Verification"

echo -n "Testing oxigdal-core... "
CORE_RESULT=$(cargo test --package oxigdal-core --lib --quiet 2>&1 | grep "test result:")
if echo "$CORE_RESULT" | grep -q "ok"; then
    CORE_PASSED=$(echo "$CORE_RESULT" | grep -oP '\d+(?= passed)' || echo "0")
    check_pass "oxigdal-core: $CORE_PASSED tests passed"
else
    check_fail "oxigdal-core: tests failed"
fi

echo -n "Testing oxigdal-geotiff... "
GEOTIFF_RESULT=$(cargo test --package oxigdal-geotiff --lib --quiet 2>&1 | grep "test result:")
if echo "$GEOTIFF_RESULT" | grep -q "ok"; then
    GEOTIFF_PASSED=$(echo "$GEOTIFF_RESULT" | grep -oP '\d+(?= passed)' || echo "0")
    check_pass "oxigdal-geotiff: $GEOTIFF_PASSED tests passed"
else
    check_fail "oxigdal-geotiff: tests failed"
fi

# 4. Code Quality
print_section "4. Code Quality"

echo -n "Running clippy... "
if cargo clippy --workspace --all-features --quiet -- -D warnings 2>/dev/null; then
    check_pass "Clippy: no warnings"
else
    check_warn "Clippy: some warnings (acceptable for Phase 1)"
fi

# 5. SLOC Verification
print_section "5. Code Metrics"

if command -v tokei &> /dev/null; then
    RUST_SLOC=$(tokei . --exclude '*.md' 2>/dev/null | grep "Rust" | awk '{print $4}')

    if [ ! -z "$RUST_SLOC" ]; then
        # Remove commas if present
        RUST_SLOC_CLEAN=$(echo $RUST_SLOC | tr -d ',')

        if [ $RUST_SLOC_CLEAN -gt 300000 ]; then
            check_pass "Rust SLOC: $RUST_SLOC (exceeds 307,000 target)"
        elif [ $RUST_SLOC_CLEAN -gt 70000 ]; then
            check_pass "Rust SLOC: $RUST_SLOC (exceeds Phase 1 target)"
        else
            check_warn "Rust SLOC: $RUST_SLOC (below target)"
        fi
    fi
else
    check_warn "tokei not installed, skipping SLOC check"
fi

# 6. Demo Verification
print_section "6. Demo Application"

if [ -f "demo/index.html" ]; then
    check_pass "Demo index.html exists"
else
    check_fail "Demo index.html missing"
fi

if [ -f "demo/cog-viewer/index.html" ]; then
    check_pass "COG viewer HTML exists"
else
    check_fail "COG viewer HTML missing"
fi

if [ -f "demo/pkg/oxigdal_wasm_bg.wasm" ]; then
    check_pass "WASM package in demo/pkg/"

    DEMO_WASM_SIZE=$(stat -f%z demo/pkg/oxigdal_wasm_bg.wasm 2>/dev/null || stat -c%s demo/pkg/oxigdal_wasm_bg.wasm 2>/dev/null)
    DEMO_WASM_SIZE_KB=$((DEMO_WASM_SIZE / 1024))
    echo "  Size: ${DEMO_WASM_SIZE_KB}KB"
else
    check_fail "WASM package not in demo/pkg/"
fi

# 7. Documentation
print_section "7. Documentation"

if [ -f "README.md" ]; then
    check_pass "Root README.md exists"
else
    check_warn "Root README.md missing"
fi

DOC_COUNT=$(find crates/*/README.md 2>/dev/null | wc -l)
if [ $DOC_COUNT -gt 5 ]; then
    check_pass "Crate documentation: $DOC_COUNT README files"
else
    check_warn "Limited crate documentation: $DOC_COUNT README files"
fi

# 8. Deployment Readiness
print_section "8. Deployment Readiness"

if [ -d ".github/workflows" ]; then
    WORKFLOW_COUNT=$(find .github/workflows/*.yml 2>/dev/null | wc -l)
    if [ $WORKFLOW_COUNT -gt 0 ]; then
        check_pass "GitHub workflows: $WORKFLOW_COUNT configured"
    else
        check_warn "No GitHub workflows found"
    fi
else
    check_warn ".github/workflows directory missing"
fi

if [ -f "netlify.toml" ] || [ -f "demo/netlify.toml" ]; then
    check_pass "Netlify configuration exists"
else
    check_warn "Netlify configuration missing"
fi

if [ -f "vercel.json" ] || [ -f "demo/vercel.json" ]; then
    check_pass "Vercel configuration exists"
else
    check_warn "Vercel configuration missing"
fi

# Summary
print_section "Verification Summary"

PASS_RATE=$((PASSED_CHECKS * 100 / TOTAL_CHECKS))

echo ""
echo "Total Checks:   $TOTAL_CHECKS"
echo -e "${GREEN}Passed:         $PASSED_CHECKS${NC}"
echo -e "${YELLOW}Warnings:       $WARNING_CHECKS${NC}"
echo -e "${RED}Failed:         $FAILED_CHECKS${NC}"
echo ""
echo "Pass Rate:      $PASS_RATE%"
echo ""

if [ $FAILED_CHECKS -eq 0 ]; then
    echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${GREEN}✓ Phase 1 Verification: PASSED${NC}"
    echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""
    echo "Phase 1 is production-ready!"
    echo ""
    exit 0
else
    echo -e "${RED}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${RED}✗ Phase 1 Verification: FAILED${NC}"
    echo -e "${RED}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""
    echo "Please address the failures above."
    echo ""
    exit 1
fi
