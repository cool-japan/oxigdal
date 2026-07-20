#!/usr/bin/env python3
"""Regression tests for scripts/verify-publish-order.py's pure logic
(topological sort, CRATES array parsing). These do not invoke `cargo`
or touch the network -- they exercise the graph/parsing functions
directly against small synthetic fixtures.

Run with:
    python3 scripts/tests/test_verify_publish_order.py
"""
from __future__ import annotations

import importlib.util
import os
import sys
import unittest

_HERE = os.path.dirname(os.path.abspath(__file__))
_MODULE_PATH = os.path.join(os.path.dirname(_HERE), "verify-publish-order.py")

_spec = importlib.util.spec_from_file_location("verify_publish_order", _MODULE_PATH)
if _spec is None or _spec.loader is None:
    raise RuntimeError(f"could not load module spec from {_MODULE_PATH}")
verify_publish_order = importlib.util.module_from_spec(_spec)
sys.modules["verify_publish_order"] = verify_publish_order
_spec.loader.exec_module(verify_publish_order)

topo_order = verify_publish_order.topo_order
parse_crates_array = verify_publish_order.parse_crates_array


class TopoOrderTests(unittest.TestCase):
    def test_simple_chain_is_ordered_dependency_first(self) -> None:
        names = {"a", "b", "c"}
        # a depends on b, b depends on c => correct publish order is c, b, a
        deps = {"a": {"b"}, "b": {"c"}, "c": set()}
        order, cycle = topo_order(names, deps)
        self.assertEqual(cycle, [])
        self.assertLess(order.index("c"), order.index("b"))
        self.assertLess(order.index("b"), order.index("a"))

    def test_independent_nodes_are_alphabetically_stable(self) -> None:
        names = {"zeta", "alpha", "mu"}
        deps = {"zeta": set(), "alpha": set(), "mu": set()}
        order, cycle = topo_order(names, deps)
        self.assertEqual(cycle, [])
        self.assertEqual(order, ["alpha", "mu", "zeta"])

    def test_diamond_dependency_resolves_correctly(self) -> None:
        # top depends on left and right; both depend on bottom.
        names = {"top", "left", "right", "bottom"}
        deps = {
            "top": {"left", "right"},
            "left": {"bottom"},
            "right": {"bottom"},
            "bottom": set(),
        }
        order, cycle = topo_order(names, deps)
        self.assertEqual(cycle, [])
        self.assertEqual(order[0], "bottom")
        self.assertEqual(order[-1], "top")
        self.assertLess(order.index("left"), order.index("top"))
        self.assertLess(order.index("right"), order.index("top"))

    def test_cycle_is_reported_not_silently_dropped(self) -> None:
        names = {"a", "b"}
        deps = {"a": {"b"}, "b": {"a"}}
        order, cycle = topo_order(names, deps)
        self.assertEqual(sorted(cycle), ["a", "b"])


class ParseCratesArrayTests(unittest.TestCase):
    def test_parses_simple_array(self) -> None:
        script = (
            "VERSION=\"0.1.7\"\n"
            "CRATES=(\n"
            "    \"oxigdal-core\"\n"
            "    \"oxigdal-proj\"  # comment\n"
            "    \"oxigdal-geotiff\"\n"
            ")\n"
            "echo done\n"
        )
        entries = parse_crates_array(script)
        self.assertEqual(entries, ["oxigdal-core", "oxigdal-proj", "oxigdal-geotiff"])

    def test_returns_empty_list_when_array_absent(self) -> None:
        self.assertEqual(parse_crates_array("echo no array here\n"), [])

    def test_detects_missing_entry_against_known_reference_case(self) -> None:
        # Regression fixture mirroring the real bug this tool was built to catch:
        # oxigdal-wasm-geoparquet absent from an otherwise-complete CRATES array.
        script = (
            "CRATES=(\n"
            "    \"oxigdal-core\"\n"
            "    \"oxigdal-wasm\"\n"
            "    \"oxigdal-cli\"\n"
            ")\n"
        )
        entries = parse_crates_array(script)
        self.assertNotIn("oxigdal-wasm-geoparquet", entries)


if __name__ == "__main__":
    unittest.main()
