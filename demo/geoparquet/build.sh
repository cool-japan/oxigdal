#!/usr/bin/env bash
#
# OxiGeo GeoParquet Live — WASM build script.
#
# Builds crates/oxigeo-wasm-geoparquet with wasm-pack (web target) and
# rewrites the generated package.json name to the published npm scope.
# Idempotent: re-running against an already-rewritten pkg is a no-op.
#
# The demo's ./pkg is a symlink to the crate's pkg output directory.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CRATE_DIR="${REPO_ROOT}/crates/oxigeo-wasm-geoparquet"
PKG_DIR="${CRATE_DIR}/pkg"
NPM_NAME="@cooljapan/oxigeo-geoparquet"

if ! command -v wasm-pack >/dev/null 2>&1; then
    echo "error: wasm-pack is not installed (cargo install wasm-pack)" >&2
    exit 1
fi

echo "Building ${CRATE_DIR} → ${PKG_DIR}"
wasm-pack build "${CRATE_DIR}" \
    --target web \
    --release \
    --out-dir pkg \
    --out-name oxigeo_geoparquet

# Rewrite the package name (wasm-pack derives "oxigeo-wasm-geoparquet"
# from the crate name); a no-op when the name is already correct.
python3 - "${PKG_DIR}/package.json" "${NPM_NAME}" <<'PY'
import json, sys
path, name = sys.argv[1], sys.argv[2]
with open(path, encoding="utf-8") as f:
    pkg = json.load(f)
if pkg.get("name") != name:
    pkg["name"] = name
    with open(path, "w", encoding="utf-8") as f:
        json.dump(pkg, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"package.json name → {name}")
else:
    print(f"package.json name already {name} (no change)")
PY

WASM="${PKG_DIR}/oxigeo_geoparquet_bg.wasm"
SIZE=$(wc -c < "${WASM}" | tr -d ' ')
echo "Done: ${WASM} (${SIZE} bytes)"

# Size gate from the design contract: < 4 MB gzipped is the goal; warn
# on raw size above 8 MB so regressions are caught early.
if [ "${SIZE}" -gt 8388608 ]; then
    echo "warning: wasm binary exceeds 8 MB raw — investigate before staging" >&2
fi

echo ""
echo "Serve the demo (range-request capable server required):"
echo "  cd ${SCRIPT_DIR} && python3 serve.py 8080"
