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
# StudioWine's patches on top of the pinned commit. The reset only rewrites files that
# differ, so an unchanged patch set does not trigger a rebuild.
git -C "$SRC" reset -q --hard "$WINE_COMMIT"
for p in /src/build/patches/wine/*.patch; do
    git -C "$SRC" apply --whitespace=nowarn "$p"
    echo "applied $(basename "$p")"
done

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

# WineASIO (64-bit only: Cubase and current plug-ins are 64-bit). Built with the tools and
# headers just installed; libjack.so.0 is dlopen()ed from the host at run time.
ASIO_SRC=/src/out/.cache/wineasio-src
if [ "$(git -C "$ASIO_SRC" rev-parse HEAD 2>/dev/null)" != "$WINEASIO_COMMIT" ]; then
    rm -rf "$ASIO_SRC"
    git init -q "$ASIO_SRC"
    git -C "$ASIO_SRC" fetch -q --depth 1 "$WINEASIO_REPO" "$WINEASIO_COMMIT"
    git -C "$ASIO_SRC" checkout -q --force FETCH_HEAD
fi
git -C "$ASIO_SRC" reset -q --hard "$WINEASIO_COMMIT"
for p in /src/build/patches/wineasio/*.patch; do
    git -C "$ASIO_SRC" apply --whitespace=nowarn "$p"
    echo "applied $(basename "$p")"
done
make -C "$ASIO_SRC" clean >/dev/null
PATH="$OUT/bin:$PATH" make -C "$ASIO_SRC" 64 \
    WINEBUILD_INCLUDEDIR="$OUT/include/wine/windows -I$OUT/include" \
    WINEBUILD_LIBDIR="$OUT/lib/wine/x86_64-unix"
# The placeholder DLL must carry the name of the ELF module (wineasio64.dll.so), not the
# spec file's "wineasio.dll", or Wine's loader cannot find the driver behind it.
(cd "$ASIO_SRC" && "$OUT/bin/winebuild" -F wineasio64.dll -m64 --dll --fake-module \
    -E wineasio.dll.spec build64/*.c.o -o build64/wineasio64.dll)
install -m 644 "$ASIO_SRC/build64/wineasio64.dll" "$OUT/lib/wine/x86_64-windows/"
install -m 755 "$ASIO_SRC/build64/wineasio64.dll.so" "$OUT/lib/wine/x86_64-unix/"
mkdir -p "$OUT/share/licenses/wineasio"
cp "$ASIO_SRC/COPYING.LIB" "$OUT/share/licenses/wineasio/"

# Development tools and headers are not shipped.
rm -rf "$OUT/include" "$OUT/share/man" "$OUT/share/applications"
for t in function_grep.pl widl winebuild winecpp wineg++ winegcc winemaker wmc wrc winedump; do
    rm -f "$OUT/bin/$t"
done
find "$OUT/lib/wine/x86_64-unix" -name '*.so' -exec strip --strip-unneeded {} +
strip --strip-unneeded "$OUT/bin/wine" "$OUT/bin/wineserver" 2>/dev/null || true
echo "wine installed to $OUT"
