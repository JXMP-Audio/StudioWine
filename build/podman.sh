#!/bin/bash
# Run podman on the host. Inside a toolbox/distrobox dev container podman lives
# on the host, so the call goes through host-spawn.
set -euo pipefail
if [ -e /run/.containerenv ] && command -v host-spawn >/dev/null 2>&1; then
    exec host-spawn podman "$@"
fi
exec podman "$@"
