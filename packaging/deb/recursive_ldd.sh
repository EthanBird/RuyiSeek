#!/usr/bin/env bash
# Recursive ldd: given a starting set of libs, return the full closure of
# transitive shared-library deps.
set -uo pipefail

LIB_DIR="${1:-/usr/lib/x86_64-linux-gnu}"
shift || true
EXTRA_LIBS=( "$@" )

declare -A seen
queue=()

for lib in "${EXTRA_LIBS[@]}"; do
    if [ -z "$lib" ]; then continue; fi
    queue+=( "$lib" )
done

while [ "${#queue[@]}" -gt 0 ]; do
    cur="${queue[0]}"
    queue=( "${queue[@]:1}" )
    path=$(ldconfig -p 2>/dev/null | awk -v lib="$cur" '$1 == lib { print $NF; exit }' || true)
    if [ -z "$path" ]; then
        path=$(find "$LIB_DIR" -maxdepth 1 \( -name "${cur}.*" -type f -o -name "${cur}" -type l \) 2>/dev/null | head -1 || true)
    fi
    if [ -z "$path" ] || [ ! -e "$path" ]; then
        echo "MISSING: $cur" >&2
        continue
    fi
    if [ -n "${seen[$cur]:-}" ]; then continue; fi
    seen[$cur]="$path"
    while IFS= read -r line; do
        case "$line" in
            *'=> not found'*)
                name=$(awk '{print $1}' <<<"$line")
                echo "MISSING-DEP: $cur -> $name" >&2
                ;;
            *'=>'*)
                name=$(awk '{print $1}' <<<"$line")
                if [ -z "${seen[$name]:-}" ]; then
                    queue+=( "$name" )
                fi
                ;;
        esac
    done < <(ldd "$path" 2>/dev/null || true)
done

for soname in "${!seen[@]}"; do
    printf '%s\t%s\n' "$soname" "${seen[$soname]}"
done | sort