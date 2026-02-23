#!/usr/bin/env bash
# Disable exit-on-error for this script since we want to collect ALL test failures
set -uo pipefail

# Detect failing tests across OxiGDAL workspace
# Usage: ./scripts/detect-fail-tests.sh [gpu-ml|gpu|all|<package-name>]
#
# This script runs tests with --no-fail-fast to collect ALL failures (not just the first),
# saves the results in NDJSON format, and generates a summary report.
#
# Key differences from detect-slow-tests.sh:
# - Uses --no-fail-fast to continue after failures
# - Always exits 0 (we're collecting data, not failing the build)
# - Captures stderr in addition to stdout
# - Focuses on failed/flaky tests instead of slow tests

# Default to gpu-ml if no argument provided
TARGET="${1:-gpu-ml}"

# Output directory for JSON results
OUTPUT_DIR="target/fail-test-detection"
mkdir -p "$OUTPUT_DIR"

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
MAGENTA='\033[0;35m'
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

log_debug() {
    echo -e "${MAGENTA}[DEBUG]${NC} $*"
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

# Packages known to have external dependencies (likely to fail in CI/isolated environments)
EXTERNAL_DEP_PACKAGES=(
    "oxigdal-kafka"
    "oxigdal-redis"
    "oxigdal-s3"
    "oxigdal-azure"
    "oxigdal-gcp"
)

# Function to run fail test detection for a package
# Args:
#   $1: package name
# Returns:
#   Always 0 (we collect failures, not propagate them)
detect_fail_tests() {
    local package="$1"
    local timestamp=$(date +%Y%m%d_%H%M%S)
    local output_file="$OUTPUT_DIR/${package}_${timestamp}.ndjson"
    local stderr_file="$OUTPUT_DIR/${package}_${timestamp}.stderr"

    log_info "Detecting failing tests in package: $package"

    # Run nextest with:
    # - --no-fail-fast: Continue running all tests even after failures
    # - --message-format libtest-json-plus: Output NDJSON for parsing
    # - NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1: Enable JSON output (experimental feature)
    # - 2> stderr capture: Save stderr separately for debugging
    #
    # We deliberately ignore the exit code (|| true) because we want to collect
    # ALL failures, and nextest exits non-zero when tests fail.
    if NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run \
        --package "$package" \
        --no-fail-fast \
        --message-format libtest-json-plus \
        > "$output_file" 2> "$stderr_file" || true; then
        log_debug "Nextest completed for $package (exit code ignored)"
    else
        log_debug "Nextest returned non-zero for $package (expected if tests failed)"
    fi

    log_success "Completed failure detection for $package"
    log_info "NDJSON output saved to: $output_file"
    log_info "Stderr saved to: $stderr_file"

    # Extract failure summary if jq is available
    if command -v jq &> /dev/null; then
        # Count failed tests from NDJSON
        # nextest JSON format: {"type": "test", "event": "failed", ...}
        local fail_count=0
        local pass_count=0
        local skip_count=0

        # Read the NDJSON line by line
        while IFS= read -r line; do
            if [ -z "$line" ]; then
                continue
            fi

            # Try to parse as JSON
            if ! event_type=$(echo "$line" | jq -r '.type // empty' 2>/dev/null); then
                continue
            fi

            if [ "$event_type" = "test" ]; then
                event=$(echo "$line" | jq -r '.event // empty' 2>/dev/null)
                case "$event" in
                    failed)
                        ((fail_count++)) || true
                        ;;
                    ok)
                        ((pass_count++)) || true
                        ;;
                    skipped|ignored)
                        ((skip_count++)) || true
                        ;;
                esac
            fi
        done < "$output_file"

        # Print summary
        if [ "$fail_count" -gt 0 ]; then
            log_error "Found $fail_count FAILED tests in $package"
            log_info "  Passed: $pass_count, Skipped: $skip_count"

            # Show first 5 failed test names
            log_warning "First 5 failed tests:"
            jq -r 'select(.type == "test") | select(.event == "failed") | "  - \(.name)"' "$output_file" 2>/dev/null | head -5
        elif [ "$pass_count" -gt 0 ]; then
            log_success "All $pass_count tests PASSED in $package (Skipped: $skip_count)"
        else
            log_warning "No test events found in $package output"
        fi

        # Check stderr for interesting errors
        if [ -s "$stderr_file" ]; then
            local stderr_lines=$(wc -l < "$stderr_file" | tr -d ' ')
            if [ "$stderr_lines" -gt 0 ]; then
                log_warning "Stderr contains $stderr_lines lines (see $stderr_file)"
                # Show first few lines of stderr if it looks like an error
                if grep -qi "error\|panic\|fatal\|cuda\|gpu\|kafka" "$stderr_file" 2>/dev/null; then
                    log_debug "Stderr preview (first 5 lines):"
                    head -5 "$stderr_file" | sed 's/^/    /'
                fi
            fi
        fi
    else
        log_warning "Install 'jq' for enhanced summary reports"
    fi

    # Always return 0 - we're collecting data, not failing on test failures
    return 0
}

# Function to validate package exists in workspace
validate_package() {
    local package="$1"

    if ! cargo metadata --no-deps --format-version 1 2>/dev/null | \
         jq -e ".packages[] | select(.name == \"$package\")" > /dev/null; then
        log_error "Package '$package' not found in workspace"
        return 1
    fi
    return 0
}

# Main execution
main() {
    log_info "=== OxiGDAL Fail Test Detection ==="
    log_info "Target: $TARGET"
    log_info "Output directory: $OUTPUT_DIR"
    echo ""

    case "$TARGET" in
        gpu-ml)
            log_info "Running fail test detection for GPU+ML packages"
            for pkg in "${GPU_ML_PACKAGES[@]}"; do
                detect_fail_tests "$pkg"
                echo ""
            done
            ;;
        gpu)
            log_info "Running fail test detection for GPU packages only"
            for pkg in "${GPU_PACKAGES[@]}"; do
                detect_fail_tests "$pkg"
                echo ""
            done
            ;;
        external)
            log_info "Running fail test detection for packages with external dependencies"
            for pkg in "${EXTERNAL_DEP_PACKAGES[@]}"; do
                if validate_package "$pkg"; then
                    detect_fail_tests "$pkg"
                else
                    log_warning "Skipping $pkg (not found in workspace)"
                fi
                echo ""
            done
            ;;
        all)
            log_info "Running fail test detection for all workspace packages"
            # Get all package names from workspace
            mapfile -t ALL_PACKAGES < <(cargo metadata --no-deps --format-version 1 | jq -r '.packages[].name')
            log_info "Found ${#ALL_PACKAGES[@]} packages in workspace"
            echo ""

            for pkg in "${ALL_PACKAGES[@]}"; do
                detect_fail_tests "$pkg"
                echo ""
            done
            ;;
        *)
            # Treat as specific package name
            log_info "Running fail test detection for specific package: $TARGET"

            if validate_package "$TARGET"; then
                detect_fail_tests "$TARGET"
            else
                log_error "Package '$TARGET' does not exist in workspace"
                log_info "Use one of: gpu-ml, gpu, external, all, or a valid package name"
                exit 1
            fi
            ;;
    esac

    echo ""
    log_success "Fail test detection completed"
    log_info "Results directory: $OUTPUT_DIR"

    # Generate summary report if jq is available
    if command -v jq &> /dev/null; then
        echo ""
        log_info "=== Summary Report ==="

        local total_files=0
        local total_failed=0
        local total_passed=0
        local packages_with_failures=()

        for ndjson_file in "$OUTPUT_DIR"/*.ndjson; do
            if [ ! -f "$ndjson_file" ]; then
                continue
            fi

            ((total_files++)) || true

            # Extract package name from filename
            local pkg_name=$(basename "$ndjson_file" | sed 's/_[0-9]*\.ndjson$//')

            # Count failures and passes in this file
            local file_failures=0
            local file_passes=0

            while IFS= read -r line; do
                if [ -z "$line" ]; then
                    continue
                fi

                event=$(echo "$line" | jq -r 'select(.type == "test") | .event // empty' 2>/dev/null)
                case "$event" in
                    failed)
                        ((file_failures++)) || true
                        ((total_failed++)) || true
                        ;;
                    ok)
                        ((file_passes++)) || true
                        ((total_passed++)) || true
                        ;;
                esac
            done < "$ndjson_file"

            if [ "$file_failures" -gt 0 ]; then
                packages_with_failures+=("$pkg_name")
                echo -e "${RED}✗ $pkg_name${NC}: $file_failures failed, $file_passes passed"

                # Show failed test names
                jq -r 'select(.type == "test") | select(.event == "failed") | "    - \(.name)"' "$ndjson_file" 2>/dev/null | head -3

                local remaining=$(jq -r 'select(.type == "test") | select(.event == "failed")' "$ndjson_file" 2>/dev/null | wc -l | tr -d ' ')
                if [ "$remaining" -gt 3 ]; then
                    echo "    ... and $((remaining - 3)) more"
                fi
            elif [ "$file_passes" -gt 0 ]; then
                echo -e "${GREEN}✓ $pkg_name${NC}: All $file_passes tests passed"
            fi
        done

        echo ""
        echo -e "${BLUE}=== Overall Statistics ===${NC}"
        echo "Files analyzed: $total_files"
        echo "Total tests failed: $total_failed"
        echo "Total tests passed: $total_passed"

        if [ ${#packages_with_failures[@]} -gt 0 ]; then
            echo -e "${RED}Packages with failures (${#packages_with_failures[@]}):${NC}"
            printf '%s\n' "${packages_with_failures[@]}" | sed 's/^/  - /'
        else
            echo -e "${GREEN}No packages with failures!${NC}"
        fi
    else
        log_warning "Install 'jq' for enhanced summary reports:"
        log_info "  brew install jq  # macOS"
        log_info "  apt-get install jq  # Debian/Ubuntu"
    fi

    echo ""
    log_info "For detailed analysis, use: scripts/lib/nextest_parser.py"
}

# Run main function
main

# Always exit 0 - we're collecting test failure data, not failing the script
exit 0
