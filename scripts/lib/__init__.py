"""
Shared library modules for OxiGDAL test analysis.

This package provides reusable components for parsing nextest output
and generating reports for both slow test and fail test detection.
"""

from .nextest_parser import (
    parse_nextest_ndjson,
    TestEvent,
    TestStatus,
)

from .report_generator import (
    ReportGenerator,
    estimate_test_location,
)

__all__ = [
    'parse_nextest_ndjson',
    'TestEvent',
    'TestStatus',
    'ReportGenerator',
    'estimate_test_location',
]
