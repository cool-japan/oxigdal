#!/usr/bin/env bash
# OxiGDAL Docker build helper.
#
# Builds each Dockerfile in this directory with `--build-arg OXIGDAL_VERSION=<version>`,
# where <version> is read from the workspace root Cargo.toml's `[workspace.package].version`
# so the `org.opencontainers.image.version` OCI label baked into every image (and the
# app.kubernetes.io/version label rendered by k8s/render-manifests.sh) never drifts from the
# actual crate version again.
#
# Usage:
#   docker/build.sh                 # build all images (server, cli, dev, jupyter)
#   docker/build.sh server           # build only docker/Dockerfile.server
#   docker/build.sh --print-version  # print the resolved version and exit
#
# The image tag applied is `oxigdal/<name>:<version>` plus a floating `:latest` tag.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

resolve_version() {
    # `[workspace.package]` is the first `version = "..."` line in the root Cargo.toml; every
    # crate in the workspace inherits it via `version.workspace = true`.
    grep -m1 '^version *= *"' "${repo_root}/Cargo.toml" | sed -E 's/^version *= *"([^"]+)".*/\1/'
}

oxigdal_version="$(resolve_version)"

if [[ -z "${oxigdal_version}" ]]; then
    echo "error: could not resolve workspace version from ${repo_root}/Cargo.toml" >&2
    exit 1
fi

if [[ "${1:-}" == "--print-version" ]]; then
    echo "${oxigdal_version}"
    exit 0
fi

declare -A dockerfiles=(
    [server]="docker/Dockerfile.server"
    [cli]="docker/Dockerfile.cli"
    [dev]="docker/Dockerfile.dev"
    [jupyter]="docker/Dockerfile.jupyter"
)

targets=("$@")
if [[ ${#targets[@]} -eq 0 ]]; then
    targets=(server cli dev jupyter)
fi

for target in "${targets[@]}"; do
    dockerfile="${dockerfiles[${target}]:-}"
    if [[ -z "${dockerfile}" ]]; then
        echo "error: unknown build target '${target}' (expected one of: ${!dockerfiles[*]})" >&2
        exit 1
    fi

    echo "Building oxigdal/${target}:${oxigdal_version} from ${dockerfile} ..."
    docker build \
        --build-arg "OXIGDAL_VERSION=${oxigdal_version}" \
        -f "${repo_root}/${dockerfile}" \
        -t "oxigdal/${target}:${oxigdal_version}" \
        -t "oxigdal/${target}:latest" \
        "${repo_root}"
done
