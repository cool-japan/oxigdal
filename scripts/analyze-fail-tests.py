#!/usr/bin/env python3
"""
Fail Test Analyzer - Classifies test failures using taxonomy

This script analyzes nextest NDJSON output from fail-test-detection runs,
classifies each failure using pattern matching against the failure taxonomy,
and generates both machine-readable JSON and human-readable markdown reports.

Usage:
    python analyze-fail-tests.py [--input-dir DIR] [--output-dir DIR]

Input:
    - Nextest NDJSON files from target/fail-test-detection/
    - Failure taxonomy YAML (scripts/failure-taxonomy.yaml)

Output:
    - fail-tests.json - Machine-readable classification for auto-fix tool
    - fail-report.md - Human-readable categorized report
"""

import json
import re
import sys
from collections import defaultdict
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple

import yaml

# Import from shared library
sys.path.insert(0, str(Path(__file__).parent))
from lib.nextest_parser import TestStatus, parse_nextest_ndjson
from lib.report_generator import estimate_test_location


@dataclass
class FailureClassification:
    """Result of classifying a single test failure."""
    test_name: str
    package: str
    file_path: str
    duration: float
    category: str
    subcategory: str
    confidence: str  # HIGH, MEDIUM, LOW, NONE
    auto_fix: str
    priority: str  # P1_CRITICAL, P2_HIGH, etc.
    error_context: str
    recommended_action: str
    raw_output: str  # Full stdout/stderr for reference


class FailureClassifier:
    """
    Classifies test failures using pattern matching against taxonomy.

    The classifier loads the failure taxonomy YAML and matches error patterns
    against test output (stdout/stderr) to determine the category, subcategory,
    and recommended fix strategy.
    """

    def __init__(self, taxonomy_path: Path):
        """
        Initialize the classifier with taxonomy data.

        Args:
            taxonomy_path: Path to failure-taxonomy.yaml
        """
        with open(taxonomy_path, 'r') as f:
            self.taxonomy = yaml.safe_load(f)

        # Build pattern index for fast lookup
        self.pattern_index = self._build_pattern_index()

        # Extract metadata
        self.fix_strategies = self.taxonomy.get('FIX_STRATEGIES', {})
        self.priority_levels = self.taxonomy.get('PRIORITY_LEVELS', {})

    def _build_pattern_index(self) -> List[Tuple[str, str, Dict]]:
        """
        Build an index of all patterns for matching.

        Returns:
            List of (category, subcategory, pattern_config) tuples
        """
        pattern_index = []

        # Categories to process (exclude metadata sections)
        category_keys = [
            'HARDWARE_UNAVAILABLE',
            'EXTERNAL_DEPENDENCY',
            'ASYNC_RUNTIME',
            'ASSERTION_FAILURE',
            'RESOURCE_NOT_FOUND',
            'COMPILATION_ERROR',
            'SPECIAL_PATTERNS'
        ]

        for category_key in category_keys:
            if category_key not in self.taxonomy:
                continue

            category = self.taxonomy[category_key]

            for subcategory_key, subcategory_data in category.items():
                if subcategory_key == 'description':
                    continue

                if not isinstance(subcategory_data, dict):
                    continue

                pattern_index.append((category_key, subcategory_key, subcategory_data))

        return pattern_index

    def classify(
        self,
        test_name: str,
        package: str,
        duration: float,
        output: str
    ) -> FailureClassification:
        """
        Classify a test failure based on its output.

        Args:
            test_name: Full test name
            package: Package name
            duration: Test duration in seconds
            output: Combined stdout/stderr from test

        Returns:
            FailureClassification with category, confidence, and fix strategy
        """
        # Try to match patterns against output
        matches = []

        # Priority order for categories (process in order to avoid mis-classification)
        # Timeouts should be checked before other errors
        priority_categories = [
            'ASYNC_RUNTIME',  # Check timeouts first
            'HARDWARE_UNAVAILABLE',
            'EXTERNAL_DEPENDENCY',
            'COMPILATION_ERROR',
            'ASSERTION_FAILURE',
            'RESOURCE_NOT_FOUND',
            'SPECIAL_PATTERNS'
        ]

        # Reorder pattern_index by priority
        ordered_patterns = []
        for cat in priority_categories:
            for category, subcategory, config in self.pattern_index:
                if category == cat:
                    ordered_patterns.append((category, subcategory, config))

        for category, subcategory, config in ordered_patterns:
            patterns = config.get('patterns', [])
            confidence = config.get('confidence', 'MEDIUM')
            auto_fix = config.get('auto_fix', 'none')
            description = config.get('description', '')

            # Check if any pattern matches
            for pattern in patterns:
                try:
                    if re.search(pattern, output, re.IGNORECASE | re.MULTILINE):
                        # Calculate match score based on pattern specificity
                        score = self._calculate_match_score(pattern, confidence, category)
                        matches.append({
                            'category': category,
                            'subcategory': subcategory,
                            'confidence': confidence,
                            'auto_fix': auto_fix,
                            'description': description,
                            'score': score,
                            'pattern': pattern
                        })
                        break  # One match per subcategory is enough
                except re.error:
                    # Invalid regex pattern, skip
                    continue

        # Pick the best match (highest score)
        if matches:
            best_match = max(matches, key=lambda m: m['score'])
            category = best_match['category']
            subcategory = best_match['subcategory']
            confidence = best_match['confidence']
            auto_fix = best_match['auto_fix']
            description = best_match['description']
        else:
            # No match - unknown failure
            category = 'UNKNOWN'
            subcategory = 'unclassified'
            confidence = 'NONE'
            auto_fix = 'none'
            description = 'Failure pattern not recognized'

        # Determine priority based on auto_fix strategy
        priority = self._determine_priority(auto_fix)

        # Extract error context (first relevant error message)
        error_context = self._extract_error_context(output)

        # Generate recommended action
        recommended_action = self._generate_recommendation(
            category, subcategory, auto_fix, description
        )

        # Estimate file location
        file_path = estimate_test_location(package, test_name)

        return FailureClassification(
            test_name=test_name,
            package=package,
            file_path=file_path,
            duration=duration,
            category=category,
            subcategory=subcategory,
            confidence=confidence,
            auto_fix=auto_fix,
            priority=priority,
            error_context=error_context,
            recommended_action=recommended_action,
            raw_output=output
        )

    def _calculate_match_score(self, pattern: str, confidence: str, category: str) -> float:
        """
        Calculate match score based on pattern specificity and confidence.

        More specific patterns (longer, more complex) get higher scores.
        Higher confidence levels also increase the score.

        Returns:
            Float score (higher is better)
        """
        # Base score from confidence level
        confidence_scores = {
            'HIGH': 100.0,
            'MEDIUM': 50.0,
            'LOW': 25.0,
            'NONE': 0.0
        }
        score = confidence_scores.get(confidence, 0.0)

        # Bonus for high-priority categories (timeouts should win over generic errors)
        if category == 'ASYNC_RUNTIME':
            score += 50.0

        # Add score based on pattern complexity
        # Longer patterns are usually more specific
        score += len(pattern) * 0.1

        # Patterns with word boundaries are more specific
        if r'\b' in pattern:
            score += 10.0

        # Patterns with specific values (numbers, paths) are more specific
        if any(char in pattern for char in ['\\d', '\\w', '\\s']):
            score += 5.0

        return score

    def _determine_priority(self, auto_fix: str) -> str:
        """
        Determine priority level based on fix strategy.

        Args:
            auto_fix: Fix strategy name

        Returns:
            Priority level (P1_CRITICAL, P2_HIGH, etc.)
        """
        for priority, config in self.priority_levels.items():
            strategies = config.get('strategies', [])
            if auto_fix in strategies:
                return priority

        # Default to manual if not found
        return 'P5_MANUAL'

    def _extract_error_context(self, output: str, max_length: int = 200) -> str:
        """
        Extract the most relevant error context from output.

        Looks for common error indicators and extracts surrounding context.

        Args:
            output: Full test output
            max_length: Maximum length of context string

        Returns:
            Error context string (truncated if needed)
        """
        # Look for common error indicators
        error_indicators = [
            r'panicked at',
            r'assertion.*failed',
            r'Error:',
            r'FAIL.*\[',
            r'test timed out',
            r'wgpu error:',
            r'Connection refused',
            r'not found',
            r'TIMEOUT.*\['
        ]

        for indicator in error_indicators:
            match = re.search(indicator, output, re.IGNORECASE | re.MULTILINE)
            if match:
                # Extract context around the match
                start = max(0, match.start() - 50)
                end = min(len(output), match.end() + max_length)
                context = output[start:end].strip()

                # Clean up whitespace
                context = ' '.join(context.split())

                # Truncate if too long
                if len(context) > max_length:
                    context = context[:max_length - 3] + '...'

                return context

        # Fallback: return first N characters
        context = output.strip()[:max_length]
        if len(output) > max_length:
            context += '...'

        return context

    def _generate_recommendation(
        self,
        category: str,
        subcategory: str,
        auto_fix: str,
        description: str
    ) -> str:
        """
        Generate a human-readable recommended action.

        Args:
            category: Failure category
            subcategory: Failure subcategory
            auto_fix: Fix strategy
            description: Subcategory description

        Returns:
            Recommended action string
        """
        if auto_fix == 'none':
            return f"Manual review required: {description}"

        fix_config = self.fix_strategies.get(auto_fix, {})
        fix_description = fix_config.get('description', auto_fix)

        return f"{fix_description} ({category}/{subcategory})"


def load_nextest_failures(input_dir: Path) -> List[Tuple[str, str, float, str]]:
    """
    Load all test failures from nextest NDJSON files.

    Args:
        input_dir: Directory containing nextest NDJSON and stderr files

    Returns:
        List of (test_name, package, duration, combined_output) tuples
    """
    failures = []
    seen_tests = set()  # Deduplicate across multiple runs

    # Find all NDJSON files
    ndjson_files = sorted(input_dir.glob('*.ndjson'))

    for ndjson_file in ndjson_files:
        # Parse NDJSON for failed/timed-out tests
        test_events = parse_nextest_ndjson(
            ndjson_file,
            status_filter=[TestStatus.FAILED]
        )

        # Load corresponding stderr file
        stderr_file = ndjson_file.with_suffix('.stderr')
        if stderr_file.exists():
            with open(stderr_file, 'r') as f:
                stderr_content = f.read()
        else:
            stderr_content = ""

        # Extract per-test output from stderr using test names
        for event in test_events:
            test_key = (event.package, event.name)
            if test_key in seen_tests:
                continue  # Skip duplicate

            seen_tests.add(test_key)

            # Try to extract test-specific output from stderr
            test_output = extract_test_output_from_stderr(
                stderr_content,
                event.name,
                event.package
            )

            failures.append((
                event.name,
                event.package,
                event.duration,
                test_output
            ))

    return failures


def extract_test_output_from_stderr(
    stderr_content: str,
    test_name: str,
    package: str
) -> str:
    """
    Extract test-specific output from combined stderr.

    Looks for sections marked by test name and extracts surrounding context.

    Args:
        stderr_content: Full stderr content
        test_name: Test name to find
        package: Package name

    Returns:
        Test-specific output or full stderr if not found
    """
    # Look for patterns like:
    # "FAIL [   2.074s] (62/90) oxigdal-gpu::gpu_integration_test memory_tests::test_memory_pool_expansion"
    # or
    # "TIMEOUT [ 120.006s] (84/90) oxigdal-gpu::gpu_test test_element_wise_operations"

    # Build search patterns
    patterns = [
        rf"(?:FAIL|TIMEOUT).*{re.escape(package)}.*{re.escape(test_name)}",
        rf"{re.escape(test_name)}.*panicked",
        rf"thread.*{re.escape(test_name)}"
    ]

    for pattern in patterns:
        match = re.search(pattern, stderr_content, re.IGNORECASE)
        if match:
            # Extract context around match (500 chars before, 1000 after)
            start = max(0, match.start() - 500)
            end = min(len(stderr_content), match.end() + 1000)
            return stderr_content[start:end]

    # Fallback: return full stderr (will be truncated in error context extraction)
    return stderr_content


def generate_fail_tests_json(
    classifications: List[FailureClassification],
    output_path: Path
) -> None:
    """
    Generate machine-readable JSON report for auto-fix tool.

    Args:
        classifications: List of failure classifications
        output_path: Path to write fail-tests.json
    """
    # Group by confidence
    by_confidence = defaultdict(int)
    for c in classifications:
        by_confidence[c.confidence] += 1

    # Build failures list
    failures = []
    for c in classifications:
        failures.append({
            'test_name': c.test_name,
            'package': c.package,
            'file_path': c.file_path,
            'duration': round(c.duration, 2),
            'category': c.category,
            'subcategory': c.subcategory,
            'confidence': c.confidence,
            'auto_fix': c.auto_fix,
            'priority': c.priority,
            'error_context': c.error_context,
            'recommended_action': c.recommended_action
        })

    # Build report structure
    report = {
        'timestamp': datetime.now(timezone.utc).isoformat(),
        'total_failures': len(classifications),
        'by_confidence': dict(by_confidence),
        'failures': failures
    }

    with open(output_path, 'w') as f:
        json.dump(report, f, indent=2)

    print(f"Generated JSON report: {output_path}")


def generate_fail_report_md(
    classifications: List[FailureClassification],
    output_path: Path
) -> None:
    """
    Generate human-readable markdown report.

    Args:
        classifications: List of failure classifications
        output_path: Path to write fail-report.md
    """
    with open(output_path, 'w') as f:
        # Header
        f.write("# Test Failure Analysis Report\n\n")
        f.write(f"**Generated:** {datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M:%S')} UTC\n\n")

        # Executive summary
        f.write("## Executive Summary\n\n")
        f.write(f"**Total failures:** {len(classifications)}\n\n")

        # By category
        by_category = defaultdict(int)
        by_confidence = defaultdict(int)
        by_priority = defaultdict(int)
        auto_fixable = 0

        for c in classifications:
            by_category[c.category] += 1
            by_confidence[c.confidence] += 1
            by_priority[c.priority] += 1
            if c.auto_fix != 'none':
                auto_fixable += 1

        f.write("### By Category\n\n")
        for category, count in sorted(by_category.items(), key=lambda x: x[1], reverse=True):
            f.write(f"- **{category}:** {count}\n")
        f.write("\n")

        f.write("### By Confidence\n\n")
        for confidence, count in sorted(by_confidence.items(), key=lambda x: x[1], reverse=True):
            f.write(f"- **{confidence}:** {count}\n")
        f.write("\n")

        f.write("### By Priority\n\n")
        for priority, count in sorted(by_priority.items()):
            f.write(f"- **{priority}:** {count}\n")
        f.write("\n")

        f.write(f"**Auto-fixable:** {auto_fixable}/{len(classifications)} "
                f"({100*auto_fixable/len(classifications):.1f}%)\n\n")

        # Group failures by category and subcategory
        f.write("## Failures by Category\n\n")

        grouped: Dict[str, Dict[str, List[FailureClassification]]] = defaultdict(lambda: defaultdict(list))
        for c in classifications:
            grouped[c.category][c.subcategory].append(c)

        for category in sorted(grouped.keys()):
            f.write(f"### {category}\n\n")

            subcategories = grouped[category]
            for subcategory in sorted(subcategories.keys()):
                failures = subcategories[subcategory]
                f.write(f"#### {subcategory} ({len(failures)} failures)\n\n")

                # Table of failures
                f.write("| Test Name | Package | Duration | Confidence | Priority | Error Context |\n")
                f.write("|-----------|---------|----------|------------|----------|---------------|\n")

                for failure in sorted(failures, key=lambda x: x.duration, reverse=True):
                    name = failure.test_name
                    if len(name) > 40:
                        name = name[:37] + "..."

                    context = failure.error_context
                    if len(context) > 60:
                        context = context[:57] + "..."

                    f.write(f"| `{name}` | {failure.package} | "
                            f"{failure.duration:.1f}s | {failure.confidence} | "
                            f"{failure.priority} | {context} |\n")

                f.write("\n")

                # Recommended actions for this subcategory
                if failures:
                    action = failures[0].recommended_action
                    f.write(f"**Recommended action:** {action}\n\n")

        # Action items section
        f.write("## Action Items\n\n")

        # Group by priority
        by_priority_list: Dict[str, List[FailureClassification]] = defaultdict(list)
        for c in classifications:
            by_priority_list[c.priority].append(c)

        priority_order = ['P1_CRITICAL', 'P2_HIGH', 'P3_MEDIUM', 'P4_LOW', 'P5_MANUAL']

        for priority in priority_order:
            if priority not in by_priority_list:
                continue

            failures = by_priority_list[priority]
            f.write(f"### {priority} ({len(failures)} items)\n\n")

            # Group by auto_fix strategy
            by_strategy = defaultdict(list)
            for failure in failures:
                by_strategy[failure.auto_fix].append(failure)

            for strategy, strategy_failures in sorted(by_strategy.items()):
                f.write(f"**Strategy: {strategy}** ({len(strategy_failures)} tests)\n\n")

                for failure in strategy_failures[:10]:  # Limit to first 10
                    f.write(f"- `{failure.test_name}` ({failure.package})\n")
                    f.write(f"  - Location: `{failure.file_path}`\n")
                    f.write(f"  - {failure.recommended_action}\n")

                if len(strategy_failures) > 10:
                    f.write(f"- ... and {len(strategy_failures) - 10} more\n")

                f.write("\n")

    print(f"Generated markdown report: {output_path}")


def print_usage():
    """Print usage information."""
    print("""
Usage: analyze-fail-tests.py [OPTIONS]

Analyze nextest failure data and classify failures using taxonomy.

Options:
    --input-dir DIR     Directory containing nextest NDJSON files
                        (default: target/fail-test-detection)
    --output-dir DIR    Directory to write reports
                        (default: target/fail-test-detection)
    --help, -h          Show this help message

Output:
    fail-tests.json     Machine-readable JSON for auto-fix tool
    fail-report.md      Human-readable markdown report

Example:
    python analyze-fail-tests.py
    python analyze-fail-tests.py --input-dir /path/to/failures
""")


def main():
    """Main entry point for the analyzer."""
    # Check for help flag
    if '--help' in sys.argv or '-h' in sys.argv:
        print_usage()
        sys.exit(0)

    # Default paths
    script_dir = Path(__file__).parent
    project_root = script_dir.parent

    input_dir = project_root / 'target' / 'fail-test-detection'
    output_dir = project_root / 'target' / 'fail-test-detection'
    taxonomy_path = script_dir / 'failure-taxonomy.yaml'

    # Parse command line arguments (simple version)
    if '--input-dir' in sys.argv:
        idx = sys.argv.index('--input-dir')
        if idx + 1 < len(sys.argv):
            input_dir = Path(sys.argv[idx + 1])

    if '--output-dir' in sys.argv:
        idx = sys.argv.index('--output-dir')
        if idx + 1 < len(sys.argv):
            output_dir = Path(sys.argv[idx + 1])

    # Validate paths
    if not input_dir.exists():
        print(f"Error: Input directory not found: {input_dir}", file=sys.stderr)
        sys.exit(1)

    if not taxonomy_path.exists():
        print(f"Error: Taxonomy file not found: {taxonomy_path}", file=sys.stderr)
        sys.exit(1)

    output_dir.mkdir(parents=True, exist_ok=True)

    print(f"Loading failures from: {input_dir}")
    print(f"Using taxonomy: {taxonomy_path}")

    # Load classifier
    classifier = FailureClassifier(taxonomy_path)

    # Load failures
    failures = load_nextest_failures(input_dir)
    print(f"Found {len(failures)} test failures")

    if not failures:
        print("No failures to analyze. Exiting.")
        sys.exit(0)

    # Classify each failure
    classifications = []
    for test_name, package, duration, output in failures:
        classification = classifier.classify(test_name, package, duration, output)
        classifications.append(classification)
        print(f"  - {package}::{test_name} -> {classification.category}/{classification.subcategory} "
              f"(confidence: {classification.confidence})")

    # Generate reports
    json_path = output_dir / 'fail-tests.json'
    md_path = output_dir / 'fail-report.md'

    generate_fail_tests_json(classifications, json_path)
    generate_fail_report_md(classifications, md_path)

    # Print summary statistics
    print(f"\n{'='*60}")
    print("ANALYSIS COMPLETE")
    print(f"{'='*60}")

    # Count by category
    by_category = defaultdict(int)
    by_priority = defaultdict(int)
    auto_fixable = 0

    for c in classifications:
        by_category[c.category] += 1
        by_priority[c.priority] += 1
        if c.auto_fix != 'none':
            auto_fixable += 1

    print(f"\nTotal failures analyzed: {len(classifications)}")
    print(f"Auto-fixable: {auto_fixable}/{len(classifications)} ({100*auto_fixable/len(classifications):.1f}%)")

    print("\nBy category:")
    for category, count in sorted(by_category.items(), key=lambda x: x[1], reverse=True):
        print(f"  - {category}: {count}")

    print("\nBy priority:")
    for priority in ['P1_CRITICAL', 'P2_HIGH', 'P3_MEDIUM', 'P4_LOW', 'P5_MANUAL']:
        if priority in by_priority:
            print(f"  - {priority}: {by_priority[priority]}")

    print(f"\nReports generated:")
    print(f"  - JSON: {json_path}")
    print(f"  - Markdown: {md_path}")
    print(f"\n{'='*60}")


if __name__ == '__main__':
    main()
