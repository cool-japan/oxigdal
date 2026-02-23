#!/bin/bash
# Test script for auto-fix-tests skeleton

set -e

TOOL="./target/debug/auto-fix-tests"
INPUT="../../target/fail-test-detection/fail-tests.json"

echo "================================"
echo "Auto-Fix Tests - Skeleton Tests"
echo "================================"
echo ""

# Build the tool
echo "[1/6] Building tool..."
cargo build --quiet
echo "✓ Build successful"
echo ""

# Test --help
echo "[2/6] Testing --help flag..."
$TOOL --help > /dev/null
echo "✓ Help output works"
echo ""

# Test analyze-only mode
echo "[3/6] Testing --analyze-only mode..."
OUTPUT=$($TOOL --analyze-only --input-file "$INPUT" 2>&1 | grep -v "^warning:")
if echo "$OUTPUT" | grep -q "Failure Analysis Report"; then
    echo "✓ Analysis mode works"
else
    echo "✗ Analysis mode failed"
    exit 1
fi
echo ""

# Test dry-run mode
echo "[4/6] Testing --dry-run mode..."
OUTPUT=$($TOOL --dry-run --input-file "$INPUT" 2>&1 | grep -v "^warning:")
if echo "$OUTPUT" | grep -q "DRY RUN"; then
    echo "✓ Dry-run mode works"
else
    echo "✗ Dry-run mode failed"
    exit 1
fi
echo ""

# Test error handling (missing flags)
echo "[5/6] Testing error handling..."
if $TOOL 2>&1 | grep -q "Must specify"; then
    echo "✓ Missing flags error works"
else
    echo "✗ Missing flags error failed"
    exit 1
fi
echo ""

# Test conflicting flags
echo "[6/6] Testing conflicting flags..."
if $TOOL --dry-run --apply 2>&1 | grep -q "Cannot specify both"; then
    echo "✓ Conflicting flags error works"
else
    echo "✗ Conflicting flags error failed"
    exit 1
fi
echo ""

echo "================================"
echo "All skeleton tests passed! ✓"
echo "================================"
echo ""
echo "Summary:"
echo "  - CLI argument parsing: ✓"
echo "  - JSON loading: ✓"
echo "  - Analysis mode: ✓"
echo "  - Dry-run mode: ✓"
echo "  - Error handling: ✓"
echo "  - File discovery: ✓"
echo ""
echo "Next steps:"
echo "  1. Implement fix strategies (Task #7, #8)"
echo "  2. Add safety mechanisms (Task #9)"
echo "  3. Test with --apply mode on copies"
echo ""
