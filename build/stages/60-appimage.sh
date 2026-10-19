#!/bin/bash
# Stage 60 (in container): AppDir → out/StudioWine-<version>-x86_64.AppImage.
set -euo pipefail
cd /src
source build/versions.env
TOOL=/src/out/.cache/dl/appimagetool-x86_64.AppImage
[ -x "$TOOL" ] || { curl -fsSL -o "$TOOL" "$APPIMAGETOOL_URL"; chmod +x "$TOOL"; }
OUT=/src/out/StudioWine-$STUDIOWINE_VERSION-x86_64.AppImage
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --comp zstd --no-appstream /src/AppDir "$OUT"
echo "AppImage: $OUT ($(du -h "$OUT" | cut -f1))"
