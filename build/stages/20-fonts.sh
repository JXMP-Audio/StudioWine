#!/bin/bash
# Stage 20 (in container): free substitute fonts → out/fonts + manifest.json.
set -euo pipefail
cd /src
source build/versions.env
DL=/src/out/.cache/dl
OUT=/src/out/fonts
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$DL"

fetch() { # fetch URL SHA256 NAME
    local f="$DL/$3"
    [ -f "$f" ] || curl -fsSL -o "$f" "$1"
    echo "$2  $f" | sha256sum -c - >/dev/null
    echo "$f"
}

rm -rf "$OUT"
mkdir -p "$OUT/licenses" "$OUT/smufl/Bravura" "$OUT/smufl/Petaluma"
SUBST=()   # "file|substitute_for"

# Selawik → "Segoe UI", Windows file names.
zip=$(fetch "$SELAWIK_URL" "$SELAWIK_SHA256" Selawik_Release.zip)
unzip -q -o "$zip" '*.ttf' -d "$TMP/selawik"
ren() { python3 -I build/tools/rename_font.py "$1" "$OUT/$2" --family "$3" --subfamily "$4"; SUBST+=("$2|$3"); }
ren "$TMP/selawik/selawkl.ttf"  segoeuil.ttf  "Segoe UI" Light
ren "$TMP/selawik/selawksl.ttf" segoeuisl.ttf "Segoe UI" Semilight
ren "$TMP/selawik/selawk.ttf"   segoeui.ttf   "Segoe UI" Regular
ren "$TMP/selawik/selawksb.ttf" seguisb.ttf   "Segoe UI" Semibold
ren "$TMP/selawik/selawkb.ttf"  segoeuib.ttf  "Segoe UI" Bold
cp "$(fetch "$SELAWIK_LICENSE_URL" "$SELAWIK_LICENSE_SHA256" Selawik-LICENSE.txt)" "$OUT/licenses/Selawik-OFL.txt"

# Liberation Sans → "Microsoft Sans Serif".
LIB=/usr/share/fonts/truetype/liberation2
ren "$LIB/LiberationSans-Regular.ttf" micross.ttf "Microsoft Sans Serif" Regular

# Core-font file names that plugins open by path (fork wine-font-setup.sh MSCORE_FILE_SUBST).
for pair in arial:Sans times:Serif cour:Mono; do
    base=${pair%%:*}; fam=${pair##*:}
    cp "$LIB/Liberation$fam-Regular.ttf"    "$OUT/$base.ttf"
    cp "$LIB/Liberation$fam-Bold.ttf"       "$OUT/${base}bd.ttf"
    cp "$LIB/Liberation$fam-Italic.ttf"     "$OUT/${base}i.ttf"
    cp "$LIB/Liberation$fam-BoldItalic.ttf" "$OUT/${base}bi.ttf"
    for s in "" bd i bi; do SUBST+=("$base$s.ttf|$( case $base in arial) echo Arial;; times) echo Times New Roman;; cour) echo Courier New;; esac )"); done
done
cp /usr/share/doc/fonts-liberation2/copyright "$OUT/licenses/Liberation-copyright.txt"

# Free fonts used as-is.
DJ=/usr/share/fonts/truetype/dejavu
cp "$DJ/DejaVuSans.ttf" "$DJ/DejaVuSans-Bold.ttf" "$DJ/DejaVuSerif.ttf" "$DJ/DejaVuSansMono.ttf" "$OUT/"
cp /usr/share/doc/fonts-dejavu-core/copyright "$OUT/licenses/DejaVu-copyright.txt"
cp /usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf "$OUT/"
cp /usr/share/doc/fonts-noto-core/copyright "$OUT/licenses/Noto-copyright.txt"

cp "$(fetch "$BRAVURA_BASE/Bravura.otf" "$BRAVURA_OTF_SHA256" Bravura.otf)" "$OUT/"
cp "$(fetch "$BRAVURA_BASE/BravuraText.otf" "$BRAVURATEXT_OTF_SHA256" BravuraText.otf)" "$OUT/"
cp "$(fetch "$BRAVURA_BASE/Bravura.json" "$BRAVURA_JSON_SHA256" Bravura.json)" "$OUT/smufl/Bravura/bravura_metadata.json"
cp "$(fetch "$BRAVURA_LICENSE_URL" "$BRAVURA_LICENSE_SHA256" bravura-OFL.txt)" "$OUT/licenses/Bravura-OFL.txt"

tgz=$(fetch "$PETALUMA_URL" "$PETALUMA_SHA256" petaluma-1.065.tar.gz)
tar -xzf "$tgz" -C "$TMP"
P=$(echo "$TMP"/petaluma-*/redist)
cp "$P"/otf/Petaluma.otf "$P"/otf/PetalumaText.otf "$P"/otf/PetalumaScript.otf "$OUT/"
cp "$P/petaluma_metadata.json" "$OUT/smufl/Petaluma/"
cp "$P/OFL.txt" "$OUT/licenses/Petaluma-OFL.txt"

cat > "$OUT/licenses/RENAMED.txt" <<'EOF'
StudioWine ships modified copies of fonts licensed under the SIL Open Font License 1.1.
Only the naming tables were changed, so that Windows applications find them under the
family names they request. As the OFL requires, the modified versions do not use the
Reserved Font Names of the originals.

  segoeuil.ttf, segoeuisl.ttf, segoeui.ttf, seguisb.ttf, segoeuib.ttf
      modified from Selawik 1.01 (Microsoft Corporation, Reserved Font Name "Selawik")
  micross.ttf
      modified from Liberation Sans Regular 2.x (Red Hat, Inc.)

They are free substitutes, not the Microsoft fonts of the same name. Users who hold a
licence for the genuine fonts can install them with StudioWine's "Fonts…" dialog.
EOF

# manifest.json: every font file; substitute_for = the family a renamed/stand-in file replaces.
printf '%s\n' "${SUBST[@]}" > "$TMP/subst.txt"
python3 -I - "$OUT" "$TMP/subst.txt" <<'EOF'
import hashlib, json, os, sys
from fontTools.ttLib import TTFont
out, subst_file = sys.argv[1], sys.argv[2]
subst = dict(l.rstrip("\n").split("|", 1) for l in open(subst_file) if l.strip())
entries = []
for f in sorted(os.listdir(out)):
    if not f.lower().endswith((".ttf", ".otf")):
        continue
    p = os.path.join(out, f)
    name = TTFont(p)["name"]
    fam = name.getDebugName(16) or name.getDebugName(1)
    entries.append({"file": f, "family": fam, "substitute_for": subst.get(f),
                    "sha256": hashlib.sha256(open(p, "rb").read()).hexdigest()})
json.dump(entries, open(os.path.join(out, "manifest.json"), "w"), indent=1)
print(f"manifest: {len(entries)} fonts")
EOF
