#!/usr/bin/env bash
# Bundle the UOS 20 runtime closure for ruyiseek-ui into a package staging tree.

set -euo pipefail

STAGING="${1:?usage: bundle-libs.sh <staging-tree>}"
INSTALL_LIBS_DIR="/usr/lib/ruyiseek"
LIBS_DIR="$STAGING$INSTALL_LIBS_DIR"
HELPER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DYN_BIN="$STAGING/usr/bin/ruyiseek-ui"

if [ ! -x "$DYN_BIN" ]; then
    echo "bundle-libs.sh: binary not found: $DYN_BIN" >&2
    exit 1
fi

for command_name in readelf ldd ldconfig file; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "bundle-libs.sh: required existing tool is missing: $command_name" >&2
        exit 1
    fi
done

# A staging directory must never inherit libraries from a previous package.
rm -rf "$LIBS_DIR"
mkdir -p "$LIBS_DIR"

# Libraries opened directly by the executable are discovered through NEEDED.
# Slint/winit opens the X11 stack with dlopen, so those SONAMEs are explicit
# roots even though they do not appear in readelf -d output.
SEEDS=(
    libX11.so.6
    libxcb.so.1
    libXi.so.6
    libXcursor.so.1
    libX11-xcb.so.1
    libxkbcommon.so.0
    libxkbcommon-x11.so.0
    libxcb-xkb.so.1
    libfontconfig.so.1
    libfreetype.so.6
    libstdc++.so.6
    libgcc_s.so.1
)

LIB_SEARCH_DIR=/lib/x86_64-linux-gnu
if [ ! -d "$LIB_SEARCH_DIR" ]; then
    LIB_SEARCH_DIR=/usr/lib/x86_64-linux-gnu
fi

closure_file=$(mktemp)
trap 'rm -f "$closure_file"' EXIT
"$HELPER_DIR/recursive_ldd.sh" "$LIB_SEARCH_DIR" "${SEEDS[@]}" >"$closure_file"

copy_soname() {
    local soname="$1"
    local source_path="$2"
    local real_path real_name

    real_path=$(readlink -f "$source_path")
    if [ -z "$real_path" ] || [ ! -f "$real_path" ]; then
        echo "bundle-libs.sh: unresolved library target: $soname -> $source_path" >&2
        exit 1
    fi
    real_name=$(basename "$real_path")

    # Copy the real ELF object first, then recreate the SONAME link using a
    # relative target. cp -P alone would copy only a dangling symlink.
    if [ ! -e "$LIBS_DIR/$real_name" ]; then
        cp -a "$real_path" "$LIBS_DIR/$real_name"
    fi
    if [ "$soname" != "$real_name" ]; then
        ln -sfn "$real_name" "$LIBS_DIR/$soname"
    fi
}

while IFS=$'\t' read -r soname source_path; do
    [ -n "$soname" ] || continue
    # Keep the loader and glibc family supplied by the target's required
    # libc6 package. Mixing a private libc with a differently patched system
    # loader is unsupported and can assert before main().
    case "$soname" in
        libc.so.6|libdl.so.2|libm.so.6|libpthread.so.0|librt.so.1|ld-linux-x86-64.so.2)
            continue
            ;;
    esac
    copy_soname "$soname" "$source_path"
done <"$closure_file"

# The old UOS patchelf 0.10 corrupts this PIE. The binary must already carry
# the link-time RPATH from .cargo/config.toml and retain the system loader.
readelf -l "$DYN_BIN" | grep -Fq \
    '[Requesting program interpreter: /lib64/ld-linux-x86-64.so.2]'
readelf -d "$DYN_BIN" | grep -Fq \
    'Library rpath: [$ORIGIN/../lib/ruyiseek]'

broken_links=$(find -L "$LIBS_DIR" -maxdepth 1 -type l -print)
if [ -n "$broken_links" ]; then
    echo "bundle-libs.sh: package contains dangling runtime links:" >&2
    echo "$broken_links" >&2
    exit 1
fi

resolution=$(
    /lib64/ld-linux-x86-64.so.2 --inhibit-cache --list "$DYN_BIN" 2>&1
) || {
    echo "bundle-libs.sh: isolated loader resolution failed:" >&2
    echo "$resolution" >&2
    exit 1
}

# Non-glibc dependencies must resolve inside the package. The system loader
# and libc6 family are the only intentional host resolutions.
libs_dir_real=$(readlink -f "$LIBS_DIR")
while IFS=$'\t' read -r soname resolved_path; do
    [ -n "$resolved_path" ] || continue
    resolved_real=$(readlink -f "$resolved_path")
    case "$resolved_real" in
        "$libs_dir_real"/*) ;;
        *)
            case "$soname" in
                libc.so.6|libdl.so.2|libm.so.6|libpthread.so.0|librt.so.1|ld-linux-x86-64.so.2)
                    ;;
                *)
                    echo "bundle-libs.sh: dependency escaped package runtime: $soname -> $resolved_real" >&2
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

"$DYN_BIN" --demo-double-ctrl | grep -Fq '双击 Ctrl 已识别'

count=$(wc -l <"$closure_file")
size=$(du -sh "$LIBS_DIR" | awk '{print $1}')
echo "==> inspected $count libraries, bundled non-glibc closure ($size), isolated resolution passed"
