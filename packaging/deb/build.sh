#!/usr/bin/env bash
# Build a .deb package of RuyiSeek from the current source tree.
#
# Usage:
#   packaging/deb/build.sh                    # build with default settings
#   packaging/deb/build.sh --skip-build       # skip cargo build, use existing musl release/
#   packaging/deb/build.sh --output DIR       # write .deb into DIR (default: dist/)
#   packaging/deb/build.sh --target TRIPLE    # override cargo target (default: x86_64-unknown-linux-musl)
#
# Requirements: cargo (>= 1.75), rustc (>= 1.75), fakeroot, dpkg-deb, gzip.
# The script intentionally does NOT need debhelper or a Debian source tree —
# it builds a single binary .deb directly with dpkg-deb.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
STAGING="$ROOT_DIR/packaging/deb"
DIST="$ROOT_DIR/dist"
SKIP_BUILD=0
TARGET="x86_64-unknown-linux-musl"

usage() {
    sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
    exit 1
}

while [ $# -gt 0 ]; do
    case "$1" in
        --skip-build) SKIP_BUILD=1; shift ;;
        --output) DIST="$2"; shift 2 ;;
        --target) TARGET="$2"; shift 2 ;;
        -h|--help) usage ;;
        *) echo "unknown option: $1" >&2; usage ;;
    esac
done

mkdir -p "$DIST"

if [ "$SKIP_BUILD" = 0 ]; then
    echo "==> cargo build --release --target $TARGET --workspace"
    (cd "$ROOT_DIR" && cargo build --release --target "$TARGET" --workspace)
fi

BIN_DIR="$ROOT_DIR/target/$TARGET/release"
for bin in ruyi ruyiseekd ruyiseek-ui; do
    if [ ! -x "$BIN_DIR/$bin" ]; then
        echo "missing binary: $BIN_DIR/$bin" >&2
        exit 1
    fi
done

echo "==> refreshing staging tree"
rm -rf "$STAGING/usr/bin"
mkdir -p "$STAGING/usr/bin"
# Strip dev/build files that should not ship in the .deb. Only matches
# files at the *immediate* depth inside $STAGING; nested files like
# usr/share/doc/ruyiseek/README are preserved. This explicitly skips
# build.sh itself so it does not delete its own source file, and skips
# shim.c/shim.o so the source files for the link-time stubs do not
# ship in the binary package.
find "$STAGING" -maxdepth 1 -mindepth 1 \
    -not -name 'build.sh' \
    \( -name '*.sh' -o -name '*.md' -o -name 'README*' -o -name 'shim.c' -o -name 'shim.o' \) -delete || true
install -m 0755 "$BIN_DIR/ruyiseekd"  "$STAGING/usr/bin/ruyiseekd"
install -m 0755 "$BIN_DIR/ruyiseek-ui" "$STAGING/usr/bin/ruyiseek-ui"
install -m 0755 "$BIN_DIR/ruyi"       "$STAGING/usr/bin/ruyi"

# Make sure maintainer scripts are executable
chmod 0755 "$STAGING/DEBIAN/postinst" \
          "$STAGING/DEBIAN/prerm" \
          "$STAGING/DEBIAN/postrm"

# Restore the source man page if it was compressed on a previous run
[ -f "$STAGING/usr/share/man/man1/ruyiseek.1" ] || \
    gunzip -kf "$STAGING/usr/share/man/man1/ruyiseek.1.gz" 2>/dev/null || true

# Compress changelog and man page
gzip -9kf "$STAGING/DEBIAN/changelog" -c > "$STAGING/usr/share/doc/ruyiseek/changelog.gz"
cp "$STAGING/DEBIAN/copyright" "$STAGING/usr/share/doc/ruyiseek/copyright"
gzip -9f "$STAGING/usr/share/man/man1/ruyiseek.1"

# Compute Installed-Size from staged file sizes (KiB)
INSTALLED_SIZE=$(du -sk "$STAGING" | cut -f1)
if grep -q '^Installed-Size:' "$STAGING/DEBIAN/control"; then
    sed -i "s/^Installed-Size:.*/Installed-Size: $INSTALLED_SIZE/" "$STAGING/DEBIAN/control"
else
    echo "Installed-Size: $INSTALLED_SIZE" >> "$STAGING/DEBIAN/control"
fi

# Extract metadata
PKG=$(grep -E '^Package:' "$STAGING/DEBIAN/control" | head -1 | awk '{print $2}')
VER=$(grep -E '^Version:' "$STAGING/DEBIAN/control" | head -1 | awk '{print $2}')
ARCH=$(grep -E '^Architecture:' "$STAGING/DEBIAN/control" | head -1 | awk '{print $2}')
DEB_FILE="$DIST/${PKG}_${VER}_${ARCH}.deb"

echo "==> building $DEB_FILE"
fakeroot dpkg-deb --build --root-owner-group "$STAGING" "$DEB_FILE"

echo "==> verifying"
dpkg-deb -I "$DEB_FILE" | sed 's/^/    /'
echo "    ----"
dpkg-deb -c "$DEB_FILE" | sed 's/^/    /'

echo "==> done: $DEB_FILE"