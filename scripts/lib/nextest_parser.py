#!/usr/bin/env python3
"""
Nextest NDJSON output parser.

This module provides functionality to parse nextest's newline-delimited JSON
output format, extracting test execution metadata including names, packages,
durations, and statuses.
"""

import json
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Dict, List, Optional, Union


class TestStatus(Enum):
    """Test execution status."""
    PASSED = "passed"
    FAILED = "failed"
    SKIPPED = "skipped"
    IGNORED = "ignored"
    UNKNOWN = "unknown"

    @classmethod
    def from_string(cls, status: str) -> 'TestStatus':
        """Convert string status to enum, defaulting to UNKNOWN."""
        try:
            return cls(status.lower())
        except ValueError:
            return cls.UNKNOWN


@dataclass
class TestEvent:
    """Structured representation of a test execution event."""
    name: str
    package: str
    duration: float  # seconds
    status: TestStatus
    retry_data: Optional[Dict] = None  # For retries/flaky test detection

    def to_dict(self) -> Dict:
        """Convert to dictionary for JSON serialization."""
        result = {
            'name': self.name,
            'package': self.package,
            'duration': round(self.duration, 2),
            'status': self.status.value,
        }
        if self.retry_data:
            result['retry_data'] = self.retry_data
        return result


def parse_nextest_ndjson(
    json_path: Union[Path, str],
    status_filter: Optional[List[TestStatus]] = None,
    duration_threshold: Optional[float] = None
) -> List[TestEvent]:
    """
    Parse nextest NDJSON output and extract test execution data.

    Args:
        json_path: Path to the nextest JSON output file
        status_filter: Optional list of statuses to include (default: all)
        duration_threshold: Optional minimum duration in seconds (default: include all)

    Returns:
        List of TestEvent objects matching the filters

    Example:
        >>> events = parse_nextest_ndjson('nextest-output.json',
        ...                                status_filter=[TestStatus.FAILED])
        >>> failed_tests = [e for e in events if e.status == TestStatus.FAILED]
    """
    json_path = Path(json_path)
    test_events = []

    with open(json_path, 'r') as f:
        for line_num, line in enumerate(f, start=1):
            line = line.strip()
            if not line:
                continue

            try:
                event = json.loads(line)

                # Look for test completion events (event: ok, failed, etc.)
                if event.get('type') == 'test' and event.get('event') in ['ok', 'failed', 'ignored', 'skipped']:
                    test_event = _parse_test_event(event)

                    if test_event is None:
                        continue

                    # Apply filters
                    if status_filter and test_event.status not in status_filter:
                        continue

                    if duration_threshold and test_event.duration < duration_threshold:
                        continue

                    test_events.append(test_event)

            except json.JSONDecodeError:
                # Skip invalid JSON lines silently (many lines are non-JSON output)
                continue
            except Exception as e:
                # Catch any other parsing errors
                print(f"Warning: Error parsing line {line_num}: {e}",
                      file=__import__('sys').stderr)
                continue

    return test_events


def _parse_test_event(event: Dict) -> Optional[TestEvent]:
    """
    Parse a test completion event into a TestEvent object.

    Returns None if required fields are missing.
    """
    # Extract test name (format: "package::binary$module::test_name")
    test_name_full = event.get('name', '')
    if not test_name_full:
        return None

    # Parse the name to extract package and test name
    # Format: "oxigdal-analytics::oxigdal_analytics$change::detection::tests::test_absolute_difference"
    parts = test_name_full.split('::', 1)
    if len(parts) >= 2:
        package = parts[0]
        # Remove the binary name part if present
        test_name = parts[1]
        if '$' in test_name:
            test_name = test_name.split('$', 1)[1]
    else:
        package = 'unknown'
        test_name = test_name_full

    # Extract duration from exec_time (float in seconds)
    exec_time = event.get('exec_time', 0.0)
    duration = float(exec_time) if exec_time is not None else 0.0

    # Extract status from event field
    event_type = event.get('event', 'unknown')
    if event_type == 'ok':
        status = TestStatus.PASSED
    elif event_type == 'failed':
        status = TestStatus.FAILED
    elif event_type == 'ignored':
        status = TestStatus.IGNORED
    elif event_type == 'skipped':
        status = TestStatus.SKIPPED
    else:
        status = TestStatus.UNKNOWN

    # Extract retry information if present
    retry_data = None
    if 'retry_data' in event:
        retry_data = event['retry_data']

    return TestEvent(
        name=test_name,
        package=package,
        duration=duration,
        status=status,
        retry_data=retry_data
    )
