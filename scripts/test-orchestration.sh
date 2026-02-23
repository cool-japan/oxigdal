#!/usr/bin/env bash
#
# End-to-end test for auto-fix-fail-tests orchestration script
#
# Tests all modes and argument combinations to verify the script works correctly.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ORCHESTRATOR="$SCRIPT_DIR/auto-fix-fail-tests.sh"
OUTPUT_DIR="$SCRIPT_DIR/../target/fail-test-detection"

# Color output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

test_pass() {
    echo -e "${GREEN}✓${NC} $*"
}

test_fail() {
    echo -e "${RED}✗${NC} $*"
    exit 1
}

test_info() {
    echo -e "${YELLOW}→${NC} $*"
}

echo "========================================"
echo "Testing auto-fix-fail-tests orchestration"
echo "========================================"
echo ""

# Test 1: Help message
test_info "Test 1: Help message displays correctly"
if "$ORCHESTRATOR" --help | grep -q "Orchestrates fail test detection"; then
    test_pass "Help message works"
else
    test_fail "Help message failed"
fi

# Test 2: Detect-only mode
test_info "Test 2: Detect-only mode"
if "$ORCHESTRATOR" oxigdal-gpu --detect-only > /tmp/detect-test.log 2>&1; then
    if grep -q "DETECTION" /tmp/detect-test.log && grep -q "COMPLETE" /tmp/detect-test.log; then
        test_pass "Detect-only mode works"
    else
        test_fail "Detect-only did not complete correctly"
    fi
else
    test_fail "Detect-only mode failed"
fi

# Test 3: Verify NDJSON files were created
test_info "Test 3: NDJSON files created"
if ls "$OUTPUT_DIR"/*.ndjson >/dev/null 2>&1; then
    test_pass "NDJSON files created"
else
    test_fail "No NDJSON files found"
fi

# Test 4: Analyze-only mode
test_info "Test 4: Analyze-only mode"
if "$ORCHESTRATOR" oxigdal-gpu --analyze-only > /tmp/analyze-test.log 2>&1; then
    if grep -q "ANALYSIS" /tmp/analyze-test.log && grep -q "COMPLETE" /tmp/analyze-test.log; then
        test_pass "Analyze-only mode works"
    else
        test_fail "Analyze-only did not complete correctly"
    fi
else
    test_fail "Analyze-only mode failed"
fi

# Test 5: Verify JSON and markdown reports were created
test_info "Test 5: Reports created"
if [[ -f "$OUTPUT_DIR/fail-tests.json" ]] && [[ -f "$OUTPUT_DIR/fail-report.md" ]]; then
    test_pass "Reports created successfully"
else
    test_fail "Reports not found"
fi

# Test 6: Verify JSON structure
test_info "Test 6: JSON structure validation"
if command -v jq &> /dev/null; then
    if jq -e '.total_failures' "$OUTPUT_DIR/fail-tests.json" > /dev/null 2>&1; then
        test_pass "JSON structure is valid"
    else
        test_fail "JSON structure invalid"
    fi
else
    test_pass "JSON validation skipped (jq not installed)"
fi

# Test 7: Fix-only mode (dry-run)
test_info "Test 7: Fix-only mode (dry-run)"
if "$ORCHESTRATOR" --fix-only --dry-run > /tmp/fix-test.log 2>&1; then
    if grep -q "AUTO-FIX" /tmp/fix-test.log && grep -q "dry-run" /tmp/fix-test.log; then
        test_pass "Fix-only dry-run mode works"
    else
        test_fail "Fix-only dry-run did not complete correctly"
    fi
else
    test_fail "Fix-only mode failed"
fi

# Test 8: Full workflow (dry-run)
test_info "Test 8: Full workflow (dry-run)"
if "$ORCHESTRATOR" oxigdal-gpu --full --dry-run > /tmp/full-test.log 2>&1; then
    if grep -q "DETECTION" /tmp/full-test.log && \
       grep -q "ANALYSIS" /tmp/full-test.log && \
       grep -q "AUTO-FIX" /tmp/full-test.log && \
       grep -q "VERIFICATION" /tmp/full-test.log; then
        test_pass "Full workflow works"
    else
        test_fail "Full workflow missing stages"
    fi
else
    test_fail "Full workflow failed"
fi

# Test 9: Argument parsing - package group
test_info "Test 9: Package group argument parsing"
# Strip ANSI color codes before grepping
if "$ORCHESTRATOR" gpu --detect-only 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -q "Package Group:.*gpu"; then
    test_pass "Package group argument works"
else
    test_fail "Package group parsing failed"
fi

# Test 10: Argument parsing - confidence level
test_info "Test 10: Confidence level argument parsing"
# Strip ANSI color codes before grepping
if "$ORCHESTRATOR" --fix-only --dry-run --min-confidence MEDIUM 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -q "Min Confidence:.*MEDIUM"; then
    test_pass "Min confidence argument works"
else
    test_fail "Min confidence parsing failed"
fi

# Test 11: Summary section
test_info "Test 11: Summary section displayed"
if "$ORCHESTRATOR" oxigdal-gpu --detect-only 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -q "SUMMARY"; then
    test_pass "Summary section displayed"
else
    test_fail "Summary section missing"
fi

# Test 12: Statistics calculation
test_info "Test 12: Statistics calculated"
if "$ORCHESTRATOR" oxigdal-gpu --analyze-only 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -q "Auto-fixable:.*%"; then
    test_pass "Statistics calculated correctly"
else
    test_fail "Statistics calculation failed"
fi

echo ""
echo "========================================"
echo -e "${GREEN}All tests passed!${NC}"
echo "========================================"
echo ""
echo "Orchestration script is working correctly."
echo "Clean up test logs with: rm /tmp/*-test.log"
