#!/usr/bin/env bash
set -euo pipefail

# Detect slow tests across OxiGDAL workspace
# Usage: ./scripts/detect-slow-tests.sh [gpu-ml|gpu|all|<package-name>]

# Default to gpu-ml if no argument provided
TARGET="${1:-gpu-ml}"

# Output directory for JSON results
OUTPUT_DIR="target/slow-test-detection"
mkdir -p "$OUTPUT_DIR"

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${BLUE}[INFO]${NC} $*"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $*"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*"
}

# Package groups
GPU_ML_PACKAGES=(
    "oxigdal-gpu"
    "oxigdal-gpu-advanced"
    "oxigdal-ml"
    "oxigdal-ml-foundation"
    "oxigdal-analytics"
    "oxigdal-distributed"
)

GPU_PACKAGES=(
    "oxigdal-gpu"
    "oxigdal-gpu-advanced"
)

# Function to run slow test detection for a package
detect_slow_tests() {
    local package="$1"
    local timestamp=$(date +%Y%m%d_%H%M%S)
    local output_file="$OUTPUT_DIR/${package}_${timestamp}.json"

    log_info "Detecting slow tests in package: $package"

    if NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run \
        --package "$package" \
        --profile detect-slow \
        --message-format libtest-json-plus \
        > "$output_file" 2>&1; then
        log_success "Completed detection for $package"
        log_info "Results saved to: $output_file"

        # Extract slow tests summary
        if command -v jq &> /dev/null; then
            local slow_count=$(jq -r 'select(.type == "test-finished") | select(.event.duration_ms > 1000) | .event.name' "$output_file" 2>/dev/null | wc -l | tr -d ' ')
            if [ "$slow_count" -gt 0 ]; then
                log_warning "Found $slow_count slow tests (>1s) in $package"
            else
                log_success "No slow tests detected in $package"
            fi
        fi
    else
        log_error "Failed to run tests for $package"
        return 1
    fi
}

# Main execution
case "$TARGET" in
    gpu-ml)
        log_info "Running slow test detection for GPU+ML packages"
        for pkg in "${GPU_ML_PACKAGES[@]}"; do
            detect_slow_tests "$pkg" || true
        done
        ;;
    gpu)
        log_info "Running slow test detection for GPU packages only"
        for pkg in "${GPU_PACKAGES[@]}"; do
            detect_slow_tests "$pkg" || true
        done
        ;;
    all)
        log_info "Running slow test detection for all workspace packages"
        # Get all package names from workspace
        mapfile -t ALL_PACKAGES < <(cargo metadata --no-deps --format-version 1 | jq -r '.packages[].name')
        for pkg in "${ALL_PACKAGES[@]}"; do
            detect_slow_tests "$pkg" || true
        done
        ;;
    *)
        # Treat as specific package name
        log_info "Running slow test detection for specific package: $TARGET"
        detect_slow_tests "$TARGET"
        ;;
esac

log_success "Slow test detection completed"
log_info "Results directory: $OUTPUT_DIR"

# Summary report if jq is available
if command -v jq &> /dev/null; then
    echo ""
    log_info "=== Summary Report ==="
    for json_file in "$OUTPUT_DIR"/*.json; do
        if [ -f "$json_file" ]; then
            pkg_name=$(basename "$json_file" | cut -d'_' -f1-2)
            slow_tests=$(jq -r 'select(.type == "test-finished") | select(.event.duration_ms > 1000) | "\(.event.name): \(.event.duration_ms)ms"' "$json_file" 2>/dev/null | head -10)

            if [ -n "$slow_tests" ]; then
                echo -e "${YELLOW}$pkg_name:${NC}"
                echo "$slow_tests"
                echo ""
            fi
        fi
    done
else
    log_warning "Install 'jq' for enhanced summary reports"
fi
