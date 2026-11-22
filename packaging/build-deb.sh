#!/usr/bin/env bash
# Build a Debian binary package for CamView.
#
# Usage:
#   ./packaging/build-deb.sh
#   CARGO_BUILD_JOBS=1 ./packaging/build-deb.sh
#   OUT_DIR=/tmp/camview-deb ./packaging/build-deb.sh
#
# The package is written to dist/ by default. It is not committed to Git;
# GitHub Releases publish it as a downloadable asset.

set -euo pipefail
umask 022

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PKG=camview
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
if [[ -z "$VERSION" ]]; then
    echo "error: could not read the package version from Cargo.toml" >&2
    exit 1
fi

DEB_REVISION="${DEB_REVISION:-1}"
DEB_VERSION="${VERSION}-${DEB_REVISION}"
DEB_ARCH="${DEB_ARCH:-$(dpkg --print-architecture)}"
OUT_DIR="${OUT_DIR:-$ROOT/dist}"
CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/camview-deb.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

STAGE="$WORK/root"
mkdir -p "$STAGE/DEBIAN" "$OUT_DIR"

echo "==> Building CamView ${VERSION} (${DEB_ARCH})"
CARGO_BUILD_JOBS="$CARGO_BUILD_JOBS" cargo build --release --locked
BIN="$ROOT/target/release/camview"
if [[ ! -x "$BIN" ]]; then
    echo "error: release binary not found at $BIN" >&2
    exit 1
fi

install -Dm755 "$BIN" "$STAGE/usr/bin/$PKG"

install -d "$STAGE/usr/share/applications"
sed 's|@BINDIR@|/usr/bin|g' data/camview.desktop.in \
    > "$STAGE/usr/share/applications/camview.desktop"
chmod 0644 "$STAGE/usr/share/applications/camview.desktop"

install -Dm644 data/camview.svg \
    "$STAGE/usr/share/icons/hicolor/scalable/apps/camview.svg"
install -Dm644 data/io.github.orozcorgint.camview.metainfo.xml \
    "$STAGE/usr/share/metainfo/io.github.orozcorgint.camview.metainfo.xml"

install -Dm644 data/camview.1 "$STAGE/usr/share/man/man1/camview.1"
gzip -9n "$STAGE/usr/share/man/man1/camview.1"

install -Dm644 README.md "$STAGE/usr/share/doc/$PKG/README.md"
install -Dm644 SECURITY.md "$STAGE/usr/share/doc/$PKG/SECURITY.md"
install -Dm644 THIRD_PARTY.md "$STAGE/usr/share/doc/$PKG/THIRD_PARTY.md"
install -Dm644 packaging/deb/copyright "$STAGE/usr/share/doc/$PKG/copyright"
gzip -9n -c CHANGELOG.md > "$STAGE/usr/share/doc/$PKG/changelog.gz"

{
    echo "$PKG ($DEB_VERSION) unstable; urgency=medium"
    echo
    echo "  * Package release."
    echo
    echo " -- orozcorgint <orozcorgint@users.noreply.github.com>  $(LC_ALL=C date -R)"
} > "$WORK/changelog.Debian"
gzip -9n "$WORK/changelog.Debian"
install -Dm644 "$WORK/changelog.Debian.gz" \
    "$STAGE/usr/share/doc/$PKG/changelog.Debian.gz"

install -m755 packaging/deb/postinst "$STAGE/DEBIAN/postinst"
install -m755 packaging/deb/postrm "$STAGE/DEBIAN/postrm"

# Ask dpkg-shlibdeps for the shared-library dependencies of the binary. This
# covers libc, libgcc and libmpv automatically; the remaining X11/GL/Wayland
# libraries are loaded dynamically by eframe/winit and are listed explicitly.
mkdir -p "$WORK/shlibs/debian"
cat > "$WORK/shlibs/debian/control" <<'EOF'
Source: camview
Section: video
Priority: optional
Maintainer: orozcorgint <orozcorgint@users.noreply.github.com>
Build-Depends: debhelper-compat (= 13)
Standards-Version: 4.7.0
Homepage: https://github.com/orozcorgint/camview

Package: camview
Architecture: any
Description: lightweight RTSP camera viewer and recorder
 Package build helper.
EOF
SHLIBS="$(cd "$WORK/shlibs" && dpkg-shlibdeps -O -e"$STAGE/usr/bin/$PKG" 2>/dev/null \
    | sed -n 's/^shlibs:Depends=//p')"
if [[ -z "$SHLIBS" ]]; then
    echo "error: dpkg-shlibdeps did not return any dependencies" >&2
    exit 1
fi

RUNTIME_DEPENDS="socat, libgl1, libegl1, libx11-6, libxkbcommon0, libxkbcommon-x11-0, libxcursor1, libxrandr2, libxi6, libwayland-client0, libwayland-cursor0, libwayland-egl1, libxcb1, libxcb-render0, libxcb-shape0, libxcb-xfixes0"
DEPENDS="$SHLIBS, $RUNTIME_DEPENDS"
RECOMMENDS="ffmpeg, libva2, intel-media-va-driver | i965-va-driver"
SUGGESTS="xdotool, libnotify-bin"
INSTALLED_SIZE="$(du -sk "$STAGE" | cut -f1)"

# Use a delimiter that cannot occur in the package dependency fields.
sed \
    -e "s#@VERSION@#$DEB_VERSION#g" \
    -e "s#@ARCH@#$DEB_ARCH#g" \
    -e "s#@INSTALLED_SIZE@#$INSTALLED_SIZE#g" \
    -e "s#@DEPENDS@#$DEPENDS#g" \
    -e "s#@RECOMMENDS@#$RECOMMENDS#g" \
    -e "s#@SUGGESTS@#$SUGGESTS#g" \
    packaging/deb/control.in > "$STAGE/DEBIAN/control"

# Validate the desktop entry and AppStream metadata when the tools are present.
if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate "$STAGE/usr/share/applications/camview.desktop"
fi
if command -v xmllint >/dev/null 2>&1; then
    xmllint --noout data/io.github.orozcorgint.camview.metainfo.xml
fi
if command -v appstreamcli >/dev/null 2>&1; then
    appstreamcli validate --no-net data/io.github.orozcorgint.camview.metainfo.xml
fi
if command -v groff >/dev/null 2>&1; then
    groff -man -Tutf8 data/camview.1 >/dev/null
fi

# Debian package checksums.
(
    cd "$STAGE"
    find usr -type f -print0 | sort -z | xargs -0 md5sum > DEBIAN/md5sums
)

PKG_FILE="$OUT_DIR/${PKG}_${DEB_VERSION}_${DEB_ARCH}.deb"
rm -f "$PKG_FILE"
dpkg-deb --build --root-owner-group -Zxz "$STAGE" "$PKG_FILE"

echo
echo "==> Package: $PKG_FILE"
dpkg-deb --info "$PKG_FILE" | sed -n '1,24p'
echo
sha256sum "$PKG_FILE"
