#!/bin/bash
# check-glibc.sh TREE [MAX] — every ELF under TREE needs at most GLIBC_<MAX> (default 2.35).
set -euo pipefail
TREE=${1:?tree}; MAX=${2:-2.35}
[ -d "$TREE" ] || { echo "FAIL: $TREE does not exist"; exit 1; }
worst=0.0; worst_file=""
while IFS= read -r -d '' f; do
    head -c4 "$f" 2>/dev/null | grep -q $'\x7fELF' || continue
    v=$(objdump -T "$f" 2>/dev/null | grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sed 's/GLIBC_//' | sort -V | tail -1 || true)
    [ -n "$v" ] || continue
    if [ "$(printf '%s\n%s\n' "$worst" "$v" | sort -V | tail -1)" = "$v" ] && [ "$v" != "$worst" ]; then
        worst=$v; worst_file=$f
    fi
done < <(find "$TREE" -type f -print0)
if [ "$(printf '%s\n%s\n' "$MAX" "$worst" | sort -V | tail -1)" != "$MAX" ]; then
    echo "FAIL: $worst_file needs GLIBC_$worst (max $MAX)"; exit 1
fi
echo "OK: highest GLIBC requirement in $TREE is $worst (${worst_file:-none})"
