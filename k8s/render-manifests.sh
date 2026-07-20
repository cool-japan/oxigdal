#!/usr/bin/env bash
# Render k8s/deployment.yaml with the current workspace version substituted for the
# `0.0.0-UNRENDERED` placeholder in its `app.kubernetes.io/version` labels, so the label
# tracks the workspace's actual Cargo.toml version instead of being hand-edited (and drifting)
# on every release.
#
# Usage:
#   k8s/render-manifests.sh | kubectl apply -f -
#   k8s/render-manifests.sh > /tmp/oxigdal-deployment.rendered.yaml

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

resolve_version() {
    grep -m1 '^version *= *"' "${repo_root}/Cargo.toml" | sed -E 's/^version *= *"([^"]+)".*/\1/'
}

oxigdal_version="$(resolve_version)"

if [[ -z "${oxigdal_version}" ]]; then
    echo "error: could not resolve workspace version from ${repo_root}/Cargo.toml" >&2
    exit 1
fi

sed "s/0.0.0-UNRENDERED/${oxigdal_version}/g" "${script_dir}/deployment.yaml"
