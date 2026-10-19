#!/bin/bash
# Stage 50 (in container): assemble AppDir from out/{wine,fonts,shim,launcher}.
set -euo pipefail
cd /src
A=/src/AppDir
for d in out/wine out/fonts out/shim out/launcher/studiowine; do
    [ -e "$d" ] || { echo "missing $d — run the earlier stages first" >&2; exit 1; }
done
rm -rf "$A"
mkdir -p "$A/usr/bin" "$A/usr/share/applications" "$A/usr/share/icons/hicolor/scalable/apps"
cp -a out/wine "$A/wine"
cp -a out/fonts "$A/fonts"
cp -a out/shim "$A/shim"
install -m 755 out/launcher/studiowine "$A/usr/bin/studiowine"
install -m 755 build/assets/AppRun "$A/AppRun"
cp build/assets/studiowine.desktop "$A/studiowine.desktop"
cp build/assets/studiowine.desktop "$A/usr/share/applications/"
cp build/assets/studiowine.svg "$A/studiowine.svg"
cp build/assets/studiowine.svg "$A/usr/share/icons/hicolor/scalable/apps/"
ln -sf studiowine.svg "$A/.DirIcon"
bash build/tools/bundle_libs.sh "$A"
echo "AppDir ready: $A"
