#!/bin/bash
# Stage 40 (in container): release build of the launcher → out/launcher/studiowine.
set -euo pipefail
cd /src/launcher
cargo build --release --locked
mkdir -p /src/out/launcher
cp "${CARGO_TARGET_DIR:-target}/release/studiowine" /src/out/launcher/studiowine
echo "launcher built: /src/out/launcher/studiowine"
