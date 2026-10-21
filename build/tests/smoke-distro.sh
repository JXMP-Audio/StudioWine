#!/bin/bash
# smoke-distro.sh IMAGE — start the AppImage on another distribution: `--wine --version`,
# then the launcher under Xvfb for a few seconds (window must appear, process must not crash).
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
IMG=${1:?image}
case $IMG in
  *fedora*) PREP='dnf -y -q install xorg-x11-server-Xvfb xdotool mesa-libGL mesa-libEGL mesa-dri-drivers libxkbcommon-x11 libXcursor libXi libXrandr libXinerama libXcomposite fontconfig procps-ng >/dev/null' ;;
  *ubuntu*|*debian*) PREP='apt-get update -qq >/dev/null && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends xvfb xdotool libgl1 libegl1 libgl1-mesa-dri libxkbcommon-x11-0 libxcursor1 libxi6 libxrandr2 libxinerama1 libxcomposite1 fontconfig procps >/dev/null' ;;
  *) echo "unknown image family: $IMG" >&2; exit 2 ;;
esac
"$REPO/build/podman.sh" run --rm -v "$REPO:/src:ro,z" -e XDG_DATA_HOME=/tmp/data -e HOME=/tmp "$IMG" sh -c "
  $PREP || exit 3
  A=/src/out/StudioWine-$(. "$REPO/build/versions.env"; echo "$STUDIOWINE_VERSION")-x86_64.AppImage
  echo \"glibc: \$(ldd --version | head -1)\"
  echo \"wine: \$(\$A --appimage-extract-and-run --wine --version)\"
  Xvfb :99 -screen 0 1280x800x24 >/dev/null 2>&1 & export DISPLAY=:99; sleep 2
  \$A --appimage-extract-and-run >/tmp/l.log 2>&1 & sleep 10
  if xdotool search --name StudioWine >/dev/null; then echo 'window: StudioWine is open'; else echo 'window: MISSING'; tail -5 /tmp/l.log; exit 1; fi
  pgrep -x studiowine >/dev/null && echo 'process: running' || { echo 'process: died'; exit 1; }
"
