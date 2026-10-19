#!/bin/bash
# Assertions on out/wine (Task 2). Runs on the host; executes wine inside the build container.
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
source "$REPO/build/versions.env"
W=$REPO/out/wine
fail=0
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; fail=1; fi; }

check "out/wine exists" '[ -x "$W/bin/wine" ]'
ver=$("$REPO/build/podman.sh" run --rm -v "$REPO:/src:z" "$BUILD_IMAGE" /src/out/wine/bin/wine --version 2>/dev/null)
check "wine --version is wine-11.0 (got '$ver')" '[ "$ver" = "wine-11.0" ]'
check "wine.inf lists Cubase15.exe" 'grep -q "Cubase15.exe" "$W/share/wine/wine.inf"'
check "wine.inf has HideWineVersion" 'grep -qi HideWineVersion "$W/share/wine/wine.inf"'
for d in d2d1 dcomp dxgi d3d11 dwrite uianimation pwrshsip; do
    check "x86_64-windows/$d.dll" '[ -f "$W/lib/wine/x86_64-windows/$d.dll" ]'
done
check "i386-windows/d2d1.dll" '[ -f "$W/lib/wine/i386-windows/d2d1.dll" ]'
check "wine mono msi" '[ -f "$W/share/wine/mono/wine-mono-$WINE_MONO_VERSION-x86.msi" ]'
check "glibc <= 2.35" '"$REPO/build/tests/check-glibc.sh" "$W" >/dev/null'
check "ntdll.so does not need libunwind" '! objdump -p "$W/lib/wine/x86_64-unix/ntdll.so" 2>/dev/null | grep -q libunwind'
exit $fail
