#!/usr/bin/env python3
"""
Analyze slow tests from nextest JSON output.

This script parses nextest JSON output files, extracts tests taking >60 seconds,
and generates both human-readable markdown and machine-readable JSON reports.

Usage:
    python scripts/analyze-slow-tests.py [input_dir] [output_dir]

Arguments:
    input_dir   Directory containing nextest JSON files (default: target/slow-test-detection)
    output_dir  Directory to write reports (default: target/slow-test-detection)

Output:
    - report.md: Human-readable markdown report grouped by package
    - slow-tests.json: Machine-readable JSON with test metadata
"""

import json
import sys
from pathlib import Path
from typing import Dict, List, Tuple
from collections import defaultdict

# Threshold for slow tests (seconds)
SLOW_THRESHOLD = 60.0

def parse_nextest_json(json_path: Path) -> List[Dict]:
    """Parse nextest JSON output and extract test execution data."""
    slow_tests = []

    with open(json_path, 'r') as f:
        for line in f:
            line = line.strip()
            if not line:
                continue

            try:
                event = json.loads(line)

                # Look for test-finished events
                if event.get('type') == 'test-finished':
                    test_name = event.get('name', '')
                    package = event.get('package-id', '').split(' ')[0] if event.get('package-id') else 'unknown'

                    # Extract duration
                    exec_time = event.get('exec-time')
                    if exec_time is not None:
                        duration = exec_time.get('secs', 0) + exec_time.get('nanos', 0) / 1e9

                        if duration >= SLOW_THRESHOLD:
                            slow_tests.append({
                                'name': test_name,
                                'package': package,
                                'duration': duration,
                                'status': event.get('status', 'unknown')
                            })
            except json.JSONDecodeError:
                # Skip invalid JSON lines
                continue

    return slow_tests

def estimate_test_location(package: str, test_name: str) -> str:
    """Estimate the file location of a test based on naming patterns."""
    # Common patterns:
    # - integration tests: tests/*.rs
    # - unit tests: src/*.rs
    # - benchmark tests: benches/*.rs

    if '::' in test_name:
        parts = test_name.split('::')
        if len(parts) >= 2:
            # Try to extract module path
            module = parts[0]

            # Common test directories
            if 'integration' in test_name.lower() or 'it_' in test_name:
                return f"crates/{package}/tests/{module}.rs"
            elif 'bench' in test_name.lower():
                return f"crates/{package}/benches/{module}.rs"
            else:
                return f"crates/{package}/src/{module}.rs"

    # Fallback
    return f"crates/{package}/tests/ or src/"

def generate_markdown_report(tests_by_package: Dict[str, List[Dict]], output_path: Path):
    """Generate human-readable markdown report."""
    with open(output_path, 'w') as f:
        f.write("# Slow Test Analysis Report\n\n")
        f.write(f"Tests taking longer than {SLOW_THRESHOLD} seconds.\n\n")
        f.write(f"**Total slow tests found:** {sum(len(tests) for tests in tests_by_package.values())}\n\n")

        # Sort packages by total time
        package_times = []
        for pkg, tests in tests_by_package.items():
            total_time = sum(t['duration'] for t in tests)
            package_times.append((pkg, total_time, tests))

        package_times.sort(key=lambda x: x[1], reverse=True)

        for pkg, total_time, tests in package_times:
            f.write(f"## Package: `{pkg}`\n\n")
            f.write(f"**Total time:** {total_time:.2f}s\n")
            f.write(f"**Slow tests:** {len(tests)}\n\n")

            # Sort tests by duration
            sorted_tests = sorted(tests, key=lambda x: x['duration'], reverse=True)

            f.write("| Test Name | Duration | Status | Estimated Location |\n")
            f.write("|-----------|----------|--------|--------------------|\n")

            for test in sorted_tests:
                name = test['name']
                duration = f"{test['duration']:.2f}s"
                status = test['status']
                location = estimate_test_location(pkg, name)

                # Truncate long test names for readability
                if len(name) > 60:
                    name = name[:57] + "..."

                f.write(f"| `{name}` | {duration} | {status} | `{location}` |\n")

            f.write("\n")

        # Summary section
        f.write("## Summary\n\n")
        f.write("### Recommendations\n\n")
        f.write("1. **Parallelize I/O operations** in integration tests\n")
        f.write("2. **Use smaller datasets** for test fixtures\n")
        f.write("3. **Mock external dependencies** where possible\n")
        f.write("4. **Split large tests** into smaller, focused units\n")
        f.write("5. **Use `#[ignore]`** for expensive tests that don't need to run every time\n")
        f.write("6. **Consider benchmarks** for performance-critical code instead of long-running tests\n\n")

def generate_json_report(tests_by_package: Dict[str, List[Dict]], output_path: Path):
    """Generate machine-readable JSON report."""
    report = {
        'threshold_seconds': SLOW_THRESHOLD,
        'total_slow_tests': sum(len(tests) for tests in tests_by_package.values()),
        'packages': {}
    }

    for pkg, tests in tests_by_package.items():
        total_time = sum(t['duration'] for t in tests)

        report['packages'][pkg] = {
            'total_time_seconds': round(total_time, 2),
            'slow_test_count': len(tests),
            'tests': [
                {
                    'name': t['name'],
                    'duration_seconds': round(t['duration'], 2),
                    'status': t['status'],
                    'estimated_location': estimate_test_location(pkg, t['name'])
                }
                for t in sorted(tests, key=lambda x: x['duration'], reverse=True)
            ]
        }

    with open(output_path, 'w') as f:
        json.dump(report, f, indent=2)

def main():
    # Parse arguments
    input_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else Path('target/slow-test-detection')
    output_dir = Path(sys.argv[2]) if len(sys.argv) > 2 else Path('target/slow-test-detection')

    # Create output directory
    output_dir.mkdir(parents=True, exist_ok=True)

    # Find all JSON files
    json_files = list(input_dir.glob('*.json'))

    if not json_files:
        print(f"No JSON files found in {input_dir}", file=sys.stderr)
        sys.exit(1)

    print(f"Found {len(json_files)} JSON file(s) to analyze")

    # Collect all slow tests
    all_slow_tests = []
    for json_file in json_files:
        print(f"Processing {json_file.name}...")
        slow_tests = parse_nextest_json(json_file)
        all_slow_tests.extend(slow_tests)

    if not all_slow_tests:
        print(f"\nNo tests found taking longer than {SLOW_THRESHOLD} seconds!")
        print("This is good news - all tests are fast! 🎉")
        return

    # Group by package
    tests_by_package = defaultdict(list)
    for test in all_slow_tests:
        tests_by_package[test['package']].append(test)

    # Generate reports
    markdown_path = output_dir / 'report.md'
    json_path = output_dir / 'slow-tests.json'

    print(f"\nGenerating reports...")
    generate_markdown_report(tests_by_package, markdown_path)
    generate_json_report(tests_by_package, json_path)

    print(f"\n✅ Reports generated:")
    print(f"   - Markdown: {markdown_path}")
    print(f"   - JSON: {json_path}")
    print(f"\n📊 Summary:")
    print(f"   - Total slow tests: {len(all_slow_tests)}")
    print(f"   - Affected packages: {len(tests_by_package)}")
    print(f"   - Threshold: >{SLOW_THRESHOLD}s")

if __name__ == '__main__':
    main()
