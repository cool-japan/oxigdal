#!/usr/bin/env python3
"""Verify (read-only) the dependency-ordered publish list used by the
external `pub_oxigeo.sh` release script against the ground truth
reported by `cargo metadata`.

This tool NEVER runs `cargo publish` and NEVER edits the external
publish script -- it only reads it, computes the correct topological
publish order from the workspace's real dependency graph (honoring
optional dependencies gated behind default features, since `cargo
publish` builds with default features only), and reports:

  * crates that are publishable workspace members but missing from
    the external script's CRATES array (e.g. a newly added crate),
  * stale entries in the CRATES array that no longer correspond to
    any real workspace member (dead references that will hard-fail
    the publish run when reached),
  * true dependency-order violations, i.e. a crate listed before an
    internal dependency it needs to already be on the registry,
  * (optionally, with --emit-order) a corrected `CRATES=(...)` bash
    array snippet in valid topological order, for a human to paste
    into the external script.

Usage:
    python3 scripts/verify-publish-order.py [--external-script PATH]
                                             [--emit-order]

Exit status is non-zero if any missing crate, stale entry, or order
violation is found.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from collections import defaultdict, deque


def project_root() -> str:
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.dirname(here)


def default_external_script(root: str) -> str:
    project_name = os.path.basename(root)
    home = os.path.expanduser("~")
    return os.path.join(home, "work", f"pub_{project_name}.sh")


def load_metadata(root: str) -> dict:
    proc = subprocess.run(
        ["cargo", "metadata", "--format-version=1"],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)
        raise SystemExit(f"cargo metadata failed with exit code {proc.returncode}")
    return json.loads(proc.stdout)


def build_graph(meta: dict) -> tuple[set[str], dict[str, set[str]], dict[str, bool], dict[str, str]]:
    """Returns (workspace_names, deps_by_name, publishable_by_name, dir_by_name)."""
    packages = meta["packages"]
    id_to_pkg = {p["id"]: p for p in packages}
    ws_ids = set(meta["workspace_members"])
    ws_names = {id_to_pkg[i]["name"] for i in ws_ids if i in id_to_pkg}

    publishable: dict[str, bool] = {}
    dirs: dict[str, str] = {}
    name_to_id: dict[str, str] = {}
    for pid in ws_ids:
        pkg = id_to_pkg.get(pid)
        if pkg is None:
            continue
        name = pkg["name"]
        name_to_id[name] = pid
        publishable[name] = pkg.get("publish") is None
        dirs[name] = os.path.dirname(pkg["manifest_path"])

    resolve_nodes = {n["id"]: n for n in meta.get("resolve", {}).get("nodes", [])}
    deps: dict[str, set[str]] = defaultdict(set)
    for name in ws_names:
        pid = name_to_id.get(name)
        node = resolve_nodes.get(pid, {})
        for d in node.get("deps", []):
            dep_name = id_to_pkg.get(d["pkg"], {}).get("name")
            if dep_name is None or dep_name == name or dep_name not in ws_names:
                continue
            kinds = [dk.get("kind") for dk in d.get("dep_kinds", [])]
            # Keep normal (kind is None/null) and build-time edges; drop dev-only.
            if any(k in (None, "build") for k in kinds):
                deps[name].add(dep_name)

    return ws_names, deps, publishable, dirs


def topo_order(names: set[str], deps: dict[str, set[str]]) -> tuple[list[str], list[str]]:
    """Kahn's algorithm; ties broken alphabetically for stable, readable output.
    Returns (order, cycle_members) -- cycle_members is non-empty if a cycle exists.
    """
    indeg = {n: 0 for n in names}
    for n in names:
        for d in deps.get(n, ()):
            if d in indeg:
                indeg[n] += 1

    ready = sorted(n for n, deg in indeg.items() if deg == 0)
    ready_dq: deque[str] = deque(ready)
    order: list[str] = []
    remaining = dict(indeg)

    # Build reverse edges: who depends on me
    dependents: dict[str, set[str]] = defaultdict(set)
    for n in names:
        for d in deps.get(n, ()):
            if d in names:
                dependents[d].add(n)

    while ready_dq:
        batch = sorted(ready_dq)
        ready_dq.clear()
        for n in batch:
            order.append(n)
            for dep_on_n in sorted(dependents.get(n, ())):
                remaining[dep_on_n] -= 1
                if remaining[dep_on_n] == 0:
                    ready_dq.append(dep_on_n)

    cycle_members = sorted(n for n, deg in remaining.items() if deg > 0)
    return order, cycle_members


def parse_crates_array(script_text: str) -> list[str]:
    m = re.search(r"CRATES=\((.*?)\n\)", script_text, re.S)
    if not m:
        return []
    return re.findall(r'"([^"]+)"', m.group(1))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--external-script", default=None, help="path to the external pub_<project>.sh script")
    parser.add_argument("--emit-order", action="store_true", help="print a corrected CRATES=(...) bash snippet")
    args = parser.parse_args()

    root = project_root()
    external_script = args.external_script or default_external_script(root)

    meta = load_metadata(root)
    ws_names, deps, publishable, dirs = build_graph(meta)
    publishable_names = {n for n, ok in publishable.items() if ok}

    order, cycle_members = topo_order(ws_names, deps)
    if cycle_members:
        print("ERROR: dependency cycle detected among workspace members:", file=sys.stderr)
        print(" ", ", ".join(cycle_members), file=sys.stderr)
        return 2

    publish_order = [n for n in order if n in publishable_names]

    print(f"Project root:        {root}")
    print(f"External script:     {external_script}")
    print(f"Publishable crates (cargo metadata): {len(publishable_names)}")
    print()

    exit_code = 0

    if not os.path.isfile(external_script):
        print(f"WARNING: external publish script not found at {external_script}; "
              f"only reporting the computed order.")
        exit_code = 1
    else:
        script_text = open(external_script, encoding="utf-8").read()
        script_entries = parse_crates_array(script_text)
        script_set = set(script_entries)

        missing = sorted(publishable_names - script_set)
        stale = sorted(
            name for name in script_set
            if name not in ws_names and name not in publishable_names
        )
        # position-based order-violation check restricted to entries present in the script
        pos = {name: i for i, name in enumerate(script_entries)}
        violations = []
        for name, dep_set in deps.items():
            if name not in pos or name not in publishable_names:
                continue
            for d in dep_set:
                if d in pos and d in publishable_names and pos[d] > pos[name]:
                    violations.append((name, d, pos[name], pos[d]))

        if missing:
            print(f"MISSING from CRATES array ({len(missing)}):")
            for m in missing:
                print(f"  - {m}")
            exit_code = 1
        else:
            print("No missing publishable crates in the CRATES array.")

        if stale:
            print(f"\nSTALE entries in CRATES array (no matching workspace member; "
                  f"will hard-fail get_crate_path fallback) ({len(stale)}):")
            for s in stale:
                print(f"  - {s}")
            exit_code = 1
        else:
            print("No stale entries in the CRATES array.")

        if violations:
            print(f"\nORDER VIOLATIONS (listed before an internal dependency it needs "
                  f"already published) ({len(violations)}):")
            for name, dep, pos_name, pos_dep in sorted(violations, key=lambda v: v[2]):
                print(f"  - {name} (position {pos_name + 1}) depends on {dep} "
                      f"(position {pos_dep + 1}) -- {dep} must come first")
            exit_code = 1
        else:
            print("No dependency-order violations in the CRATES array.")

    if args.emit_order:
        print("\n# --- corrected CRATES array (topological order) ---")
        print("CRATES=(")
        for name in publish_order:
            print(f'    "{name}"')
        print(")")

    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
