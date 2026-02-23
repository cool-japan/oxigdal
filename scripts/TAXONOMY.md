# Failure Taxonomy Reference

Complete reference for the test failure classification taxonomy used by the auto-fix system.

## Overview

The taxonomy defines **148 regex patterns** organized into **54 subcategories** across **8 major categories**. Each subcategory specifies:

- **Description**: Human-readable explanation
- **Confidence**: HIGH, MEDIUM, or LOW
- **Auto-fix Strategy**: Recommended fix approach
- **Patterns**: Regex patterns to match against test output
- **Priority** (optional): P1-P5 for triage

## Quick Reference

| Category | Subcategories | Confidence | Common Fix |
|----------|---------------|------------|------------|
| HARDWARE_UNAVAILABLE | 4 | HIGH | add_ignore_with_env_check |
| EXTERNAL_DEPENDENCY | 7 | HIGH-MEDIUM | add_ignore_with_env_check |
| ASYNC_RUNTIME | 4 | HIGH-LOW | add_timeout_or_ignore, none |
| ASSERTION_FAILURE | 5 | HIGH-MEDIUM | add_should_panic, none |
| RESOURCE_NOT_FOUND | 4 | HIGH-MEDIUM | add_skip_if_unavailable, add_env_check |
| COMPILATION_ERROR | 3 | HIGH-MEDIUM | skip_if_unavailable, none |
| SPECIAL_PATTERNS | 5 | MEDIUM-LOW | Various |
| FIX_STRATEGIES | - | - | Metadata |
| PRIORITY_LEVELS | - | - | Metadata |

## Major Categories

### 1. HARDWARE_UNAVAILABLE

**Description**: Tests that fail because required hardware is not present

**Use Case**: GPU-dependent tests on systems without GPU, CUDA tests on non-CUDA systems

**Common Fix**: Add `#[ignore]` with environment check or `#[cfg_attr]` for feature gating

#### Subcategories

##### gpu_device_not_found

**Description**: GPU device not available (CUDA, OpenCL, Metal, Vulkan, WebGPU)

**Confidence**: HIGH

**Auto-fix**: add_ignore_with_env_check

**Patterns** (9):
- `No adapter found`
- `No GPU found`
- `GPU.*not available`
- `wgpu.*RequestDeviceError`
- `Failed to get GPU adapter`
- `No suitable adapter found`
- `Adapter.*not found`
- `Device lost`
- `GPU context creation failed`

**Example Error**:
```
thread 'test_gpu_buffer_creation' panicked at tests/gpu_test.rs:45:5:
No adapter found
```

**Fix Applied**:
```rust
#[test]
#[ignore = "GPU device not available"]
fn test_gpu_buffer_creation() {
    if std::env::var("OXIGDAL_GPU_TESTS").is_err() {
        eprintln!("Skipping GPU test (set OXIGDAL_GPU_TESTS=1 to enable)");
        return;
    }
    // ... test code ...
}
```

##### cuda_unavailable

**Description**: CUDA runtime or device not available

**Confidence**: HIGH

**Auto-fix**: add_ignore_with_env_check

**Patterns** (6):
- `CUDA.*not available`
- `CUDA.*not found`
- `cudaError`
- `CUDA driver version`
- `No CUDA-capable device`
- `CUDA runtime error`

##### opencl_unavailable

**Description**: OpenCL runtime or device not available

**Confidence**: HIGH

**Auto-fix**: add_ignore_with_env_check

**Patterns** (4):
- `OpenCL.*not available`
- `OpenCL.*not found`
- `clGetDeviceIDs.*failed`
- `CL_DEVICE_NOT_FOUND`

##### metal_unavailable

**Description**: Metal framework not available (iOS/macOS)

**Confidence**: HIGH

**Auto-fix**: add_ignore_with_env_check

**Patterns** (3):
- `Metal.*not available`
- `MTL.*failed`
- `Metal device not found`

---

### 2. EXTERNAL_DEPENDENCY

**Description**: Tests requiring external services or infrastructure

**Use Case**: Integration tests that depend on Redis, Kafka, S3, databases, etc.

**Common Fix**: Add `#[ignore]` with environment check

#### Subcategories

##### kafka_unavailable

**Description**: Kafka broker not running or unreachable

**Confidence**: HIGH

**Auto-fix**: add_ignore_with_env_check

**Patterns** (7):
- `Connection refused.*9092`
- `Failed to connect to Kafka`
- `rdkafka.*Local: Broker transport failure`
- `Broker.*not available`
- `kafka.*connection refused`
- `No brokers available`
- `Failed to create Kafka.*client`

**Example Error**:
```
thread 'test_kafka_streaming' panicked:
Connection refused (os error 111): localhost:9092
```

**Fix Applied**:
```rust
#[test]
#[ignore = "Kafka broker not available"]
fn test_kafka_streaming() {
    if std::env::var("OXIGDAL_KAFKA_TESTS").is_err() {
        eprintln!("Skipping Kafka test (set OXIGDAL_KAFKA_TESTS=1 to enable)");
        return;
    }
    // ... test code ...
}
```

##### redis_unavailable

**Description**: Redis server not running or unreachable

**Confidence**: HIGH

**Auto-fix**: add_ignore_with_env_check

**Patterns** (5):
- `Connection refused.*6379`
- `Redis.*connection refused`
- `Failed to connect to Redis`
- `ERR.*invalid password`
- `NOAUTH Authentication required`

##### database_unavailable

**Description**: Database server (PostgreSQL, MySQL, MongoDB, etc.) not available

**Confidence**: HIGH

**Auto-fix**: add_ignore_with_env_check

**Patterns** (8):
- `Connection refused.*5432` (PostgreSQL)
- `Connection refused.*3306` (MySQL)
- `Connection refused.*27017` (MongoDB)
- `database.*not available`
- `Failed to connect to database`
- `FATAL:.*database.*does not exist`
- `Access denied for user`
- `Could not connect to server`

##### s3_unavailable

**Description**: AWS S3 or MinIO not accessible

**Confidence**: HIGH

**Auto-fix**: add_ignore_with_env_check

**Patterns** (7):
- `S3.*NoSuchBucket`
- `S3.*AccessDenied`
- `AWS.*credentials.*not found`
- `InvalidAccessKeyId`
- `SignatureDoesNotMatch`
- `s3://.*not found`
- `MinIO.*connection refused`

##### azure_unavailable

**Description**: Azure Blob Storage not accessible

**Confidence**: HIGH

**Auto-fix**: add_ignore_with_env_check

**Patterns** (4):
- `Azure.*authentication failed`
- `AZURE_STORAGE_ACCOUNT.*not set`
- `Azure.*AccountNotFound`
- `az://.*not found`

##### gcp_unavailable

**Description**: Google Cloud Storage not accessible

**Confidence**: HIGH

**Auto-fix**: add_ignore_with_env_check

**Patterns** (4):
- `GCP.*authentication failed`
- `gs://.*not found`
- `GOOGLE_APPLICATION_CREDENTIALS.*not set`
- `Invalid.*credentials.*Google`

##### http_service_unavailable

**Description**: HTTP/REST service not reachable

**Confidence**: MEDIUM

**Auto-fix**: add_ignore_with_env_check

**Patterns** (6):
- `Connection refused.*http`
- `HTTP.*404.*Not Found`
- `HTTP.*502.*Bad Gateway`
- `HTTP.*503.*Service Unavailable`
- `Failed to connect to.*http`
- `Requires HTTP server setup`

---

### 3. ASYNC_RUNTIME

**Description**: Asynchronous runtime issues including timeouts and race conditions

**Use Case**: Tokio tests that timeout, deadlock, or have race conditions

**Common Fix**: Add `#[ignore]` for timeouts, manual review for deadlocks

#### Subcategories

##### tokio_timeout

**Description**: Test exceeded time limit (slow test)

**Confidence**: HIGH

**Auto-fix**: add_timeout_or_ignore

**Patterns** (7):
- `test timed out`
- `\\(test timed out\\)`
- `TIMEOUT.*\\[.*\\]`
- `has been running for over.*seconds`
- `TERMINATING.*>.*s\\]`
- `operation timed out after`
- `tokio.*time.*elapsed`

**Example Error**:
```
test test_async_operation ... TIMEOUT [120s]
thread 'test_async_operation' panicked:
test timed out
```

**Fix Applied**:
```rust
#[test]
#[ignore = "Test times out (slow)"]
// TODO: Investigate timeout - likely performance issue
fn test_async_operation() {
    // ... test code ...
}
```

##### deadlock_detected

**Description**: Potential deadlock in async code

**Confidence**: MEDIUM

**Auto-fix**: none (requires manual investigation)

**Patterns** (4):
- `deadlock`
- `thread.*blocked`
- `lock.*poisoned`
- `await.*never completed`

##### async_panic

**Description**: Panic in async task

**Confidence**: MEDIUM

**Auto-fix**: none (requires manual investigation)

**Patterns** (3):
- `task.*panicked`
- `async.*panic`
- `future.*panic`

##### race_condition

**Description**: Flaky test indicating race condition

**Confidence**: LOW

**Auto-fix**: none (requires manual investigation)

**Patterns** (3):
- `race condition`
- `sometimes passes`
- `flaky test`

---

### 4. ASSERTION_FAILURE

**Description**: Test assertions failed due to logic errors or incorrect expectations

**Use Case**: Logic bugs, incorrect expectations, validation issues

**Common Fix**: Manual review (most cases), add `#[should_panic]` for expected panics

#### Subcategories

##### expected_panic

**Description**: Test should panic but didn't (or vice versa)

**Confidence**: HIGH

**Auto-fix**: add_should_panic

**Patterns** (4):
- `test did not panic`
- `test panicked unexpectedly`
- `should have panicked`
- `unexpected panic`

**Example Error**:
```
thread 'test_division_by_zero' panicked:
test did not panic as expected
```

**Fix Applied**:
```rust
#[test]
#[should_panic(expected = "division by zero")]
fn test_division_by_zero() {
    let result = 10 / 0;
}
```

##### assertion_equality

**Description**: Equality assertion failed (assert_eq, assert_ne)

**Confidence**: MEDIUM

**Auto-fix**: none (requires manual review)

**Patterns** (4):
- `assertion.*failed.*left == right`
- `assertion.*failed.*left != right`
- `assert_eq!.*failed`
- `assert_ne!.*failed`

##### assertion_condition

**Description**: Boolean assertion failed (assert)

**Confidence**: MEDIUM

**Auto-fix**: none (requires manual review)

**Patterns** (3):
- `assertion.*failed`
- `assert!.*failed`
- `condition.*false`

##### float_comparison

**Description**: Floating point comparison issue (NaN, infinity, precision)

**Confidence**: MEDIUM

**Auto-fix**: none (requires manual review)

**Patterns** (5):
- `NaN`
- `infinity`
- `float.*comparison`
- `partial_cmp.*None`
- `is_nan`

##### validation_error

**Description**: Input validation or constraint violation

**Confidence**: MEDIUM

**Auto-fix**: none (requires manual review)

**Patterns** (5):
- `validation.*failed`
- `invalid.*value`
- `constraint.*violated`
- `out of bounds`
- `Invalid.*parameter`

---

### 5. RESOURCE_NOT_FOUND

**Description**: Required resources (files, data, configs) not found

**Use Case**: Missing test data, configuration files, environment variables

**Common Fix**: Add environment check or skip if unavailable

#### Subcategories

##### file_not_found

**Description**: Test data file or resource file missing

**Confidence**: HIGH

**Auto-fix**: add_skip_if_unavailable

**Patterns** (5):
- `No such file or directory`
- `FileNotFound`
- `file.*not found`
- `cannot find.*file`
- `failed to open.*file`

**Example Error**:
```
thread 'test_load_geotiff' panicked:
No such file or directory (os error 2): test_data/sample.tif
```

**Fix Applied**:
```rust
#[test]
#[cfg_attr(not(feature = "test-data"), ignore = "Test data not available")]
fn test_load_geotiff() {
    // ... test code ...
}
```

##### env_var_not_set

**Description**: Required environment variable not set

**Confidence**: HIGH

**Auto-fix**: add_env_check

**Patterns** (5):
- `environment variable.*not set`
- `env::var.*NotPresent`
- `missing.*environment variable`
- `AWS_ACCESS_KEY_ID.*not set`
- `KAFKA_BROKERS.*not set`

**Fix Applied**:
```rust
#[test]
fn test_s3_upload() {
    if std::env::var("AWS_ACCESS_KEY_ID").is_err() {
        eprintln!("Skipping test: AWS_ACCESS_KEY_ID not set");
        return;
    }
    // ... test code ...
}
```

##### config_missing

**Description**: Configuration file or settings missing

**Confidence**: HIGH

**Auto-fix**: add_skip_if_unavailable

**Patterns** (3):
- `config.*not found`
- `configuration.*missing`
- `settings.*not found`

##### data_missing

**Description**: Test data or fixtures missing

**Confidence**: MEDIUM

**Auto-fix**: add_skip_if_unavailable

**Patterns** (3):
- `test data.*not found`
- `fixtures.*missing`
- `sample.*not found`

---

### 6. COMPILATION_ERROR

**Description**: Test compilation or feature gate issues

**Use Case**: Optional dependencies not enabled, feature flags missing

**Common Fix**: Add `#[cfg_attr]` for feature gating

#### Subcategories

##### feature_not_enabled

**Description**: Required cargo feature not enabled

**Confidence**: HIGH

**Auto-fix**: skip_if_unavailable

**Patterns** (4):
- `feature.*not enabled`
- `requires.*feature`
- `enable.*feature.*to use`
- `cargo feature.*required`

**Fix Applied**:
```rust
#[test]
#[cfg_attr(not(feature = "gpu"), ignore = "GPU feature not enabled")]
fn test_gpu_compute() {
    // ... test code ...
}
```

##### dependency_missing

**Description**: Optional dependency not available

**Confidence**: HIGH

**Auto-fix**: skip_if_unavailable

**Patterns** (4):
- `optional dependency.*not available`
- `crate.*not found`
- `module.*not found`
- `dependency.*missing`

##### compilation_failed

**Description**: Test file failed to compile

**Confidence**: MEDIUM

**Auto-fix**: none (requires manual fix)

**Patterns** (5):
- `compilation error`
- `could not compile`
- `syntax error`
- `type mismatch`
- `mismatched types`

---

### 7. SPECIAL_PATTERNS

**Description**: Special cases and edge scenarios

**Use Case**: Platform-specific tests, unstable features, known issues

#### Subcategories

##### platform_specific

**Description**: Test only runs on specific platforms

**Confidence**: MEDIUM

**Auto-fix**: skip_if_unavailable

**Patterns** (5):
- `platform.*not supported`
- `only available on.*OS`
- `Unix-only`
- `Windows-only`
- `macOS-only`

##### unstable_feature

**Description**: Uses unstable Rust features

**Confidence**: LOW

**Auto-fix**: none

**Patterns** (3):
- `unstable feature`
- `nightly.*required`
- `feature gate`

##### known_issue

**Description**: Known bug or limitation

**Confidence**: MEDIUM

**Auto-fix**: add_ignore

**Patterns** (4):
- `known issue`
- `TODO.*fix`
- `FIXME`
- `bug.*tracked`

##### network_failure

**Description**: General network connectivity issue

**Confidence**: MEDIUM

**Auto-fix**: add_ignore_with_env_check

**Patterns** (5):
- `network.*unreachable`
- `DNS.*resolution failed`
- `timeout.*network`
- `connection.*reset`
- `network error`

##### permission_denied

**Description**: Insufficient permissions

**Confidence**: MEDIUM

**Auto-fix**: add_ignore

**Patterns** (3):
- `permission denied`
- `access denied`
- `insufficient.*permissions`

---

### 8. FIX_STRATEGIES (Metadata)

**Description**: Defines available fix strategies

**Strategies**:

| Strategy | Description | Example |
|----------|-------------|---------|
| `add_ignore` | Add `#[ignore]` attribute | `#[ignore = "GPU not available"]` |
| `add_ignore_with_env_check` | Add `#[ignore]` + env check | See examples above |
| `add_env_check` | Insert env var check at start | `if env::var("VAR").is_err() { return; }` |
| `add_timeout_or_ignore` | Add `#[ignore]` with timeout comment | `#[ignore = "Test times out"]` |
| `add_should_panic` | Add `#[should_panic]` | `#[should_panic(expected = "msg")]` |
| `skip_if_unavailable` | Add `#[cfg_attr]` | `#[cfg_attr(not(feature), ignore)]` |
| `none` | No auto-fix (manual review) | - |

---

### 9. PRIORITY_LEVELS (Metadata)

**Description**: Defines priority levels for triage

**Levels**:

| Priority | Description | Response Time |
|----------|-------------|---------------|
| P1_CRITICAL | Blocks CI/CD, breaks builds | Immediate |
| P2_HIGH | Affects core functionality | Same day |
| P3_MEDIUM | Limits test coverage | This week |
| P4_LOW | Minor inconvenience | This sprint |
| P5_TRIVIAL | Documentation, cleanup | Backlog |

## Confidence Levels

### HIGH Confidence

**Definition**: Exact pattern match with clear, safe fix

**Characteristics**:
- Pattern is unambiguous (e.g., "No adapter found")
- Fix is well-defined and safe
- No side effects expected
- Can be auto-applied by default

**Examples**:
- `gpu_device_not_found` - "No adapter found"
- `kafka_unavailable` - "Connection refused.*9092"
- `file_not_found` - "No such file or directory"

**Auto-fix Policy**: Enabled by default

### MEDIUM Confidence

**Definition**: Pattern match with some ambiguity, needs review

**Characteristics**:
- Pattern may have false positives
- Fix requires context or judgment
- May need manual verification
- Requires explicit `--allow-medium` flag

**Examples**:
- `http_service_unavailable` - Generic HTTP errors
- `assertion_equality` - May be logic bug
- `deadlock_detected` - Needs investigation

**Auto-fix Policy**: Requires `--allow-medium` flag

### LOW Confidence

**Definition**: Weak pattern match, manual review required

**Characteristics**:
- High risk of false positive
- No clear automated fix
- Requires developer judgment
- Preview only, auto-fix blocked

**Examples**:
- `race_condition` - "sometimes passes"
- `unstable_feature` - May be intentional
- Generic error patterns

**Auto-fix Policy**: Blocked (dry-run only)

### NONE

**Definition**: No pattern matched (unclassified)

**Characteristics**:
- Test output doesn't match any pattern
- Requires pattern addition to taxonomy
- Manual classification needed

**Auto-fix Policy**: Blocked

## Pattern Syntax

### Regex Flavor

Patterns use Python `re` module syntax (similar to Perl regex):

- `.` - Any character
- `.*` - Zero or more of any character
- `\\.` - Literal dot (escaped)
- `[0-9]` - Character class
- `(foo|bar)` - Alternation
- `^` - Start of line
- `$` - End of line

### Case Sensitivity

All patterns are matched **case-insensitively** (`re.IGNORECASE` flag).

### Examples

```yaml
patterns:
  - "No adapter found"           # Exact phrase (case-insensitive)
  - "GPU.*not available"         # "GPU" followed by anything, then "not available"
  - "Connection refused.*9092"   # Port number after "Connection refused"
  - "wgpu.*RequestDeviceError"   # wgpu error anywhere in output
  - "\\(test timed out\\)"       # Literal parentheses (escaped)
```

## Extending the Taxonomy

### Adding New Subcategory

1. **Choose Category**: Determine which major category fits
2. **Define Subcategory**: Add under appropriate category
3. **Write Patterns**: Create regex patterns based on real errors
4. **Set Confidence**: Assign based on pattern specificity
5. **Choose Fix**: Select appropriate auto-fix strategy
6. **Test**: Verify against real test failures

**Example**:

```yaml
HARDWARE_UNAVAILABLE:
  tpu_unavailable:
    description: "TPU (Tensor Processing Unit) not available"
    confidence: HIGH
    auto_fix: add_ignore_with_env_check
    priority: P2_HIGH
    patterns:
      - "TPU.*not found"
      - "No TPU devices available"
      - "TPU.*initialization failed"
```

### Adding New Category

1. **Define Category**: Add top-level category
2. **Write Description**: Explain category purpose
3. **Add Subcategories**: At least 2-3 subcategories
4. **Document**: Update this reference

**Example**:

```yaml
SECURITY_ISSUE:
  description: "Security-related test failures"

  auth_failure:
    description: "Authentication or authorization failed"
    confidence: MEDIUM
    auto_fix: add_ignore_with_env_check
    patterns:
      - "authentication.*failed"
      - "unauthorized"
      - "access token.*invalid"
```

### Best Practices

**Pattern Writing**:
- Start with specific patterns (reduce false positives)
- Use `.*` for flexibility, but not excessively
- Test against real error output
- Avoid overly generic patterns (e.g., just "error")
- Escape special regex characters (parentheses, dots, etc.)

**Confidence Assignment**:
- HIGH: Pattern is unambiguous, fix is safe
- MEDIUM: Some ambiguity or context needed
- LOW: High risk of false positive or unclear fix

**Fix Strategy Selection**:
- `add_ignore_with_env_check`: Hardware/service unavailable
- `add_env_check`: Missing env vars
- `skip_if_unavailable`: Feature/dependency missing
- `add_should_panic`: Expected panic
- `add_timeout_or_ignore`: Timeout tests
- `none`: Needs manual investigation

**Testing New Patterns**:

```bash
# Add pattern to taxonomy
vim scripts/failure-taxonomy.yaml

# Test classification
python3 scripts/analyze-fail-tests.py --verbose

# Review report
cat target/fail-test-detection/fail-report.md

# Check matched patterns
jq '.[] | select(.subcategory == "your_new_category")' \
  target/fail-test-detection/fail-tests.json
```

## Pattern Statistics

**By Category**:

| Category | Subcategories | Patterns | Avg Patterns/Subcategory |
|----------|---------------|----------|--------------------------|
| HARDWARE_UNAVAILABLE | 4 | 22 | 5.5 |
| EXTERNAL_DEPENDENCY | 7 | 41 | 5.9 |
| ASYNC_RUNTIME | 4 | 17 | 4.3 |
| ASSERTION_FAILURE | 5 | 21 | 4.2 |
| RESOURCE_NOT_FOUND | 4 | 16 | 4.0 |
| COMPILATION_ERROR | 3 | 13 | 4.3 |
| SPECIAL_PATTERNS | 5 | 20 | 4.0 |
| **TOTAL** | **32** | **150** | **4.7** |

**By Confidence Level**:

| Confidence | Subcategories | Percentage | Auto-fix Default |
|------------|---------------|------------|------------------|
| HIGH | 18 | 56% | ✅ Yes |
| MEDIUM | 11 | 34% | ⚠️ Requires `--allow-medium` |
| LOW | 3 | 9% | ❌ No (preview only) |

**By Fix Strategy**:

| Strategy | Subcategories | Percentage |
|----------|---------------|------------|
| add_ignore_with_env_check | 14 | 44% |
| none | 9 | 28% |
| add_skip_if_unavailable | 4 | 13% |
| add_env_check | 2 | 6% |
| add_timeout_or_ignore | 1 | 3% |
| add_should_panic | 1 | 3% |
| skip_if_unavailable | 1 | 3% |

## Usage Examples

### Example 1: GPU Test Classification

**Test Output**:
```
thread 'test_gpu_buffer_creation' panicked at tests/gpu_test.rs:45:5:
No adapter found
stack backtrace:
...
```

**Classification**:
- **Category**: HARDWARE_UNAVAILABLE
- **Subcategory**: gpu_device_not_found
- **Matched Pattern**: "No adapter found"
- **Confidence**: HIGH
- **Auto-fix**: add_ignore_with_env_check

**Applied Fix**:
```rust
#[test]
#[ignore = "GPU device not available"]
fn test_gpu_buffer_creation() {
    if std::env::var("OXIGDAL_GPU_TESTS").is_err() {
        eprintln!("Skipping GPU test (set OXIGDAL_GPU_TESTS=1 to enable)");
        return;
    }
    // ... original test code ...
}
```

### Example 2: Timeout Classification

**Test Output**:
```
test test_large_dataset_processing ... TIMEOUT [120s]
thread 'test_large_dataset_processing' panicked:
test timed out
```

**Classification**:
- **Category**: ASYNC_RUNTIME
- **Subcategory**: tokio_timeout
- **Matched Pattern**: "test timed out"
- **Confidence**: HIGH
- **Auto-fix**: add_timeout_or_ignore

**Applied Fix**:
```rust
#[test]
#[ignore = "Test times out (slow - needs optimization)"]
// TODO: Optimize algorithm or increase timeout
fn test_large_dataset_processing() {
    // ... original test code ...
}
```

### Example 3: Missing Environment Variable

**Test Output**:
```
thread 'test_kafka_producer' panicked:
environment variable not set: KAFKA_BROKERS
```

**Classification**:
- **Category**: RESOURCE_NOT_FOUND
- **Subcategory**: env_var_not_set
- **Matched Pattern**: "environment variable.*not set"
- **Confidence**: HIGH
- **Auto-fix**: add_env_check

**Applied Fix**:
```rust
#[test]
fn test_kafka_producer() {
    if std::env::var("KAFKA_BROKERS").is_err() {
        eprintln!("Skipping test: KAFKA_BROKERS not set");
        return;
    }
    // ... original test code ...
}
```

## Related Documentation

- **[Main Documentation](../docs/FAIL_TEST_DETECTION.md)** - User guide
- **[Scripts Guide](README-fail-test-detection.md)** - Script documentation
- **[Tool README](../tools/auto-fix-tests/README.md)** - Auto-fix tool

## Contributing

To contribute new patterns or categories:

1. Test against real failures in the codebase
2. Add patterns to `scripts/failure-taxonomy.yaml`
3. Update this documentation
4. Verify classification accuracy with `analyze-fail-tests.py --verbose`
5. Submit PR with examples of matched failures

## License

Part of OxiGDAL project. See top-level LICENSE file.
