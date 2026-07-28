#!/usr/bin/env python3
"""Generate `scripts/publish-order.txt`: the topological crate-publish order
for every publishable workspace member, derived from the real dependency
graph reported by `cargo metadata`.

This is a *generator*, not a publisher: it never runs `cargo publish` and
never touches crates.io. It only reads `cargo metadata --format-version 1`
output (workspace members + their intra-workspace resolved dependency edges,
honoring the default-features-only build `cargo publish` actually performs)
and writes the resulting order to `scripts/publish-order.txt`.

Rationale (COOLJAPAN release-engineering note): the order previously lived
only in the external, unversioned `~/work/pub_oxigeo.sh` release script.
That script's `CRATES=(...)` array must stay dependency-ordered by hand,
which silently drifts as crates are added/renamed. This script -- and the
committed `scripts/publish-order.txt` it produces -- gives the repo an
in-tree, regenerable source of truth that `scripts/verify-publish-order.py`
(already in this directory) can check the external script against.

Usage:
    python3 scripts/gen-publish-order.py [--check]

    --check   Do not write the file; exit non-zero if the existing
              `scripts/publish-order.txt` is stale relative to the current
              dependency graph (useful in a pre-publish sanity pass).

To regenerate after adding/removing/re-wiring a crate:
    python3 scripts/gen-publish-order.py
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from collections import defaultdict, deque
from datetime import datetime, timezone


def project_root() -> str:
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.dirname(here)


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


def build_graph(meta: dict):
    """Returns (workspace_names, deps_by_name, publishable_by_name, dir_by_name).

    `deps_by_name[x]` is the set of *other workspace crates* that `x` depends
    on via a normal or build-time edge (dev-only edges are dropped, since they
    never gate what must already be on the registry for `cargo publish` to
    succeed).
    """
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
        # Cargo: `publish` is `None` (unset, defaults to publishable) or a
        # non-empty list of allowed registries -- both mean "publishable".
        # An explicit `publish = false` serializes as an empty list `[]`.
        publish_field = pkg.get("publish")
        publishable[name] = publish_field is None or publish_field != []
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


def topo_order(names: set[str], deps: dict[str, set[str]]):
    """Kahn's algorithm; ties broken alphabetically for stable, readable
    output. Returns (order, cycle_members) -- cycle_members is non-empty if
    a cycle exists (which would make publishing impossible without a
    temporary path/git dependency break).
    """
    indeg = {n: 0 for n in names}
    for n in names:
        for d in deps.get(n, ()):
            if d in indeg:
                indeg[n] += 1

    dependents: dict[str, set[str]] = defaultdict(set)
    for n in names:
        for d in deps.get(n, ()):
            if d in names:
                dependents[d].add(n)

    ready_dq: deque[str] = deque(sorted(n for n, deg in indeg.items() if deg == 0))
    order: list[str] = []
    remaining = dict(indeg)

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


HEADER_TEMPLATE = """\
# scripts/publish-order.txt
#
# Topological `cargo publish` order for every publishable OxiGeo workspace
# crate: each crate appears strictly after every OTHER workspace crate it
# depends on (via a normal or build-time dependency edge; dev-dependencies
# are excluded, since `cargo publish` never needs those already registered).
# Ties among crates with no ordering constraint between them are broken
# alphabetically for a stable, readable diff.
#
# GENERATED FILE -- do not hand-edit. Regenerate with:
#     python3 scripts/gen-publish-order.py
#
# The generator reads `cargo metadata --format-version 1` (the ground-truth
# dependency graph, including default-features-only resolution -- the same
# feature set `cargo publish` builds with) and topologically sorts workspace
# members via Kahn's algorithm. It never runs `cargo publish` and never
# touches crates.io.
#
# Crates with `publish = false` in their Cargo.toml (e.g. internal example/
# benchmark-only crates) are intentionally excluded from this list.
#
# Generated: {generated_at}
# Workspace root: {root}
# Publishable crate count: {count}
#
"""


def render(root: str, publish_order: list[str]) -> str:
    header = HEADER_TEMPLATE.format(
        generated_at=datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC"),
        root=root,
        count=len(publish_order),
    )
    body = "\n".join(publish_order) + "\n"
    return header + body


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument(
        "--check",
        action="store_true",
        help="don't write the file; exit non-zero if it would change",
    )
    parser.add_argument(
        "--out",
        default=None,
        help="output path (default: <root>/scripts/publish-order.txt)",
    )
    args = parser.parse_args()

    root = project_root()
    out_path = args.out or os.path.join(root, "scripts", "publish-order.txt")

    meta = load_metadata(root)
    ws_names, deps, publishable, _dirs = build_graph(meta)
    publishable_names = {n for n, ok in publishable.items() if ok}

    order, cycle_members = topo_order(ws_names, deps)
    if cycle_members:
        print("ERROR: dependency cycle detected among workspace members:", file=sys.stderr)
        print(" ", ", ".join(cycle_members), file=sys.stderr)
        return 2

    publish_order = [n for n in order if n in publishable_names]
    rendered = render(root, publish_order)

    if args.check:
        if not os.path.isfile(out_path):
            print(f"MISSING: {out_path} does not exist yet.", file=sys.stderr)
            return 1
        existing = open(out_path, encoding="utf-8").read()
        # Ignore the "Generated: <timestamp>" line when diffing for staleness.
        existing_body = "\n".join(
            line for line in existing.splitlines() if not line.startswith("# Generated:")
        )
        rendered_body = "\n".join(
            line for line in rendered.splitlines() if not line.startswith("# Generated:")
        )
        if existing_body != rendered_body:
            print(f"STALE: {out_path} does not match the current dependency graph.", file=sys.stderr)
            print("Regenerate with: python3 scripts/gen-publish-order.py", file=sys.stderr)
            return 1
        print(f"OK: {out_path} is up to date ({len(publish_order)} crates).")
        return 0

    with open(out_path, "w", encoding="utf-8") as f:
        f.write(rendered)
    print(f"Wrote {out_path} ({len(publish_order)} publishable crates).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
