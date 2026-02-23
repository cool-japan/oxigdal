#!/usr/bin/env bash
set -euo pipefail

# Auto-ignore slow tests by adding #[ignore] attributes
# Usage: ./scripts/auto-ignore-slow-tests.sh [--dry-run|--apply] [slow-tests.json]

# Default values
MODE=""
INPUT_FILE="${2:-target/slow-test-detection/slow-tests.json}"
TOOL_DIR="tools/add-ignore-attr"
TOOL_BIN="target/release/add-ignore-attr"

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

# Parse arguments
if [ $# -lt 1 ]; then
    log_error "Usage: $0 [--dry-run|--apply] [slow-tests.json]"
    log_info "Examples:"
    log_info "  $0 --dry-run"
    log_info "  $0 --apply"
    log_info "  $0 --dry-run target/custom-slow-tests.json"
    log_info "  $0 --apply target/custom-slow-tests.json"
    exit 1
fi

MODE="$1"

# Validate mode
case "$MODE" in
    --dry-run)
        log_info "Running in DRY-RUN mode (no files will be changed)"
        ;;
    --apply)
        log_info "Running in APPLY mode (files will be modified)"
        ;;
    *)
        log_error "Invalid mode: $MODE"
        log_error "Must specify either --dry-run or --apply"
        exit 1
        ;;
esac

# Check if input file exists
if [ ! -f "$INPUT_FILE" ]; then
    log_error "Input file not found: $INPUT_FILE"
    log_info "First, run: ./scripts/detect-slow-tests.sh to generate slow-tests.json"
    exit 1
fi

log_info "Using slow tests from: $INPUT_FILE"

# Check if tool needs to be built
if [ ! -f "$TOOL_BIN" ]; then
    log_info "Building add-ignore-attr tool..."

    if [ ! -d "$TOOL_DIR" ]; then
        log_error "Tool directory not found: $TOOL_DIR"
        exit 1
    fi

    if ! cargo build --release --manifest-path "$TOOL_DIR/Cargo.toml"; then
        log_error "Failed to build add-ignore-attr tool"
        exit 1
    fi

    log_success "Tool built successfully"
else
    log_info "add-ignore-attr tool already built"
fi

# Run the tool
log_info "Running add-ignore-attr tool..."
echo ""

if [ "$MODE" = "--dry-run" ]; then
    if "$TOOL_BIN" --input "$INPUT_FILE" --dry-run; then
        log_success "Dry-run completed successfully"
        log_info "Review the output above and run with --apply to make changes"
    else
        log_error "Tool execution failed"
        exit 1
    fi
else
    if "$TOOL_BIN" --input "$INPUT_FILE" --apply; then
        log_success "Changes applied successfully"
        log_info "Test files have been modified with #[ignore] attributes"
    else
        log_error "Tool execution failed"
        exit 1
    fi
fi

echo ""
log_success "Auto-ignore process completed"
