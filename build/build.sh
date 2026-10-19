#!/bin/bash
# StudioWine build entry point.
#   build/build.sh [all|image|wine|fonts|shim|launcher|appdir|appimage]...
set -euo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
source "$REPO/build/versions.env"
PODMAN="$REPO/build/podman.sh"

image() {
    local args=()
    for v in LLVM_MINGW_URL LLVM_MINGW_SHA256 NTSYNC_H_URL NTSYNC_H_SHA256 RUST_VERSION; do
        args+=(--build-arg "$v=${!v}")
    done
    "$PODMAN" build -t "$BUILD_IMAGE" -f "$REPO/build/Containerfile" "${args[@]}" "$REPO/build"
}

# Run a stage script inside the build container with the repo at /src.
in_container() {
    "$PODMAN" run --rm --userns=keep-id -v "$REPO:/src:z" -w /src \
        -e HOME=/tmp -e CARGO_TARGET_DIR=/src/out/.cache/cargo-target \
        "$BUILD_IMAGE" bash "$@"
}

stage() {
    case $1 in
        image)    image ;;
        wine)     in_container build/stages/10-wine.sh ;;
        fonts)    in_container build/stages/20-fonts.sh ;;
        shim)     in_container build/stages/30-shim.sh --test ;;
        launcher) in_container build/stages/40-launcher.sh ;;
        appdir)   in_container build/stages/50-appdir.sh ;;
        appimage) in_container build/stages/60-appimage.sh ;;
        all)      for s in image wine fonts shim launcher appdir appimage; do stage "$s"; done ;;
        *) echo "unknown stage: $1" >&2; exit 2 ;;
    esac
}

[ $# -gt 0 ] || set -- all
for s in "$@"; do stage "$s"; done
