#!/bin/bash
# bundle_libs.sh APPDIR — copy the shared libraries Wine's Unix side needs into APPDIR/usr/lib.
# Walks DT_NEEDED recursively and stops at libraries that must come from the host
# (glibc, GPU drivers, X11/Wayland, audio, GStreamer/GLib), so nothing reachable only
# through a host library is bundled.
set -euo pipefail
APPDIR=${1:?appdir}
DEST=$APPDIR/usr/lib
mkdir -p "$DEST"
HOST='^(ld-linux.*|libc|libm|libdl|libpthread|librt|libresolv|libutil|libgcc_s|libstdc\+\+|libGL.*|libEGL.*|libGLX.*|libGLdispatch|libOpenGL|libOSMesa|libgbm|libdrm.*|libvulkan|libX.*|libxcb.*|libxkbcommon.*|libwayland.*|libasound|libpulse.*|libpipewire.*|libdbus-1|libudev|libsystemd|libz|libgst.*|libglib-2\.0|libgobject-2\.0|libgmodule-2\.0|libgio-2\.0|liborc-.*|libcups|libsane|libusb-1\.0|libpcap|libkrb5|libgssapi_krb5|libSDL2.*|libv4l.*|libOpenCL|libnetapi|libodbc)\.so'
# Libraries Wine dlopen()s by soname; they have no DT_NEEDED entry anywhere.
DLOPEN=(libfreetype.so.6 libfontconfig.so.1 libgnutls.so.30)

resolve() { ldconfig -p | awk -v n="$1" '$1 == n && /x86-64/ && !found {print $NF; found=1}' || true; }
needed() { objdump -p "$1" 2>/dev/null | awk '/NEEDED/ {print $2}'; }

declare -A seen=()
queue=()
for o in "$APPDIR"/wine/bin/wine "$APPDIR"/wine/bin/wineserver "$APPDIR"/wine/lib/wine/x86_64-unix/*.so; do
    while read -r n; do queue+=("$n"); done < <(needed "$o")
done
queue+=("${DLOPEN[@]}")
while [ ${#queue[@]} -gt 0 ]; do
    name=${queue[0]}; queue=("${queue[@]:1}")
    [ -n "${seen[$name]:-}" ] && continue
    seen[$name]=1
    [[ $name =~ $HOST ]] && continue
    [ -e "$APPDIR/wine/lib/wine/x86_64-unix/$name" ] && continue   # Wine's own ntdll.so, win32u.so
    path=$(resolve "$name")
    [ -n "$path" ] || { echo "warning: $name not found in build image" >&2; continue; }
    cp -L "$path" "$DEST/$name"
    while read -r n; do queue+=("$n"); done < <(needed "$path")
done
strip --strip-unneeded "$DEST"/*.so* 2>/dev/null || true
echo "bundled $(ls "$DEST" | wc -l) libraries into $DEST: $(ls "$DEST" | tr '\n' ' ')"
