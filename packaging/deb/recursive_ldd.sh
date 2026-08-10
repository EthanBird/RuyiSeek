#!/usr/bin/env bash
# Print the transitive shared-library closure as: SONAME<TAB>resolved-path.

set -euo pipefail

LIB_DIR="${1:-/usr/lib/x86_64-linux-gnu}"
shift || true
EXTRA_LIBS=("$@")

declare -A seen
queue=("${EXTRA_LIBS[@]}")
missing=0

resolve_library() {
    local soname="$1"
    local path=""

    # Prefer the requested architecture directory. ldconfig may contain the
    # same SONAME for amd64 and an enabled i386 foreign architecture.
    path=$(find "$LIB_DIR" -maxdepth 1 \
        \( -name "$soname" -o -name "$soname.*" \) \
        \( -type f -o -type l \) -print 2>/dev/null | sort -V | head -1 || true)
    if [ -z "$path" ]; then
        path=$(ldconfig -p 2>/dev/null | awk -v lib="$soname" \
            '$1 == lib && ($0 ~ /x86-64/ || $NF ~ /x86_64-linux-gnu/) { print $NF; exit }' || true)
    fi
    printf '%s' "$path"
}

while [ "${#queue[@]}" -gt 0 ]; do
    current="${queue[0]}"
    queue=("${queue[@]:1}")
    [ -n "$current" ] || continue
    [ -z "${seen[$current]:-}" ] || continue

    path=$(resolve_library "$current")
    if [ -z "$path" ] || [ ! -e "$path" ]; then
        echo "recursive_ldd.sh: missing library: $current" >&2
        missing=$((missing + 1))
        continue
    fi
    seen[$current]="$path"

    while IFS= read -r line; do
        case "$line" in
            *'=> not found'*)
                dependency=$(awk '{print $1}' <<<"$line")
                echo "recursive_ldd.sh: missing dependency: $current -> $dependency" >&2
                missing=$((missing + 1))
                ;;
            *'=>'*)
                dependency=$(awk '{print $1}' <<<"$line")
                [ -n "${seen[$dependency]:-}" ] || queue+=("$dependency")
                ;;
        esac
    done < <(ldd "$path" 2>/dev/null || true)
done

if [ "$missing" -ne 0 ]; then
    exit 1
fi

for soname in "${!seen[@]}"; do
    printf '%s\t%s\n' "$soname" "${seen[$soname]}"
done | sort
