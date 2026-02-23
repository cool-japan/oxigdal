# Quick Start: Automated Test Failure Detection & Fixing

Get started with the OxiGDAL fail test detection system in 5 minutes.

## What It Does

Automatically detects, classifies, and fixes common test failures like:
- GPU/hardware not available
- External services (Kafka, Redis, S3) not running
- Test timeouts
- Missing environment variables
- Missing test data files

## Installation

### Prerequisites

```bash
# Install cargo-nextest (required)
cargo install cargo-nextest

# Install Python dependencies (required)
pip install pyyaml
```

### Verify Installation

```bash
# Check nextest
cargo nextest --version

# Check Python
python3 --version

# Check PyYAML
python3 -c "import yaml; print('PyYAML OK')"
```

## Basic Usage

### 1. Detect & Preview (Safe, No Changes)

```bash
# Run on GPU packages, preview only
./scripts/auto-fix-fail-tests.sh gpu --dry-run
```

**Output**:
```
============================================================
                Auto-Fix Failed Tests
============================================================

[STAGE 1] Test Detection
  → Running nextest on: oxigdal-gpu, oxigdal-gpu-advanced
[✓] Detection complete: 12 failures found

[STAGE 2] Failure Classification
  → Processing NDJSON files
[✓] Classification complete: 12/12 classified (100.0%)

[STAGE 3] Apply Auto-Fixes (DRY-RUN)
  → HIGH confidence: 12 tests
  → Would fix 12 tests in 2 packages
[✓] Dry-run complete (no changes made)

Next Steps:
  • Review: cat target/fail-test-detection/fail-report.md
  • Apply:  ./scripts/auto-fix-fail-tests.sh gpu --apply
```

### 2. Review the Report

```bash
# Human-readable markdown report
cat target/fail-test-detection/fail-report.md

# Machine-readable JSON (for tools)
jq . target/fail-test-detection/fail-tests.json
```

**Example Report**:
```markdown
# Test Failure Analysis Report

**Total Failures:** 12
**Classified:** 12 (100.0%)

## HIGH Confidence Failures (12 tests)

### HARDWARE_UNAVAILABLE > gpu_device_not_found (8 tests)

#### oxigdal-gpu::test_gpu_buffer_creation
- **File:** tests/gpu_test.rs:45
- **Fix:** add_ignore_with_env_check
- **Error:** No adapter found
- **Action:** Add environment check or ignore attribute
```

### 3. Apply Fixes (With Confirmation)

```bash
# Apply HIGH confidence fixes (interactive)
./scripts/auto-fix-fail-tests.sh gpu --apply
```

**Confirmation Prompt**:
```
┌─────────────────────────────────────────────┐
│ Auto-Fix Summary                            │
├─────────────────────────────────────────────┤
│ Packages:     oxigdal-gpu, oxigdal-gpu-adv. │
│ Tests:        12                            │
│ Confidence:   HIGH                          │
│ Backup Dir:   .auto-fix-backups/20260213... │
└─────────────────────────────────────────────┘

Proceed with fixes? (y/N): y
```

**Result**:
```
[✓] Backup created: .auto-fix-backups/20260213_094523
[✓] Pre-flight check: All packages compile
[✓] Modified: crates/oxigdal-gpu/tests/gpu_test.rs (8 tests)
[✓] Modified: crates/oxigdal-gpu-advanced/tests/multi_gpu_test.rs (4 tests)
[✓] Post-modification check: All packages compile
[✓] Audit logged: tools/auto-fix-tests/auto-fix-audit.log

Success! 12 tests fixed across 2 packages.
```

### 4. Verify Fixes Worked

```bash
# Re-run tests to verify
cargo nextest run -p oxigdal-gpu

# Check git diff
git diff crates/oxigdal-gpu/tests/
```

**Example Fix Applied**:

**Before**:
```rust
#[test]
fn test_gpu_buffer_creation() {
    let device = get_gpu_device();  // Panics if no GPU
    // ... test code ...
}
```

**After**:
```rust
#[test]
#[ignore = "GPU device not available"]
fn test_gpu_buffer_creation() {
    if std::env::var("OXIGDAL_GPU_TESTS").is_err() {
        eprintln!("Skipping GPU test (set OXIGDAL_GPU_TESTS=1 to enable)");
        return;
    }
    let device = get_gpu_device();
    // ... test code ...
}
```

## Common Workflows

### Workflow 1: Fix GPU Tests

```bash
# 1. Detect and preview
./scripts/auto-fix-fail-tests.sh gpu --dry-run

# 2. Review report
cat target/fail-test-detection/fail-report.md

# 3. Apply fixes
./scripts/auto-fix-fail-tests.sh gpu --apply

# 4. Verify
cargo nextest run -p oxigdal-gpu
```

### Workflow 2: Fix All Failing Tests

```bash
# Fix all workspace packages (HIGH confidence only)
./scripts/auto-fix-fail-tests.sh all --apply
```

### Workflow 3: CI/CD Automation

```bash
# Non-interactive mode for CI
./scripts/auto-fix-fail-tests.sh all --apply --yes

# Exit code 0 if successful
echo $?
```

### Workflow 4: Include Medium Confidence

```bash
# More aggressive fixing (requires explicit flag)
./scripts/auto-fix-fail-tests.sh gpu --apply \
  --min-confidence MEDIUM \
  --allow-medium
```

### Workflow 5: Recovery from Backup

```bash
# If something went wrong
cd tools/auto-fix-tests
cargo run -- --restore .auto-fix-backups/20260213_094523
```

## Package Groups

| Group | Packages | Use Case |
|-------|----------|----------|
| `gpu` | oxigdal-gpu, oxigdal-gpu-advanced | GPU tests only |
| `gpu-ml` | GPU + ML packages | Default |
| `external` | Streaming, edge packages | External services |
| `all` | All workspace packages | Full workspace |
| `<name>` | Specific package | e.g., `oxigdal-gpu` |

**Examples**:
```bash
# GPU only
./scripts/auto-fix-fail-tests.sh gpu --apply

# External services (Kafka, Redis, etc.)
./scripts/auto-fix-fail-tests.sh external --apply

# Specific package
./scripts/auto-fix-fail-tests.sh oxigdal-streaming --apply

# Everything
./scripts/auto-fix-fail-tests.sh all --apply
```

## Fix Strategies

The system applies different fixes based on failure type:

| Failure Type | Fix Applied | Example |
|--------------|-------------|---------|
| GPU not found | `#[ignore]` + env check | Hardware unavailable |
| Service unavailable | `#[ignore]` + env check | Kafka, Redis, S3 |
| Timeout | `#[ignore]` + TODO comment | Slow test |
| Expected panic | `#[should_panic]` | Division by zero |
| Missing env var | Early return check | AWS credentials |
| Missing file | `#[cfg_attr]` | Feature-gated data |

## Safety Features

The system includes comprehensive safety mechanisms:

1. **Automatic Backups** - Every file backed up before modification
2. **Pre-flight Check** - Verifies compilation before changes
3. **Post-modification Verification** - Validates compilation after changes
4. **Auto-rollback** - Automatically restores on failure
5. **Interactive Confirmation** - Shows summary, asks permission
6. **Audit Logging** - Complete history of all operations
7. **Confidence Enforcement** - Defaults to HIGH confidence only

**Backup Location**:
```
tools/auto-fix-tests/.auto-fix-backups/
├── 20260213_094523/     # Timestamped backup
│   ├── gpu_test.rs      # Original file
│   └── ...
└── 20260213_103045/     # Another backup
```

**Audit Log**:
```
tools/auto-fix-tests/auto-fix-audit.log
```

## Confidence Levels

| Level | Description | Default Behavior |
|-------|-------------|------------------|
| HIGH | Exact pattern match, safe fix | ✅ Auto-applied |
| MEDIUM | Some ambiguity, review recommended | ⚠️ Requires `--allow-medium` |
| LOW | Weak pattern, manual review needed | ❌ Preview only |
| NONE | No pattern matched | ❌ Not fixable |

**Example**:
```bash
# Default: HIGH only
./scripts/auto-fix-fail-tests.sh gpu --apply

# Include MEDIUM (more aggressive)
./scripts/auto-fix-fail-tests.sh gpu --apply \
  --min-confidence MEDIUM \
  --allow-medium

# Preview LOW (cannot apply)
./scripts/auto-fix-fail-tests.sh gpu --dry-run \
  --min-confidence LOW
```

## Troubleshooting

### "No NDJSON files found"

**Problem**: Analysis stage can't find test output

**Solution**:
```bash
# Run detection first
./scripts/detect-fail-tests.sh gpu

# Then analyze
./scripts/auto-fix-fail-tests.sh gpu --analyze-only
```

### "Compilation failed"

**Problem**: Fixes broke compilation

**Solution**:
- Auto-rollback activates automatically
- Check `tools/auto-fix-tests/auto-fix-audit.log`
- Manually restore if needed:
  ```bash
  cd tools/auto-fix-tests
  cargo run -- --restore .auto-fix-backups/TIMESTAMP
  ```

### "MEDIUM confidence blocked"

**Problem**: Medium confidence fixes require explicit flag

**Solution**:
```bash
./scripts/auto-fix-fail-tests.sh gpu --apply \
  --min-confidence MEDIUM \
  --allow-medium
```

### "Nothing to fix"

**Problem**: No HIGH confidence failures found

**Solution**:
```bash
# Check report
cat target/fail-test-detection/fail-report.md

# Try MEDIUM confidence (preview)
./scripts/auto-fix-fail-tests.sh gpu --dry-run \
  --min-confidence MEDIUM
```

## Next Steps

### Learn More

- **[Full Documentation](FAIL_TEST_DETECTION.md)** - Complete user guide
- **[Taxonomy Reference](../scripts/TAXONOMY.md)** - All 148 patterns
- **[Scripts Guide](../scripts/README-fail-test-detection.md)** - Script details
- **[Tool README](../tools/auto-fix-tests/README.md)** - Tool internals

### Advanced Usage

- **Custom Package Groups** - Define your own groups
- **Extending Taxonomy** - Add new failure patterns
- **CI/CD Integration** - Automate in GitHub Actions
- **Batch Processing** - Process multiple groups
- **Pattern Development** - Create custom patterns

### Examples

**CI Integration**:
```yaml
# .github/workflows/auto-fix-tests.yml
- name: Auto-fix failing tests
  run: ./scripts/auto-fix-fail-tests.sh all --apply --yes
```

**Pre-commit Hook**:
```bash
#!/bin/bash
# .git/hooks/pre-commit
./scripts/auto-fix-fail-tests.sh all --dry-run
```

**Nightly Cleanup**:
```bash
#!/bin/bash
# scripts/nightly-test-cleanup.sh
./scripts/auto-fix-fail-tests.sh all --apply --yes
```

## Summary

**Quick Commands**:

```bash
# Preview changes
./scripts/auto-fix-fail-tests.sh gpu --dry-run

# Review report
cat target/fail-test-detection/fail-report.md

# Apply fixes (interactive)
./scripts/auto-fix-fail-tests.sh gpu --apply

# Automated (CI)
./scripts/auto-fix-fail-tests.sh all --apply --yes

# Restore backup
cd tools/auto-fix-tests
cargo run -- --restore .auto-fix-backups/TIMESTAMP
```

**Safety Guarantees**:
- ✅ Automatic backups before changes
- ✅ Pre-flight compilation check
- ✅ Post-modification verification
- ✅ Auto-rollback on failure
- ✅ Complete audit trail
- ✅ Interactive confirmation
- ✅ Conservative defaults (HIGH confidence)

**Get Help**:
```bash
# Script help
./scripts/auto-fix-fail-tests.sh --help

# Tool help
cd tools/auto-fix-tests
cargo run -- --help
```

## License

Part of OxiGDAL project. See top-level LICENSE file.
