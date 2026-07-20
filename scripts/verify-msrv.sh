#!/bin/bash
# OxiGDAL MSRV (Minimum Supported Rust Version) verification script.
#
# Checks `cargo check` for the workspace default-members against the
# rust-version declared in [workspace.package] of the root Cargo.toml,
# using a pinned rustup toolchain (installed on demand if missing).
#
# This script never publishes anything and never mutates Cargo.toml —
# it only reports whether the currently declared MSRV actually builds.
#
# Author: COOLJAPAN OU (Team Kitasan)
# License: Apache-2.0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
CARGO_TOML="$PROJECT_ROOT/Cargo.toml"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

print_info() { echo -e "${BLUE}i${NC} $1"; }
print_ok() { echo -e "${GREEN}OK${NC} $1"; }
print_warn() { echo -e "${YELLOW}!${NC} $1"; }
print_err() { echo -e "${RED}FAIL${NC} $1"; }

usage() {
    cat <<EOF
Usage: $(basename "$0") [--toolchain VERSION] [--packages "pkg1 pkg2 ..."] [--all]

  --toolchain VERSION   Rust toolchain to check against.
                         Defaults to the rust-version declared in
                         [workspace.package] of the root Cargo.toml.
  --packages "..."      Space-separated list of -p package names to check.
                         Defaults to a representative set covering the
                         core/geo drivers plus every crate that pulls in
                         a dependency with a known elevated MSRV (sysinfo).
  --all                 Check every default-member package instead of the
                         representative set (slow: full workspace check).
  -h, --help            Show this help.

Exit status is non-zero if the check fails against the declared MSRV.
This script never publishes and never edits Cargo.toml.
EOF
}

TOOLCHAIN=""
PACKAGES=""
CHECK_ALL="false"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --toolchain)
            TOOLCHAIN="$2"
            shift 2
            ;;
        --packages)
            PACKAGES="$2"
            shift 2
            ;;
        --all)
            CHECK_ALL="true"
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            print_err "Unknown argument: $1"
            usage
            exit 1
            ;;
    esac
done

DECLARED_MSRV="$(grep -m1 '^rust-version' "$CARGO_TOML" | sed -E 's/.*"([^"]+)".*/\1/')"
if [[ -z "$DECLARED_MSRV" ]]; then
    print_err "Could not find rust-version in [workspace.package] of $CARGO_TOML"
    exit 1
fi

if [[ -z "$TOOLCHAIN" ]]; then
    TOOLCHAIN="$DECLARED_MSRV"
fi

print_info "Declared workspace MSRV: $DECLARED_MSRV"
print_info "Checking against toolchain: $TOOLCHAIN"

if ! rustup toolchain list 2>/dev/null | grep -q "^${TOOLCHAIN}\b\|^${TOOLCHAIN}-"; then
    print_info "Toolchain $TOOLCHAIN not installed; installing (minimal profile)..."
    rustup toolchain install "$TOOLCHAIN" --profile minimal
fi

if [[ "$CHECK_ALL" == "true" ]]; then
    print_info "Checking all default-member packages (this may take a while)..."
    cd "$PROJECT_ROOT"
    if cargo "+${TOOLCHAIN}" check --workspace 2>&1 | tee /tmp/oxigdal-msrv-check.log; then
        print_ok "All default-member packages check clean under $TOOLCHAIN"
        exit 0
    else
        print_err "Workspace check failed under $TOOLCHAIN — see /tmp/oxigdal-msrv-check.log"
        exit 1
    fi
fi

if [[ -z "$PACKAGES" ]]; then
    # Representative set: core geometry/driver stack plus every crate known
    # to depend (directly or via workspace inheritance) on `sysinfo`, whose
    # published patch releases have historically raised their own MSRV
    # ahead of this workspace's declared floor.
    PACKAGES="oxigdal-core oxigdal-algorithms oxigdal-proj oxigdal-geotiff oxigdal oxigdal-server oxigdal-ml oxigdal-cluster oxigdal-bench oxigdal-dev-tools oxigdal-edge"
fi

print_info "Packages: $PACKAGES"

cd "$PROJECT_ROOT"
# shellcheck disable=SC2086
if cargo "+${TOOLCHAIN}" check $(printf -- '-p %s ' $PACKAGES) 2>&1 | tee /tmp/oxigdal-msrv-check.log; then
    print_ok "Representative package set checks clean under $TOOLCHAIN"
    exit 0
else
    print_err "Check failed under $TOOLCHAIN — see /tmp/oxigdal-msrv-check.log for the offending crate"
    print_warn "If the failure names a third-party dependency's own MSRV requirement,"
    print_warn "the workspace rust-version must be raised to match (or that dependency pinned lower)."
    exit 1
fi
