#!/usr/bin/env bash
# Build a UOS 20-compatible RuyiSeek binary Debian package.
#
# Usage:
#   packaging/deb/build.sh
#   packaging/deb/build.sh --skip-build
#   packaging/deb/build.sh --output DIR
#
# No dependency is installed by this script. It only uses tools already on the
# host and fails with a precise message when one is missing.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
DIST="$ROOT_DIR/dist"
SKIP_BUILD=0
TARGET_STATIC=x86_64-unknown-linux-musl
TARGET_DYN=x86_64-unknown-linux-gnu

usage() {
    sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
    exit "${1:-0}"
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --skip-build)
            SKIP_BUILD=1
            shift
            ;;
        --output)
            [ "$#" -ge 2 ] || usage 1
            DIST="$2"
            shift 2
            ;;
        -h|--help)
            usage 0
            ;;
        *)
            echo "build.sh: unknown option: $1" >&2
            usage 1
            ;;
    esac
done

required_commands=(cargo rustc cc dpkg-deb gzip readelf file install)
for command_name in "${required_commands[@]}"; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "build.sh: required existing tool is missing: $command_name" >&2
        exit 1
    fi
done

if ! rustup target list --installed 2>/dev/null | grep -Fxq "$TARGET_STATIC"; then
    echo "build.sh: Rust target $TARGET_STATIC is not already installed" >&2
    echo "build.sh: refusing to install dependencies automatically" >&2
    exit 1
fi

mkdir -p "$DIST" "$ROOT_DIR/target"

# dpkg-deb 1.19 on UOS honors SOURCE_DATE_EPOCH for ar/tar member mtimes.
# Pin it to the source revision so temporary staging directory timestamps do
# not make byte-identical source and binaries produce different packages.
if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
    SOURCE_DATE_EPOCH=$(git -C "$ROOT_DIR" log -1 --format=%ct 2>/dev/null || true)
    if [ -z "$SOURCE_DATE_EPOCH" ]; then
        SOURCE_DATE_EPOCH=$(stat -c %Y "$ROOT_DIR/Cargo.lock")
    fi
fi
export SOURCE_DATE_EPOCH

if [ "$SKIP_BUILD" -eq 0 ]; then
    echo "==> building static CLI and daemon with Rust's self-contained musl target"
    (
        cd "$ROOT_DIR"
        cargo build --release --locked --target "$TARGET_STATIC" \
            -p ruyi-cli -p ruyiseekd
    )

    echo "==> building the glibc 2.28-compatible desktop UI"
    (
        cd "$ROOT_DIR"
        cargo build --release --locked --target "$TARGET_DYN" \
            -p ruyiseek-ui
    )
fi

static_bin_dir="$ROOT_DIR/target/$TARGET_STATIC/release"
dynamic_bin_dir="$ROOT_DIR/target/$TARGET_DYN/release"
for binary in \
    "$static_bin_dir/ruyi" \
    "$static_bin_dir/ruyiseekd" \
    "$dynamic_bin_dir/ruyiseek-ui"; do
    if [ ! -x "$binary" ]; then
        echo "build.sh: missing built binary: $binary" >&2
        exit 1
    fi
done

staging_parent=$(mktemp -d "$ROOT_DIR/target/deb-staging.XXXXXX")
staging="$staging_parent/root"
cleanup() {
    rm -rf "$staging_parent"
}
trap cleanup EXIT

mkdir -p "$staging"
cp -a "$SCRIPT_DIR/DEBIAN" "$staging/DEBIAN"
cp -a "$SCRIPT_DIR/etc" "$staging/etc"
cp -a "$SCRIPT_DIR/usr" "$staging/usr"

# The source template may contain ignored leftovers from older build scripts.
# Never let those files influence a new package.
rm -rf "$staging/usr/bin" "$staging/usr/lib/ruyiseek" \
    "$staging/usr/share/icons/hicolor"
mkdir -p "$staging/usr/bin"
install -m 0755 "$static_bin_dir/ruyi" "$staging/usr/bin/ruyi"
install -m 0755 "$static_bin_dir/ruyiseekd" "$staging/usr/bin/ruyiseekd"
install -m 0755 "$dynamic_bin_dir/ruyiseek-ui" "$staging/usr/bin/ruyiseek-ui"

"$SCRIPT_DIR/bundle-libs.sh" "$staging"

icon_name=io.github.ethanbird.RuyiSeek
icon_svg="$ROOT_DIR/packaging/icons/$icon_name.svg"
install -d "$staging/usr/share/icons/hicolor/scalable/apps"
install -m 0644 "$icon_svg" \
    "$staging/usr/share/icons/hicolor/scalable/apps/$icon_name.svg"
if command -v rsvg-convert >/dev/null 2>&1; then
    for size in 48 64 128 256; do
        install -d "$staging/usr/share/icons/hicolor/${size}x${size}/apps"
        rsvg-convert -w "$size" -h "$size" "$icon_svg" \
            >"$staging/usr/share/icons/hicolor/${size}x${size}/apps/$icon_name.png"
    done
else
    echo "build.sh: rsvg-convert is absent; shipping the canonical SVG only" >&2
fi

chmod 0755 "$staging/DEBIAN/postinst" "$staging/DEBIAN/prerm" \
    "$staging/DEBIAN/postrm"
install -d "$staging/usr/share/doc/ruyiseek"
gzip -9n -c "$staging/DEBIAN/changelog" \
    >"$staging/usr/share/doc/ruyiseek/changelog.gz"
install -m 0644 "$staging/DEBIAN/copyright" \
    "$staging/usr/share/doc/ruyiseek/copyright"

installed_size=$(du -sk --exclude=DEBIAN "$staging" | cut -f1)
if grep -q '^Installed-Size:' "$staging/DEBIAN/control"; then
    sed -i "s/^Installed-Size:.*/Installed-Size: $installed_size/" \
        "$staging/DEBIAN/control"
else
    printf 'Installed-Size: %s\n' "$installed_size" \
        >>"$staging/DEBIAN/control"
fi

package=$(awk '/^Package:/ {print $2; exit}' "$staging/DEBIAN/control")
version=$(awk '/^Version:/ {print $2; exit}' "$staging/DEBIAN/control")
architecture=$(awk '/^Architecture:/ {print $2; exit}' "$staging/DEBIAN/control")
deb_file="$DIST/${package}_${version}_${architecture}.deb"
deb_tmp="$deb_file.tmp"

rm -f "$deb_tmp"
echo "==> building $deb_file"
dpkg-deb --build --root-owner-group "$staging" "$deb_tmp"
mv -f "$deb_tmp" "$deb_file"

"$SCRIPT_DIR/verify.sh" --skip-daemon "$deb_file"
(
    cd "$DIST"
    sha256sum "$(basename "$deb_file")" \
        >"$(basename "$deb_file").sha256"
)

echo "==> package: $deb_file"
echo "==> checksum: $deb_file.sha256"
