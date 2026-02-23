#!/usr/bin/env bash
#
# Auto-Fix Fail Tests Orchestration Script
#
# Orchestrates the complete workflow for detecting, analyzing, and fixing failing tests:
#   1. DETECT - Run nextest to capture all failures (NDJSON)
#   2. ANALYZE - Classify failures using taxonomy (JSON + Markdown)
#   3. FIX - Apply automated fixes based on classification
#   4. VERIFY - Re-run tests to confirm fixes worked
#
# Usage:
#   ./scripts/auto-fix-fail-tests.sh [PACKAGE_GROUP] [OPTIONS]
#
# Package Groups:
#   gpu         - GPU packages only (oxigdal-gpu, oxigdal-gpu-advanced)
#   gpu-ml      - GPU + ML packages (default)
#   external    - Packages with external dependencies
#   all         - All workspace packages
#   <package>   - Specific package name (e.g., oxigdal-gpu)
#
# Options:
#   --detect-only       Run detection stage only
#   --analyze-only      Run analysis stage only (requires existing NDJSON)
#   --fix-only          Run fix stage only (requires existing fail-tests.json)
#   --full              Run all stages (default)
#   --dry-run           Preview fixes without applying (default)
#   --apply             Actually apply fixes (requires confirmation)
#   --min-confidence    Minimum confidence for fixes (HIGH, MEDIUM, LOW; default: HIGH)
#   --yes               Skip confirmation prompts
#   --help              Show this help message
#
# Examples:
#   ./scripts/auto-fix-fail-tests.sh gpu --dry-run
#   ./scripts/auto-fix-fail-tests.sh gpu --apply --yes
#   ./scripts/auto-fix-fail-tests.sh oxigdal-gpu --analyze-only
#   ./scripts/auto-fix-fail-tests.sh --fix-only --apply --min-confidence MEDIUM

set -uo pipefail

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
MAGENTA='\033[0;35m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color
BOLD='\033[1m'

# Script paths
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DETECT_SCRIPT="$SCRIPT_DIR/detect-fail-tests.sh"
ANALYZE_SCRIPT="$SCRIPT_DIR/analyze-fail-tests.py"
AUTO_FIX_TOOL="$PROJECT_ROOT/tools/auto-fix-tests"
OUTPUT_DIR="$PROJECT_ROOT/target/fail-test-detection"

# Default arguments
PACKAGE_GROUP="gpu-ml"
MODE="full"
FIX_MODE="dry-run"
MIN_CONFIDENCE="HIGH"
SKIP_CONFIRMATION=false

# Logging functions
log_header() {
    echo -e "\n${BOLD}${CYAN}============================================================${NC}"
    echo -e "${BOLD}${CYAN}$*${NC}"
    echo -e "${BOLD}${CYAN}============================================================${NC}\n"
}

log_stage() {
    echo -e "\n${BOLD}${BLUE}[$1] $2${NC}"
    echo -e "${BLUE}$(printf '=%.0s' {1..60})${NC}\n"
}

log_info() {
    echo -e "${BLUE}[INFO]${NC} $*"
}

log_success() {
    echo -e "${GREEN}[✓]${NC} $*"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $*"
}

log_error() {
    echo -e "${RED}[✗]${NC} $*"
}

log_step() {
    echo -e "${MAGENTA}  →${NC} $*"
}

# Show usage
show_usage() {
    cat << EOF
Usage: $(basename "$0") [PACKAGE_GROUP] [OPTIONS]

Orchestrates fail test detection, analysis, and automated fixing.

Package Groups:
  gpu         GPU packages only
  gpu-ml      GPU + ML packages (default)
  external    Packages with external dependencies
  all         All workspace packages
  <package>   Specific package name

Options:
  --detect-only       Run detection stage only
  --analyze-only      Run analysis stage only
  --fix-only          Run fix stage only
  --full              Run all stages (default)
  --dry-run           Preview fixes without applying (default)
  --apply             Actually apply fixes
  --min-confidence    Minimum confidence (HIGH, MEDIUM, LOW; default: HIGH)
  --yes               Skip confirmation prompts
  --help              Show this help message

Examples:
  $(basename "$0") gpu --dry-run
  $(basename "$0") gpu --apply --yes
  $(basename "$0") oxigdal-gpu --analyze-only
  $(basename "$0") --fix-only --apply --min-confidence MEDIUM

Workflow Stages:
  1. DETECT    - Run nextest with --no-fail-fast to collect all failures
  2. ANALYZE   - Classify failures using taxonomy patterns
  3. FIX       - Apply automated fixes (timeout, ignore, etc.)
  4. VERIFY    - Re-run tests to confirm fixes worked

EOF
}

# Parse command line arguments
parse_args() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            --help|-h)
                show_usage
                exit 0
                ;;
            --detect-only)
                MODE="detect"
                shift
                ;;
            --analyze-only)
                MODE="analyze"
                shift
                ;;
            --fix-only)
                MODE="fix"
                shift
                ;;
            --full)
                MODE="full"
                shift
                ;;
            --dry-run)
                FIX_MODE="dry-run"
                shift
                ;;
            --apply)
                FIX_MODE="apply"
                shift
                ;;
            --min-confidence)
                MIN_CONFIDENCE="$2"
                shift 2
                ;;
            --yes|-y)
                SKIP_CONFIRMATION=true
                shift
                ;;
            -*)
                log_error "Unknown option: $1"
                show_usage
                exit 1
                ;;
            *)
                PACKAGE_GROUP="$1"
                shift
                ;;
        esac
    done
}

# Validate dependencies
check_dependencies() {
    local missing=()

    if [[ ! -x "$DETECT_SCRIPT" ]]; then
        missing+=("detect-fail-tests.sh (not found or not executable)")
    fi

    if [[ ! -f "$ANALYZE_SCRIPT" ]]; then
        missing+=("analyze-fail-tests.py (not found)")
    fi

    if ! command -v python3 &> /dev/null; then
        missing+=("python3 (required for analysis)")
    fi

    if ! command -v cargo &> /dev/null; then
        missing+=("cargo (required for Rust builds)")
    fi

    if ! command -v jq &> /dev/null; then
        log_warning "jq not found - some summary features will be limited"
    fi

    if [[ ${#missing[@]} -gt 0 ]]; then
        log_error "Missing required dependencies:"
        for dep in "${missing[@]}"; do
            echo -e "  ${RED}✗${NC} $dep"
        done
        return 1
    fi

    return 0
}

# Stage 1: DETECT - Run fail test detection
stage_detect() {
    log_stage "1/4" "DETECTION"

    log_step "Running: $DETECT_SCRIPT $PACKAGE_GROUP"

    if ! "$DETECT_SCRIPT" "$PACKAGE_GROUP"; then
        log_error "Detection stage failed"
        return 1
    fi

    # Count failures in NDJSON files
    local failure_count=0
    local package_count=0

    if command -v jq &> /dev/null; then
        for ndjson_file in "$OUTPUT_DIR"/*.ndjson; do
            if [[ ! -f "$ndjson_file" ]]; then
                continue
            fi

            ((package_count++)) || true

            local file_failures=0
            while IFS= read -r line; do
                if [[ -z "$line" ]]; then
                    continue
                fi

                local event
                event=$(echo "$line" | jq -r 'select(.type == "test") | .event // empty' 2>/dev/null)
                if [[ "$event" == "failed" ]]; then
                    ((file_failures++)) || true
                fi
            done < "$ndjson_file"

            ((failure_count += file_failures)) || true
        done

        log_success "Detected $failure_count failures across $package_count packages"
        log_info "Output: $OUTPUT_DIR/*.ndjson"
    else
        log_success "Detection completed"
        log_info "Output: $OUTPUT_DIR/"
        log_warning "Install jq for detailed statistics"
    fi

    return 0
}

# Stage 2: ANALYZE - Classify failures using taxonomy
stage_analyze() {
    log_stage "2/4" "ANALYSIS"

    # Check for NDJSON files
    local ndjson_count
    ndjson_count=$(find "$OUTPUT_DIR" -name "*.ndjson" 2>/dev/null | wc -l | tr -d ' ')

    if [[ "$ndjson_count" -eq 0 ]]; then
        log_error "No NDJSON files found in $OUTPUT_DIR"
        log_info "Run with --detect-only first, or ensure detection stage completed successfully"
        return 1
    fi

    log_step "Running: python3 $ANALYZE_SCRIPT"

    if ! python3 "$ANALYZE_SCRIPT"; then
        log_error "Analysis stage failed"
        return 1
    fi

    # Verify outputs were created
    if [[ ! -f "$OUTPUT_DIR/fail-tests.json" ]]; then
        log_error "Expected output file not created: fail-tests.json"
        return 1
    fi

    if [[ ! -f "$OUTPUT_DIR/fail-report.md" ]]; then
        log_warning "Markdown report not created (not critical)"
    fi

    # Extract summary from JSON
    if command -v jq &> /dev/null; then
        local total
        local auto_fixable

        total=$(jq -r '.total_failures // 0' "$OUTPUT_DIR/fail-tests.json")
        auto_fixable=$(jq -r '[.failures[] | select(.auto_fix != "none")] | length' "$OUTPUT_DIR/fail-tests.json")

        log_success "Classified $total failures ($auto_fixable auto-fixable)"
        log_info "Report: $OUTPUT_DIR/fail-report.md"
        log_info "JSON: $OUTPUT_DIR/fail-tests.json"
    else
        log_success "Analysis completed"
        log_info "Reports: $OUTPUT_DIR/fail-report.md, fail-tests.json"
    fi

    return 0
}

# Stage 3: FIX - Apply automated fixes
stage_fix() {
    log_stage "3/4" "AUTO-FIX ($FIX_MODE)"

    # Check for fail-tests.json
    if [[ ! -f "$OUTPUT_DIR/fail-tests.json" ]]; then
        log_error "fail-tests.json not found in $OUTPUT_DIR"
        log_info "Run analysis stage first with --analyze-only"
        return 1
    fi

    # Build auto-fix tool if needed
    if [[ ! -f "$AUTO_FIX_TOOL/target/release/auto-fix-tests" ]] && [[ ! -f "$AUTO_FIX_TOOL/target/debug/auto-fix-tests" ]]; then
        log_step "Building auto-fix-tests tool..."
        cd "$AUTO_FIX_TOOL" || return 1
        if ! cargo build --release 2>&1 | tail -20; then
            log_error "Failed to build auto-fix-tests tool"
            return 1
        fi
        cd "$PROJECT_ROOT" || return 1
        log_success "Tool built successfully"
    fi

    # Prepare arguments for auto-fix tool
    local fix_args=()
    fix_args+=("--input-file" "$OUTPUT_DIR/fail-tests.json")
    fix_args+=("--crates-dir" "$PROJECT_ROOT/crates")
    fix_args+=("--skip-validation")  # Skip pre-flight validation (known issue with some packages)

    if [[ "$FIX_MODE" == "dry-run" ]]; then
        fix_args+=("--dry-run")
    elif [[ "$FIX_MODE" == "apply" ]]; then
        fix_args+=("--apply")

        # Confirmation prompt unless --yes was passed
        if [[ "$SKIP_CONFIRMATION" == false ]]; then
            echo ""
            log_warning "About to apply automated fixes to source files"
            log_info "Backups will be created in .auto-fix-backups/"
            echo -e "${YELLOW}Continue?${NC} [y/N] "
            read -r response
            if [[ ! "$response" =~ ^[Yy]$ ]]; then
                log_info "Aborted by user"
                return 1
            fi
        fi

        fix_args+=("--yes")
    fi

    # Add min-confidence if specified
    if [[ -n "$MIN_CONFIDENCE" ]]; then
        fix_args+=("--min-confidence" "$MIN_CONFIDENCE")
    fi

    # Run auto-fix tool
    log_step "Running: cargo run --release -- ${fix_args[*]}"

    cd "$AUTO_FIX_TOOL" || return 1
    if ! cargo run --release -- "${fix_args[@]}"; then
        log_error "Auto-fix stage failed"
        cd "$PROJECT_ROOT" || return 1
        return 1
    fi
    cd "$PROJECT_ROOT" || return 1

    if [[ "$FIX_MODE" == "dry-run" ]]; then
        log_success "Dry-run completed (no changes applied)"
        log_info "Use --apply to apply fixes"
    else
        log_success "Fixes applied successfully"
        log_info "Backups saved in: $PROJECT_ROOT/.auto-fix-backups/"
        log_info "Audit log: $PROJECT_ROOT/auto-fix-audit.log"
    fi

    return 0
}

# Stage 4: VERIFY - Re-run tests to confirm fixes
stage_verify() {
    log_stage "4/4" "VERIFICATION"

    if [[ "$FIX_MODE" == "dry-run" ]]; then
        log_info "Skipping verification (dry-run mode)"
        return 0
    fi

    # Extract packages from fail-tests.json
    local packages
    if command -v jq &> /dev/null; then
        mapfile -t packages < <(jq -r '.failures[].package' "$OUTPUT_DIR/fail-tests.json" | sort -u)
    else
        log_warning "jq not found - cannot determine packages to verify"
        log_info "Manually verify tests with: cargo nextest run -p <package>"
        return 0
    fi

    if [[ ${#packages[@]} -eq 0 ]]; then
        log_warning "No packages found in fail-tests.json"
        return 0
    fi

    log_step "Re-running tests for ${#packages[@]} packages..."

    local verify_failures=0
    local verify_successes=0

    for package in "${packages[@]}"; do
        log_info "Testing: $package"

        # Run tests with nextest
        if cargo nextest run -p "$package" --no-fail-fast > "$OUTPUT_DIR/${package}_verify.log" 2>&1; then
            log_success "  ✓ All tests passed in $package"
            ((verify_successes++)) || true
        else
            log_warning "  ✗ Some tests still failing in $package"
            ((verify_failures++)) || true

            # Show first few failures
            if command -v jq &> /dev/null && [[ -f "$OUTPUT_DIR/${package}_verify.log" ]]; then
                log_step "First 3 failures:"
                grep -i "FAIL\|failed" "$OUTPUT_DIR/${package}_verify.log" | head -3 | sed 's/^/      /'
            fi
        fi
    done

    echo ""
    log_info "Verification summary: $verify_successes passed, $verify_failures with remaining issues"

    if [[ $verify_failures -gt 0 ]]; then
        log_warning "Some packages still have test failures"
        log_info "Review logs in: $OUTPUT_DIR/*_verify.log"
        log_info "Check fail-report.md for manual fix recommendations"
    else
        log_success "All packages verified successfully!"
    fi

    return 0
}

# Main orchestration
main() {
    # Parse arguments
    parse_args "$@"

    # Print header
    log_header "FAIL TEST AUTO-FIX ORCHESTRATION"

    echo -e "${BOLD}Configuration:${NC}"
    echo -e "  Package Group:    ${CYAN}$PACKAGE_GROUP${NC}"
    echo -e "  Mode:             ${CYAN}$MODE${NC}"
    echo -e "  Fix Mode:         ${CYAN}$FIX_MODE${NC}"
    echo -e "  Min Confidence:   ${CYAN}$MIN_CONFIDENCE${NC}"
    echo -e "  Output Directory: ${CYAN}$OUTPUT_DIR${NC}"
    echo ""

    # Check dependencies
    if ! check_dependencies; then
        exit 1
    fi

    # Create output directory
    mkdir -p "$OUTPUT_DIR"

    # Execute stages based on mode
    local exit_code=0

    case "$MODE" in
        detect)
            stage_detect || exit_code=1
            ;;
        analyze)
            stage_analyze || exit_code=1
            ;;
        fix)
            stage_fix || exit_code=1
            ;;
        full)
            if ! stage_detect; then
                log_error "Detection stage failed, aborting"
                exit 1
            fi

            if ! stage_analyze; then
                log_error "Analysis stage failed, aborting"
                exit 1
            fi

            if ! stage_fix; then
                log_error "Fix stage failed, aborting"
                exit 1
            fi

            if ! stage_verify; then
                log_warning "Verification stage failed (not critical)"
                # Don't fail on verification issues
            fi
            ;;
        *)
            log_error "Unknown mode: $MODE"
            exit 1
            ;;
    esac

    # Print summary
    log_header "SUMMARY"

    if command -v jq &> /dev/null && [[ -f "$OUTPUT_DIR/fail-tests.json" ]]; then
        local total
        local auto_fixable
        local by_confidence

        total=$(jq -r '.total_failures // 0' "$OUTPUT_DIR/fail-tests.json" 2>/dev/null || echo "0")
        auto_fixable=$(jq -r '[.failures[] | select(.auto_fix != "none")] | length' "$OUTPUT_DIR/fail-tests.json" 2>/dev/null || echo "0")

        echo -e "${BOLD}Statistics:${NC}"
        echo -e "  Total failures:   ${YELLOW}$total${NC}"

        if [[ "$total" -gt 0 ]]; then
            local percentage
            percentage=$(awk "BEGIN {printf \"%.1f\", ($auto_fixable*100.0/$total)}")
            echo -e "  Auto-fixable:     ${GREEN}$auto_fixable${NC} ($percentage%)"
        else
            echo -e "  Auto-fixable:     ${GREEN}$auto_fixable${NC} (0%)"
        fi
        echo ""

        echo -e "${BOLD}Confidence breakdown:${NC}"
        jq -r '.by_confidence | to_entries[] | "  \(.key): \(.value)"' "$OUTPUT_DIR/fail-tests.json" 2>/dev/null | sed "s/^/  /"
        echo ""
    fi

    echo -e "${BOLD}Mode:${NC} $MODE ($FIX_MODE)"

    if [[ "$FIX_MODE" == "dry-run" ]]; then
        echo -e "${YELLOW}No changes applied (dry-run mode)${NC}"
        echo -e "Use ${BOLD}--apply${NC} to apply fixes"
    else
        echo -e "${GREEN}Fixes have been applied${NC}"
        echo -e "Review: ${CYAN}$PROJECT_ROOT/.auto-fix-backups/${NC}"
    fi

    echo ""
    echo -e "${BOLD}Next steps:${NC}"
    if [[ "$FIX_MODE" == "dry-run" ]]; then
        echo -e "  1. Review the analysis: ${CYAN}$OUTPUT_DIR/fail-report.md${NC}"
        echo -e "  2. Apply fixes: ${CYAN}$(basename "$0") $PACKAGE_GROUP --apply${NC}"
    else
        echo -e "  1. Review verification results in: ${CYAN}$OUTPUT_DIR/*_verify.log${NC}"
        echo -e "  2. Manual fixes needed: ${CYAN}$OUTPUT_DIR/fail-report.md${NC}"
        echo -e "  3. To restore backups: ${CYAN}cd tools/auto-fix-tests && cargo run -- --restore .auto-fix-backups/<timestamp>${NC}"
    fi

    echo ""
    log_header "COMPLETE"

    exit $exit_code
}

# Run main
main "$@"
