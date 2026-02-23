#!/usr/bin/env python3
"""
Report generator for test analysis.

This module provides functionality to generate both human-readable markdown
reports and machine-readable JSON reports from test execution data.
"""

import json
from collections import defaultdict
from pathlib import Path
from typing import Callable, Dict, List, Optional, Tuple

from .nextest_parser import TestEvent, TestStatus


def estimate_test_location(package: str, test_name: str) -> str:
    """
    Estimate the file location of a test based on naming patterns.

    Args:
        package: The package name
        test_name: The full test name (may include module path)

    Returns:
        Estimated file path relative to project root

    Example:
        >>> estimate_test_location('oxigdal-core', 'memory::tests::test_allocator')
        'crates/oxigdal-core/src/memory.rs'
    """
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


class ReportGenerator:
    """
    Generator for test analysis reports.

    Supports both markdown and JSON output formats with customizable
    templates and filtering.
    """

    def __init__(self, report_title: str = "Test Analysis Report"):
        """
        Initialize the report generator.

        Args:
            report_title: Title to use in generated reports
        """
        self.report_title = report_title

    def generate_markdown(
        self,
        test_events: List[TestEvent],
        output_path: Path,
        grouping_key: str = 'package',
        threshold_label: Optional[str] = None,
        recommendations: Optional[List[str]] = None,
        sort_by: str = 'duration'
    ) -> None:
        """
        Generate a human-readable markdown report.

        Args:
            test_events: List of test events to report on
            output_path: Path to write the markdown file
            grouping_key: Key to group tests by ('package' or 'status')
            threshold_label: Optional label describing the filtering threshold
            recommendations: Optional list of recommendation strings
            sort_by: Sort key for tests ('duration', 'name', or 'status')

        Example:
            >>> generator = ReportGenerator("Slow Test Report")
            >>> generator.generate_markdown(slow_tests, Path('report.md'),
            ...                             threshold_label=">60s")
        """
        with open(output_path, 'w') as f:
            # Header
            f.write(f"# {self.report_title}\n\n")

            if threshold_label:
                f.write(f"{threshold_label}\n\n")

            f.write(f"**Total tests found:** {len(test_events)}\n\n")

            # Group tests
            grouped = self._group_tests(test_events, grouping_key)

            # Sort groups by total metric (duration by default)
            sorted_groups = self._sort_groups(grouped, sort_by)

            # Generate sections for each group
            for group_name, group_total, tests in sorted_groups:
                self._write_markdown_section(f, group_name, group_total, tests, sort_by)

            # Summary section
            self._write_markdown_summary(f, test_events, recommendations)

    def generate_json(
        self,
        test_events: List[TestEvent],
        output_path: Path,
        grouping_key: str = 'package',
        metadata: Optional[Dict] = None
    ) -> None:
        """
        Generate a machine-readable JSON report.

        Args:
            test_events: List of test events to report on
            output_path: Path to write the JSON file
            grouping_key: Key to group tests by ('package' or 'status')
            metadata: Optional metadata to include in the report

        Example:
            >>> generator = ReportGenerator("Failed Test Report")
            >>> generator.generate_json(failed_tests, Path('failures.json'),
            ...                         metadata={'threshold_seconds': 60})
        """
        # Group tests
        grouped = self._group_tests(test_events, grouping_key)

        # Build report structure
        report = {
            'title': self.report_title,
            'total_tests': len(test_events),
            'groups': {}
        }

        # Add metadata if provided
        if metadata:
            report['metadata'] = metadata

        # Populate groups
        for group_name, tests in grouped.items():
            total_duration = sum(t.duration for t in tests)

            report['groups'][group_name] = {
                'total_duration_seconds': round(total_duration, 2),
                'test_count': len(tests),
                'tests': [
                    {
                        **t.to_dict(),
                        'estimated_location': estimate_test_location(t.package, t.name)
                    }
                    for t in sorted(tests, key=lambda x: x.duration, reverse=True)
                ]
            }

        with open(output_path, 'w') as f:
            json.dump(report, f, indent=2)

    def _group_tests(
        self,
        test_events: List[TestEvent],
        grouping_key: str
    ) -> Dict[str, List[TestEvent]]:
        """Group test events by the specified key."""
        grouped = defaultdict(list)

        for event in test_events:
            if grouping_key == 'package':
                key = event.package
            elif grouping_key == 'status':
                key = event.status.value
            else:
                key = 'all'

            grouped[key].append(event)

        return dict(grouped)

    def _sort_groups(
        self,
        grouped: Dict[str, List[TestEvent]],
        sort_by: str
    ) -> List[Tuple[str, float, List[TestEvent]]]:
        """Sort groups by total metric (duration, count, etc.)."""
        group_metrics = []

        for group_name, tests in grouped.items():
            if sort_by == 'duration':
                metric = sum(t.duration for t in tests)
            elif sort_by == 'count':
                metric = len(tests)
            else:
                metric = 0.0

            group_metrics.append((group_name, metric, tests))

        return sorted(group_metrics, key=lambda x: x[1], reverse=True)

    def _write_markdown_section(
        self,
        f,
        group_name: str,
        group_total: float,
        tests: List[TestEvent],
        sort_by: str
    ) -> None:
        """Write a markdown section for a test group."""
        f.write(f"## Group: `{group_name}`\n\n")
        f.write(f"**Total duration:** {group_total:.2f}s\n")
        f.write(f"**Test count:** {len(tests)}\n\n")

        # Sort tests
        if sort_by == 'duration':
            sorted_tests = sorted(tests, key=lambda x: x.duration, reverse=True)
        elif sort_by == 'name':
            sorted_tests = sorted(tests, key=lambda x: x.name)
        else:
            sorted_tests = tests

        # Table header
        f.write("| Test Name | Duration | Status | Estimated Location |\n")
        f.write("|-----------|----------|--------|--------------------|\n")

        # Table rows
        for test in sorted_tests:
            name = test.name
            duration = f"{test.duration:.2f}s"
            status = test.status.value
            location = estimate_test_location(test.package, name)

            # Truncate long test names for readability
            if len(name) > 60:
                name = name[:57] + "..."

            f.write(f"| `{name}` | {duration} | {status} | `{location}` |\n")

        f.write("\n")

    def _write_markdown_summary(
        self,
        f,
        test_events: List[TestEvent],
        recommendations: Optional[List[str]]
    ) -> None:
        """Write the summary section of the markdown report."""
        f.write("## Summary\n\n")

        # Statistics
        total_duration = sum(t.duration for t in test_events)
        avg_duration = total_duration / len(test_events) if test_events else 0

        f.write("### Statistics\n\n")
        f.write(f"- **Total tests:** {len(test_events)}\n")
        f.write(f"- **Total duration:** {total_duration:.2f}s\n")
        f.write(f"- **Average duration:** {avg_duration:.2f}s\n\n")

        # Status breakdown
        status_counts = defaultdict(int)
        for event in test_events:
            status_counts[event.status.value] += 1

        if status_counts:
            f.write("### Status Breakdown\n\n")
            for status, count in sorted(status_counts.items()):
                f.write(f"- **{status}:** {count}\n")
            f.write("\n")

        # Recommendations
        if recommendations:
            f.write("### Recommendations\n\n")
            for i, rec in enumerate(recommendations, start=1):
                f.write(f"{i}. {rec}\n")
            f.write("\n")
