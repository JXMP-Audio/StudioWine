#!/bin/bash
# Stage 10 (in container): build giang17's Wine fork → out/wine (relocatable, prefix "/").
set -euo pipefail
cd /src
source build/versions.env
SRC=out/.cache/wine-src
BLD=out/.cache/wine-build
OUT=/src/out/wine

if [ ! -d "$SRC/.git" ]; then
    git init -q "$SRC"
    git -C "$SRC" remote add origin "$WINE_REPO"
fi
if [ "$(git -C "$SRC" rev-parse HEAD 2>/dev/null)" != "$WINE_COMMIT" ]; then
    git -C "$SRC" fetch -q --depth 1 origin "$WINE_COMMIT"
    git -C "$SRC" checkout -q --force FETCH_HEAD
fi

mkdir -p "$BLD"
cd "$BLD"
if [ ! -f Makefile ] || [ "$(cat .configured-for 2>/dev/null)" != "$WINE_COMMIT" ]; then
    /src/$SRC/configure --prefix=/ --enable-archs=i386,x86_64 --with-mingw \
        --without-wayland --without-unwind --disable-tests \
        CFLAGS="-O2 -g0" CROSSCFLAGS="-O2 -g0"
    echo "$WINE_COMMIT" > .configured-for
fi
grep -q 'define HAVE_LINUX_NTSYNC_H 1' include/config.h \
    || echo "WARNING: built without ntsync support"

make -j"$(nproc)"
rm -rf "$OUT"
make install DESTDIR="$OUT" >/dev/null

# Wine Mono, found by Wine in share/wine/mono at wineboot time.
mkdir -p "$OUT/share/wine/mono"
MSI=/src/out/.cache/dl/wine-mono-$WINE_MONO_VERSION-x86.msi
[ -f "$MSI" ] || curl -fsSL -o "$MSI" "$WINE_MONO_URL"
echo "$WINE_MONO_SHA256  $MSI" | sha256sum -c -
cp "$MSI" "$OUT/share/wine/mono/"

# Development tools and headers are not shipped.
rm -rf "$OUT/include" "$OUT/share/man" "$OUT/share/applications"
for t in function_grep.pl widl winebuild winecpp wineg++ winegcc winemaker wmc wrc winedump; do
    rm -f "$OUT/bin/$t"
done
find "$OUT/lib/wine/x86_64-unix" -name '*.so' -exec strip --strip-unneeded {} +
strip --strip-unneeded "$OUT/bin/wine" "$OUT/bin/wineserver" 2>/dev/null || true
echo "wine installed to $OUT"
