# Fail Test Detection System - Scripts Guide

Complete guide to the scripts powering the automated test failure detection and fixing system.

## Table of Contents

1. [Overview](#overview)
2. [detect-fail-tests.sh](#detect-fail-testssh)
3. [analyze-fail-tests.py](#analyze-fail-testspy)
4. [auto-fix-fail-tests.sh](#auto-fix-fail-testssh)
5. [Shared Library Modules](#shared-library-modules)
6. [Integration Examples](#integration-examples)
7. [Development Guide](#development-guide)

## Overview

The system consists of three main scripts that work together:

```
┌──────────────────────┐
│  detect-fail-tests   │ ──▶ NDJSON files
└──────────────────────┘
           │
           ▼
┌──────────────────────┐
│  analyze-fail-tests  │ ──▶ fail-tests.json + fail-report.md
└──────────────────────┘
           │
           ▼
┌──────────────────────┐
│  auto-fix-fail-tests │ ──▶ Modified test files
└──────────────────────┘ ──▶ Backups + audit log
```

## detect-fail-tests.sh

**Purpose**: Run tests and capture failures in machine-readable format

**Location**: `scripts/detect-fail-tests.sh`

**Language**: Bash

### Usage

```bash
./scripts/detect-fail-tests.sh [PACKAGE_GROUP] [OPTIONS]
```

### Arguments

**Package Groups**:
- `gpu` - GPU packages only (oxigdal-gpu, oxigdal-gpu-advanced)
- `gpu-ml` - GPU + ML packages (default)
- `external` - Packages with external dependencies
- `all` - All workspace packages
- `<package>` - Specific package name

**Options**:
- `--help` - Show usage information

### Examples

```bash
# Detect failures in GPU packages
./scripts/detect-fail-tests.sh gpu

# Detect in specific package
./scripts/detect-fail-tests.sh oxigdal-gpu

# Detect in all packages
./scripts/detect-fail-tests.sh all
```

### Output

Creates files in `target/fail-test-detection/`:

```
target/fail-test-detection/
├── oxigdal-gpu.ndjson           # Test results for oxigdal-gpu
├── oxigdal-gpu-advanced.ndjson  # Test results for oxigdal-gpu-advanced
└── ...
```

**NDJSON Format** (Newline-Delimited JSON):

Each line is a JSON object representing a test event:

```json
{
  "type": "test",
  "event": "failed",
  "test_name": "test_gpu_buffer_creation",
  "package": "oxigdal-gpu",
  "duration": 0.523,
  "stdout": "...",
  "stderr": "thread 'test_gpu_buffer_creation' panicked at...",
  "message": "test failed"
}
```

### Implementation Details

**Process**:
1. Resolve package group to package list
2. For each package:
   - Run `cargo nextest run -p <package>`
   - Capture NDJSON output
   - Save to `target/fail-test-detection/<package>.ndjson`
3. Report summary (passed, failed, skipped counts)

**Environment**:
- Uses nextest profile `ci` if available
- Respects `RUST_BACKTRACE` environment variable
- Disables colors if `CI=true`

**Error Handling**:
- Exits with code 0 even if tests fail (expected behavior)
- Exits with non-zero if nextest command fails
- Creates output directory if missing

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success (tests may have failed, but detection succeeded) |
| 1 | Invalid arguments or usage |
| 2 | Nextest not installed |
| 3 | Package group not found |

## analyze-fail-tests.py

**Purpose**: Classify test failures using pattern matching taxonomy

**Location**: `scripts/analyze-fail-tests.py`

**Language**: Python 3.9+

### Usage

```bash
python scripts/analyze-fail-tests.py [OPTIONS]
```

### Options

```
--input-dir DIR     Directory containing NDJSON files
                    (default: target/fail-test-detection)

--output-dir DIR    Directory for output files
                    (default: target/fail-test-detection)

--taxonomy FILE     Path to taxonomy YAML
                    (default: scripts/failure-taxonomy.yaml)

--verbose           Enable verbose output

--help              Show help message
```

### Examples

```bash
# Analyze with defaults
python scripts/analyze-fail-tests.py

# Custom input directory
python scripts/analyze-fail-tests.py --input-dir /path/to/ndjson

# Verbose mode
python scripts/analyze-fail-tests.py --verbose

# Custom taxonomy
python scripts/analyze-fail-tests.py --taxonomy my-taxonomy.yaml
```

### Input

**Required Files**:
- NDJSON files in input directory (from detect-fail-tests.sh)
- Taxonomy YAML file (scripts/failure-taxonomy.yaml)

**NDJSON Format**: Nextest machine-readable output

**Taxonomy Format**: YAML with patterns and metadata

### Output

Creates two files in output directory:

**1. fail-tests.json** - Machine-readable classification:

```json
[
  {
    "test_name": "test_gpu_buffer_creation",
    "package": "oxigdal-gpu",
    "file_path": "tests/gpu_test.rs",
    "duration": 0.523,
    "category": "HARDWARE_UNAVAILABLE",
    "subcategory": "gpu_device_not_found",
    "confidence": "HIGH",
    "auto_fix": "add_ignore_with_env_check",
    "priority": "P2_HIGH",
    "error_context": "No adapter found",
    "recommended_action": "Add environment check or ignore attribute",
    "raw_output": "thread 'test_gpu_buffer_creation' panicked..."
  }
]
```

**2. fail-report.md** - Human-readable markdown report:

```markdown
# Test Failure Analysis Report

**Generated:** 2026-02-13 09:45:23 UTC
**Total Failures:** 12
**Classified:** 12 (100.0%)
**Unclassified:** 0 (0.0%)

## Summary by Category

| Category | Count | Percentage |
|----------|-------|------------|
| HARDWARE_UNAVAILABLE | 8 | 66.7% |
| EXTERNAL_DEPENDENCY | 4 | 33.3% |

## HIGH Confidence Failures (12 tests)

### HARDWARE_UNAVAILABLE > gpu_device_not_found (8 tests)

#### oxigdal-gpu::test_gpu_buffer_creation
- **File:** tests/gpu_test.rs:45
- **Duration:** 0.523s
- **Fix:** add_ignore_with_env_check
- **Error:** No adapter found
- **Action:** Add environment check or ignore attribute

...
```

### Implementation Details

**Architecture**:

```python
┌─────────────────────────┐
│  FailureClassifier      │
│  • Load taxonomy YAML   │
│  • Build pattern index  │
│  • Match patterns       │
│  • Assign confidence    │
└────────┬────────────────┘
         │
         ├──▶ Pattern Matching Engine
         │    • Regex compilation
         │    • Multi-pattern matching
         │    • Confidence scoring
         │
         ├──▶ Report Generator
         │    • JSON serialization
         │    • Markdown formatting
         │    • Summary statistics
         │
         └──▶ File Location Estimator
              • Heuristic matching
              • Test name parsing
              • Package structure
```

**Classification Algorithm**:

1. **Load Taxonomy**: Parse YAML, build pattern index (137 patterns)
2. **Parse NDJSON**: Extract failed tests with stdout/stderr
3. **Pattern Matching**:
   - For each test failure:
     - Concatenate stdout + stderr
     - Match against all patterns (regex)
     - Find first match (categories ordered by priority)
     - Extract confidence, fix strategy, priority
4. **Location Estimation**:
   - Try `tests/<test_name>.rs`
   - Try `tests/integration_test.rs`
   - Try `src/lib.rs` (for module tests)
   - Fall back to `tests/unknown.rs`
5. **Report Generation**:
   - JSON: Structured data for auto-fix tool
   - Markdown: Human-readable categorized report

**Pattern Matching Logic**:

```python
def classify_failure(self, test_failure):
    combined_output = test_failure.stdout + test_failure.stderr

    # Try each category/subcategory in order
    for category, subcategory, config in self.pattern_index:
        for pattern in config['patterns']:
            if re.search(pattern, combined_output, re.IGNORECASE):
                return Classification(
                    category=category,
                    subcategory=subcategory,
                    confidence=config['confidence'],
                    auto_fix=config['auto_fix'],
                    ...
                )

    # No match found
    return Classification(confidence='NONE', ...)
```

**Performance**:
- Processes ~100 failures in <1 second
- Compiled regex patterns cached
- Single-pass matching algorithm

### Dependencies

```bash
pip install pyyaml
```

**Version Requirements**:
- Python 3.9+
- PyYAML 6.0+

### Error Handling

| Error | Handling |
|-------|----------|
| No NDJSON files | Exit with error message |
| Invalid NDJSON | Skip malformed lines, warn |
| Missing taxonomy | Exit with error message |
| Invalid YAML | Exit with parse error |
| No failures found | Generate empty report |

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Invalid arguments |
| 2 | No NDJSON files found |
| 3 | Taxonomy file not found |
| 4 | YAML parse error |

## auto-fix-fail-tests.sh

**Purpose**: Orchestrate the complete workflow (detect → analyze → fix → verify)

**Location**: `scripts/auto-fix-fail-tests.sh`

**Language**: Bash

### Usage

```bash
./scripts/auto-fix-fail-tests.sh [PACKAGE_GROUP] [OPTIONS]
```

### Arguments

**Package Groups**: Same as detect-fail-tests.sh (gpu, gpu-ml, external, all, <package>)

**Stage Control**:
- `--detect-only` - Run detection only
- `--analyze-only` - Run analysis only (requires existing NDJSON)
- `--fix-only` - Run fixing only (requires existing JSON)
- `--full` - Run all stages (default)

**Fix Mode**:
- `--dry-run` - Preview changes without applying (default)
- `--apply` - Actually apply fixes (with confirmation)

**Confidence**:
- `--min-confidence HIGH` - High confidence only (default)
- `--min-confidence MEDIUM` - Include medium confidence
- `--min-confidence LOW` - Include low confidence (preview only)

**Automation**:
- `--yes` - Skip confirmation prompts (for CI)

**Help**:
- `--help` - Show usage information

### Examples

```bash
# Full workflow with dry-run (default)
./scripts/auto-fix-fail-tests.sh gpu

# Full workflow with apply
./scripts/auto-fix-fail-tests.sh gpu --apply

# Automated (CI mode)
./scripts/auto-fix-fail-tests.sh all --apply --yes

# Detect only
./scripts/auto-fix-fail-tests.sh gpu --detect-only

# Analyze only (requires existing NDJSON)
./scripts/auto-fix-fail-tests.sh gpu --analyze-only

# Fix only (requires existing fail-tests.json)
./scripts/auto-fix-fail-tests.sh gpu --fix-only --apply

# Include MEDIUM confidence
./scripts/auto-fix-fail-tests.sh gpu --apply --min-confidence MEDIUM --allow-medium
```

### Workflow

**Stage 1: Detection**
```bash
log_stage "STAGE 1" "Test Detection"
$DETECT_SCRIPT $PACKAGE_GROUP
```

**Stage 2: Analysis**
```bash
log_stage "STAGE 2" "Failure Classification"
python3 $ANALYZE_SCRIPT --input-dir $OUTPUT_DIR --output-dir $OUTPUT_DIR
```

**Stage 3: Fixing**
```bash
log_stage "STAGE 3" "Apply Auto-Fixes"
cd $AUTO_FIX_TOOL
cargo run --release -- \
  --input-file $OUTPUT_DIR/fail-tests.json \
  --apply \
  --min-confidence $MIN_CONFIDENCE \
  --yes
```

**Stage 4: Verification**
```bash
log_stage "STAGE 4" "Verify Fixes"
cargo nextest run -p $PACKAGE
```

### Output

**Console Output** (with colors):

```
============================================================
                Auto-Fix Failed Tests
============================================================

Package Group: gpu
Mode:          full
Fix Mode:      dry-run
Confidence:    HIGH
Output Dir:    target/fail-test-detection

============================================================
[STAGE 1] Test Detection
============================================================

  → Running nextest on: oxigdal-gpu, oxigdal-gpu-advanced
  → Saving output to: target/fail-test-detection/

[✓] Detection complete: 12 failures found

============================================================
[STAGE 2] Failure Classification
============================================================

  → Loading taxonomy: scripts/failure-taxonomy.yaml
  → Processing NDJSON files
  → Generating reports

[✓] Classification complete: 12/12 classified (100.0%)

============================================================
[STAGE 3] Apply Auto-Fixes (DRY-RUN)
============================================================

  → HIGH confidence: 12 tests
  → MEDIUM confidence: 0 tests
  → LOW confidence: 0 tests

  → Would fix 12 tests in 2 packages

[✓] Dry-run complete (no changes made)

============================================================
                    Summary
============================================================

Total Failures:     12
Classified:         12 (100.0%)
HIGH Confidence:    12
MEDIUM Confidence:  0
LOW Confidence:     0

Reports:
  • fail-tests.json
  • fail-report.md

Next Steps:
  • Review: cat target/fail-test-detection/fail-report.md
  • Apply:  ./scripts/auto-fix-fail-tests.sh gpu --apply
```

### Implementation Details

**Package Group Resolution**:

```bash
resolve_package_group() {
    case "$1" in
        "gpu")
            PACKAGES="oxigdal-gpu oxigdal-gpu-advanced"
            ;;
        "gpu-ml")
            PACKAGES="oxigdal-gpu oxigdal-gpu-advanced oxigdal-ml-*"
            ;;
        "external")
            PACKAGES="oxigdal-streaming oxigdal-edge"
            ;;
        "all")
            PACKAGES="--workspace"
            ;;
        *)
            PACKAGES="$1"  # Specific package name
            ;;
    esac
}
```

**Stage Control Logic**:

```bash
run_stage_1() {
    if [[ "$MODE" == "full" || "$MODE" == "detect-only" ]]; then
        # Run detection
        $DETECT_SCRIPT $PACKAGE_GROUP || exit 1
    fi
}

run_stage_2() {
    if [[ "$MODE" == "full" || "$MODE" == "analyze-only" ]]; then
        # Run analysis
        python3 $ANALYZE_SCRIPT || exit 1
    fi
}

run_stage_3() {
    if [[ "$MODE" == "full" || "$MODE" == "fix-only" ]]; then
        # Run fixing
        cd $AUTO_FIX_TOOL
        cargo run --release -- $FIX_ARGS || exit 1
    fi
}
```

**Error Handling**:

```bash
set -uo pipefail  # Exit on undefined variable or pipe failure

# Check prerequisites
check_prerequisites() {
    command -v cargo nextest >/dev/null || {
        log_error "cargo-nextest not installed"
        exit 2
    }

    command -v python3 >/dev/null || {
        log_error "python3 not installed"
        exit 2
    }
}
```

### Color Output

**Color Scheme**:

| Color | Usage |
|-------|-------|
| Cyan | Headers and sections |
| Blue | Info messages and stage markers |
| Green | Success messages |
| Yellow | Warnings |
| Red | Errors |
| Magenta | Step indicators |

**Color Functions**:

```bash
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'  # No Color

log_success() { echo -e "${GREEN}[✓]${NC} $*"; }
log_error() { echo -e "${RED}[✗]${NC} $*"; }
log_warning() { echo -e "${YELLOW}[WARNING]${NC} $*"; }
```

**CI Mode**: Colors disabled when `CI=true`

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Invalid arguments or stage failure |
| 2 | Missing prerequisites |
| 3 | Package group not found |

## Shared Library Modules

Located in `scripts/lib/`, used by analyze-fail-tests.py.

### nextest_parser.py

**Purpose**: Parse nextest NDJSON output

**Key Classes**:

```python
@dataclass
class TestStatus:
    test_name: str
    package: str
    duration: float
    status: str  # 'passed', 'failed', 'skipped'
    stdout: str
    stderr: str
```

**Key Functions**:

```python
def parse_nextest_ndjson(ndjson_path: Path) -> List[TestStatus]:
    """
    Parse nextest NDJSON file.

    Returns list of TestStatus objects for all tests.
    """
```

**Usage**:

```python
from lib.nextest_parser import parse_nextest_ndjson

tests = parse_nextest_ndjson(Path('target/fail-test-detection/oxigdal-gpu.ndjson'))
failures = [t for t in tests if t.status == 'failed']
```

### report_generator.py

**Purpose**: Generate human-readable reports and estimate file locations

**Key Functions**:

```python
def estimate_test_location(test_name: str, package: str) -> str:
    """
    Estimate file location from test name.

    Heuristics:
    1. tests/<test_name>.rs
    2. tests/<module>_test.rs
    3. tests/integration_test.rs
    4. src/lib.rs
    """
```

```python
def generate_markdown_report(
    classifications: List[FailureClassification],
    output_path: Path
) -> None:
    """
    Generate human-readable markdown report.

    Includes:
    - Summary statistics
    - Breakdown by category/confidence
    - Detailed test information
    - Recommended actions
    """
```

**Usage**:

```python
from lib.report_generator import estimate_test_location, generate_markdown_report

# Estimate location
file_path = estimate_test_location('test_gpu_buffer', 'oxigdal-gpu')
# Returns: 'tests/gpu_test.rs'

# Generate report
generate_markdown_report(classifications, Path('fail-report.md'))
```

## Integration Examples

### Example 1: CI Pipeline

```bash
#!/bin/bash
# .github/workflows/auto-fix.sh

set -euo pipefail

# Run full workflow
./scripts/auto-fix-fail-tests.sh all --apply --yes

# Commit if changes
if git diff --quiet; then
    echo "No changes to commit"
else
    git add -A
    git commit -m "Auto-fix failing tests"
    git push
fi
```

### Example 2: Pre-commit Hook

```bash
#!/bin/bash
# .git/hooks/pre-commit

# Check staged tests
./scripts/auto-fix-fail-tests.sh all --dry-run

# Show summary
if [ -f target/fail-test-detection/fail-report.md ]; then
    echo "=== Test Failure Summary ==="
    head -30 target/fail-test-detection/fail-report.md
fi
```

### Example 3: Nightly Job

```bash
#!/bin/bash
# scripts/nightly-test-cleanup.sh

# Run on all packages
./scripts/auto-fix-fail-tests.sh all --apply --yes

# Generate detailed report
cat target/fail-test-detection/fail-report.md | \
    mail -s "Nightly Test Cleanup Report" team@example.com
```

### Example 4: Manual Investigation

```bash
# 1. Detect failures
./scripts/detect-fail-tests.sh gpu

# 2. Analyze only (review first)
./scripts/auto-fix-fail-tests.sh gpu --analyze-only

# 3. Review report
cat target/fail-test-detection/fail-report.md

# 4. Fix specific confidence level
./scripts/auto-fix-fail-tests.sh gpu --fix-only --apply --min-confidence HIGH
```

## Development Guide

### Adding New Scripts

**Structure**:

```bash
#!/usr/bin/env bash
#
# Script description
#
# Usage:
#   ./scripts/my-script.sh [OPTIONS]

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# ... implementation ...
```

**Best Practices**:
- Use `set -uo pipefail` for safety
- Document usage in header comment
- Use color output for user-facing scripts
- Check prerequisites before running
- Provide clear error messages
- Exit with meaningful codes

### Extending the Analyzer

**Add New Classification Logic**:

```python
# scripts/analyze-fail-tests.py

class FailureClassifier:
    def classify_custom(self, test_failure):
        # Custom classification logic
        if self.is_custom_failure(test_failure):
            return Classification(
                category='CUSTOM_CATEGORY',
                subcategory='custom_type',
                confidence='HIGH',
                ...
            )
```

**Add New Report Format**:

```python
# scripts/lib/report_generator.py

def generate_json_report(classifications, output_path):
    """Generate JSON report."""
    data = [c.__dict__ for c in classifications]
    with open(output_path, 'w') as f:
        json.dump(data, f, indent=2)
```

### Testing Scripts

**Unit Testing**:

```bash
# Test detection script
./scripts/detect-fail-tests.sh gpu
test -f target/fail-test-detection/oxigdal-gpu.ndjson || exit 1

# Test analyzer
python3 scripts/analyze-fail-tests.py
test -f target/fail-test-detection/fail-tests.json || exit 1
```

**Integration Testing**:

```bash
# Full workflow test
./scripts/auto-fix-fail-tests.sh gpu --dry-run

# Verify outputs
test -f target/fail-test-detection/fail-tests.json || exit 1
test -f target/fail-test-detection/fail-report.md || exit 1
```

### Debugging

**Enable Verbose Mode**:

```bash
# Bash scripts
bash -x ./scripts/auto-fix-fail-tests.sh gpu

# Python scripts
python3 -u scripts/analyze-fail-tests.py --verbose
```

**Check Intermediate Files**:

```bash
# NDJSON output
cat target/fail-test-detection/oxigdal-gpu.ndjson | jq .

# Classification JSON
cat target/fail-test-detection/fail-tests.json | jq .

# Report
cat target/fail-test-detection/fail-report.md
```

**Trace Execution**:

```bash
# Add logging to scripts
log_debug() {
    if [[ "${DEBUG:-}" == "true" ]]; then
        echo "[DEBUG] $*" >&2
    fi
}

# Run with debug
DEBUG=true ./scripts/auto-fix-fail-tests.sh gpu
```

## Related Documentation

- **[Main Documentation](../docs/FAIL_TEST_DETECTION.md)** - User-facing guide
- **[Taxonomy Reference](TAXONOMY.md)** - Pattern reference
- **[Tool README](../tools/auto-fix-tests/README.md)** - Auto-fix tool internals

## License

Part of OxiGDAL project. See top-level LICENSE file.
