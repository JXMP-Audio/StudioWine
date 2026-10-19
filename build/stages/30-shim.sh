#!/bin/bash
# Stage 30 (in container): System.Management.Automation shim for Wine Mono → out/shim.
#   30-shim.sh [--test]   --test also runs the fork's regression cases in a scratch prefix.
set -euo pipefail
cd /src
SRC=/src/out/.cache/wine-src/scripts/mono-sma-shim
OUT=/src/out/shim
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
cp -r "$SRC" "$WORK/shim"
bash "$WORK/shim/build.sh"            # host mcs is in the image; no prefix needed
rm -rf "$OUT" && mkdir -p "$OUT"
cp "$WORK/shim/System.Management.Automation.dll" "$OUT/"
cp "$WORK/shim/test/smahost.exe" "$OUT/"
cp /src/out/.cache/wine-src/COPYING.LIB "$OUT/LICENSE"
echo "shim built: $OUT"

if [ "${1:-}" = "--test" ]; then
    export WINEPREFIX=$WORK/prefix WINEDEBUG=-all WINEDLLOVERRIDES=mshtml=d
    export PATH=/src/out/wine/bin:$PATH
    xvfb-run -a sh -c 'wine wineboot -u >/dev/null 2>&1; wineserver -w'
    xvfb-run -a env WINE=/src/out/wine/bin/wine bash "$WORK/shim/test/run-tests.sh" --prefix "$WINEPREFIX"
fi
