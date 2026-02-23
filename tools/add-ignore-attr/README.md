# add-ignore-attr

A Rust tool that uses AST manipulation to add `#[ignore]` attributes to slow test functions.

## Features

- Parses Rust source files using the `syn` crate
- Uses `VisitMut` trait to traverse and modify the AST
- Adds `#[ignore]` attributes to test functions identified in a JSON file
- Formats output using `prettyplease` to maintain code quality
- Supports both dry-run (preview) and apply modes
- Recursively scans `tests/` and `src/` directories for test files

## Usage

```bash
# Build the tool
cargo build --release

# Preview changes (dry-run mode)
./target/release/add-ignore-attr --input slow-tests.json --dry-run

# Apply changes
./target/release/add-ignore-attr --input slow-tests.json --apply

# Custom crates directory
./target/release/add-ignore-attr --input slow-tests.json --dry-run --crates-dir ../crates
```

## Input Format

The tool expects a JSON file with the following structure:

```json
{
  "slow_tests": [
    {
      "package": "oxigdal-core",
      "test_name": "test_large_allocation",
      "duration_secs": 12.5
    },
    {
      "package": "oxigdal-algorithms",
      "test_name": "test_complex_topology",
      "duration_secs": 8.3
    }
  ]
}
```

## How It Works

1. Reads the slow tests JSON file
2. Groups tests by package
3. For each package, finds all Rust test files in:
   - `crates/{package}/tests/*.rs`
   - `crates/{package}/src/**/*.rs`
4. Parses each file into an AST using `syn`
5. Uses `VisitMut` to find test functions matching the slow test names
6. Adds `#[ignore]` attribute if not already present
7. Formats the modified AST back to source code using `prettyplease`
8. Writes the changes (in apply mode) or displays preview (in dry-run mode)

## Implementation Details

- **AST Parsing**: Uses `syn::parse_file()` to parse Rust source into an AST
- **Visitor Pattern**: Implements `VisitMut` trait to traverse and modify function items
- **Test Detection**: Looks for `#[test]` and `#[tokio::test]` attributes
- **Safe Modification**: Only adds `#[ignore]` if not already present
- **Code Formatting**: Uses `prettyplease::unparse()` to format modified code

## Dependencies

- `syn` (2.0) - AST parsing and manipulation
- `quote` (1.0) - Procedural macro helpers
- `serde` (1.0) - JSON deserialization
- `clap` (4.5) - Command-line argument parsing
- `prettyplease` (0.2) - Code formatting
- `walkdir` (2.5) - Recursive directory traversal

## Example Output

```
Loaded 3 slow tests from slow-tests.json

Processing 3 packages...

Package: oxigdal-gpu (1 slow tests)
  Found 22 test files
  [+] Added #[ignore] to test: test_compute_heavy_operation in crates/oxigdal-gpu/tests/gpu_test.rs

Package: oxigdal-core (1 slow tests)
  Found 27 test files

Package: oxigdal-algorithms (1 slow tests)
  Found 106 test files

Summary:
  Total tests modified: 1
  Mode: DRY RUN (no files were changed)

Run with --apply to actually modify the files.
```
