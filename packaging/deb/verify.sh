#!/usr/bin/env bash
# Verify package structure, UOS ABI compatibility and runtime behavior.

set -euo pipefail

SKIP_DAEMON=0
if [ "${1:-}" = "--skip-daemon" ]; then
    SKIP_DAEMON=1
    shift
fi
DEB="${1:?usage: verify.sh [--skip-daemon] <package.deb>}"

for command_name in dpkg-deb readelf file find awk; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "verify.sh: required tool is missing: $command_name" >&2
        exit 1
    fi
done
if [ ! -f "$DEB" ]; then
    echo "verify.sh: package not found: $DEB" >&2
    exit 1
fi

work_dir=$(mktemp -d /tmp/ruyiseek-package-verify.XXXXXX)
cleanup() {
    rm -rf "$work_dir"
}
trap cleanup EXIT

root="$work_dir/root"
dpkg-deb --extract "$DEB" "$root"

package=$(dpkg-deb --field "$DEB" Package)
version=$(dpkg-deb --field "$DEB" Version)
architecture=$(dpkg-deb --field "$DEB" Architecture)
[ "$package" = ruyiseek ]
[ "$architecture" = amd64 ]

# Build helpers at package root were a v0.1.0-12 regression.
unexpected_root_files=$(find "$root" -mindepth 1 -maxdepth 1 -type f -print)
if [ -n "$unexpected_root_files" ]; then
    echo "verify.sh: unexpected package-root files:" >&2
    echo "$unexpected_root_files" >&2
    exit 1
fi

for binary in ruyi ruyiseekd ruyiseek-ui; do
    [ -x "$root/usr/bin/$binary" ] || {
        echo "verify.sh: missing executable: $binary" >&2
        exit 1
    }
done

for binary in ruyi ruyiseekd; do
    if readelf -d "$root/usr/bin/$binary" 2>/dev/null | grep -q '(NEEDED)'; then
        echo "verify.sh: $binary unexpectedly has a dynamic dependency" >&2
        exit 1
    fi
    if readelf -l "$root/usr/bin/$binary" | grep -q 'Requesting program interpreter'; then
        echo "verify.sh: $binary unexpectedly has a dynamic interpreter" >&2
        exit 1
    fi
done

ui="$root/usr/bin/ruyiseek-ui"
runtime="$root/usr/lib/ruyiseek"

broken_links=$(find -L "$runtime" -maxdepth 1 -type l -print)
if [ -n "$broken_links" ]; then
    echo "verify.sh: dangling runtime links:" >&2
    echo "$broken_links" >&2
    exit 1
fi

readelf -l "$ui" | grep -Fq \
    '[Requesting program interpreter: /lib64/ld-linux-x86-64.so.2]'
readelf -d "$ui" | grep -Fq 'Library rpath: [$ORIGIN/../lib/ruyiseek]'

max_glibc=$(
    readelf --version-info "$ui" 2>/dev/null \
        | awk '{for (i = 1; i <= NF; i++) if ($i ~ /^GLIBC_[0-9.]+$/) print substr($i, 7)}' \
        | sort -V | tail -1
)
if [ -n "$max_glibc" ] && [ "$(printf '%s\n%s\n' "$max_glibc" 2.28 | sort -V | tail -1)" != 2.28 ]; then
    echo "verify.sh: UI requires GLIBC_$max_glibc, newer than UOS 20 GLIBC_2.28" >&2
    exit 1
fi

resolution=$(
    /lib64/ld-linux-x86-64.so.2 --inhibit-cache --list "$ui" 2>&1
)
runtime_real=$(readlink -f "$runtime")
while IFS=$'\t' read -r soname resolved_path; do
    [ -n "$resolved_path" ] || continue
    resolved_real=$(readlink -f "$resolved_path")
    case "$resolved_real" in
        "$runtime_real"/*) ;;
        *)
            case "$soname" in
                libc.so.6|libdl.so.2|libm.so.6|libpthread.so.0|librt.so.1|ld-linux-x86-64.so.2)
                    ;;
                *)
                    echo "verify.sh: dependency resolved outside package: $soname -> $resolved_real" >&2
                    echo "$resolution" >&2
                    exit 1
                    ;;
            esac
            ;;
    esac
done < <(awk '
    /=> \// { print $1 "\t" $3 }
    /^[[:space:]]*\// { path=$1; name=path; sub(/^.*\//, "", name); print name "\t" path }
' <<<"$resolution")

demo_output=$(
    "$ui" --demo-double-ctrl 2>&1
)
grep -Fq '双击 Ctrl 已识别' <<<"$demo_output"

if [ "$SKIP_DAEMON" -eq 0 ]; then
    mkdir -p "$work_dir/home" "$work_dir/search-root"
    printf 'UOS compatibility marker\n' \
        >"$work_dir/search-root/volume-visible-marker.txt"
    socket="$work_dir/ruyiseek.sock"
    HOME="$work_dir/home" "$root/usr/bin/ruyiseekd" \
        --root "$work_dir/search-root" --socket "$socket" \
        >"$work_dir/daemon.log" 2>&1 &
    daemon_pid=$!

    ready=0
    for _ in $(seq 1 100); do
        if [ -S "$socket" ]; then
            ready=1
            break
        fi
        if ! kill -0 "$daemon_pid" 2>/dev/null; then
            break
        fi
        sleep 0.05
    done
    if [ "$ready" -ne 1 ]; then
        cat "$work_dir/daemon.log" >&2
        wait "$daemon_pid" 2>/dev/null || true
        echo "verify.sh: daemon socket did not become ready" >&2
        exit 1
    fi

    client="$root/usr/bin/ruyi"
    "$client" --socket "$socket" ping | grep -Fq 'ruyiseekd online'
    "$client" --socket "$socket" search volume-visible-marker \
        | grep -Fq 'volume-visible-marker.txt'

    # A file created after the initial scan must become searchable without
    # restarting the daemon. Keep a Chinese filename here to cover the UOS
    # regression that motivated the runtime inotify refresh.
    printf '运行期索引刷新\n' >"$work_dir/search-root/中文.txt"
    refreshed=0
    for _ in $(seq 1 100); do
        if "$client" --socket "$socket" search 中文 \
            | grep -Fq '中文.txt'; then
            refreshed=1
            break
        fi
        sleep 0.05
    done
    if [ "$refreshed" -ne 1 ]; then
        cat "$work_dir/daemon.log" >&2
        "$client" --socket "$socket" stop >/dev/null 2>&1 || true
        wait "$daemon_pid" 2>/dev/null || true
        echo "verify.sh: runtime-created Chinese filename was not indexed" >&2
        exit 1
    fi

    "$client" --socket "$socket" stop | grep -Fq acknowledged
    wait "$daemon_pid"
fi

echo "verify.sh: PASS $package $version $architecture (GLIBC_${max_glibc:-none})"
