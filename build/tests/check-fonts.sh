#!/bin/bash
# Assertions on out/fonts (Task 3). fc-scan runs in the build container.
set -uo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
source "$REPO/build/versions.env"
F=$REPO/out/fonts
fail=0
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; fail=1; fi; }

check "manifest exists" '[ -f "$F/manifest.json" ]'
[ -f "$F/manifest.json" ] || exit 1
check "manifest files exist with matching sha256" 'python3 -I - "$F" <<EOF
import hashlib, json, os, sys
d = sys.argv[1]
ok = True
for e in json.load(open(os.path.join(d, "manifest.json"))):
    p = os.path.join(d, e["file"])
    if not os.path.isfile(p) or hashlib.sha256(open(p, "rb").read()).hexdigest() != e["sha256"]:
        print("  bad:", e["file"]); ok = False
sys.exit(0 if ok else 1)
EOF'

family() { "$REPO/build/podman.sh" run --rm -v "$REPO:/src:z" "$BUILD_IMAGE" \
    fc-scan --format '%{family[0]}\n' "/src/out/fonts/$1" 2>/dev/null | head -1; }
for f in segoeui.ttf segoeuib.ttf seguisb.ttf segoeuil.ttf segoeuisl.ttf; do
    got=$(family "$f"); check "$f family is Segoe UI (got '$got')" '[ "$got" = "Segoe UI" ]'
done
got=$(family micross.ttf); check "micross.ttf family is Microsoft Sans Serif (got '$got')" '[ "$got" = "Microsoft Sans Serif" ]'
got=$(family arial.ttf); check "arial.ttf family is Liberation Sans (got '$got')" '[ "$got" = "Liberation Sans" ]'
for f in DejaVuSans.ttf NotoSansSymbols2-Regular.ttf Bravura.otf BravuraText.otf Petaluma.otf PetalumaText.otf PetalumaScript.otf \
         smufl/Bravura/bravura_metadata.json smufl/Petaluma/petaluma_metadata.json; do
    check "$f present" '[ -s "$F/$f" ]'
done
check "licenses present (>= 6 files)" '[ "$(ls "$F/licenses" 2>/dev/null | wc -l)" -ge 6 ]'
check "RENAMED.txt present" '[ -s "$F/licenses/RENAMED.txt" ]'
exit $fail
