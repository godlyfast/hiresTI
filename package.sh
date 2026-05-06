#!/bin/bash
set -euo pipefail

# ================= 配置区域 =================
APP_NAME="hiresti"
APP_ID="com.hiresti.player"
DISPLAY_NAME="HiresTI"
MAINTAINER="Eason <yelanxin@gmail.com>"
DESCRIPTION="High-Res Tidal Player for Linux with Bit-Perfect support."
LICENSE="GPL-3.0"
URL="https://github.com/yourrepo/hiresti"
# ===========================================

# ---- Supported targets ----
# Each entry: TARGET_NAME  BASE_IMAGE  DISTRO_FAMILY  PKG_TYPE  SUFFIX
TARGETS=(
    "ubuntu2404             ubuntu:24.04                deb   deb          ubuntu2404"
    "ubuntu2604             ubuntu:26.04                deb   deb          ubuntu2604"
    "debian12               debian:12                   deb   deb          debian12"
    "debian13               debian:13                   deb   deb          debian13"
    "fedora43               fedora:43                   rpm   rpm-fedora   fedora43"
    "fedora44               fedora:44                   rpm   rpm-fedora   fedora44"
    "archlinux              archlinux:latest            arch  arch         archlinux"
    "opensuse-tumbleweed    opensuse/tumbleweed:latest  suse  rpm-opensuse opensuse_tumbleweed"
)

TYPE="${1:-}"
VERSION="${2:-}"
PKG_JOBS="${HIRESTI_PKG_JOBS:-1}"
if ! [[ "$PKG_JOBS" =~ ^[0-9]+$ ]] || [ "$PKG_JOBS" -lt 1 ]; then
    PKG_JOBS=1
fi

# ---- Usage ----
print_usage() {
    echo "Usage: ./package.sh <target> <version>"
    echo ""
    echo "Local targets (build on this machine):"
    echo "  deb             Build .deb using host toolchain"
    echo "  rpm-fedora      Build Fedora .rpm using host toolchain"
    echo "  rpm-opensuse    Build openSUSE .rpm using host toolchain"
    echo "  arch            Build Arch .pkg.tar.zst using host toolchain"
    echo ""
    echo "Docker targets (build inside container for correct GLIBC):"
    echo "  ubuntu2404      Ubuntu 24.04 .deb"
    echo "  ubuntu2604      Ubuntu 26.04 .deb"
    echo "  debian12        Debian 12 .deb"
    echo "  debian13        Debian 13 .deb"
    echo "  fedora43        Fedora 43 .rpm"
    echo "  fedora44        Fedora 44 .rpm"
    echo "  archlinux              Arch Linux .pkg.tar.zst"
    echo "  opensuse-tumbleweed    openSUSE Tumbleweed .rpm"
    echo "  all                    Build all Docker targets"
    echo ""
    echo "Example: ./package.sh ubuntu2404 1.9.0beta1"
    echo "         ./package.sh all 1.9.0beta1"
    echo ""
    echo "Env:"
    echo "  HIRESTI_PKG_JOBS=N   parallel 'all' builds (default 1)"
}

if [ -z "$TYPE" ] || [ -z "$VERSION" ]; then
    print_usage
    exit 1
fi

# Compute DEB architecture string
if command -v dpkg-deb &>/dev/null; then
    DEB_ARCH="$(dpkg --print-architecture)"
else
    DEB_ARCH="$(uname -m | sed 's/x86_64/amd64/;s/aarch64/arm64/')"
fi

echo "🚀 Starting build process for $APP_NAME v$VERSION ($TYPE)..."

# Keep a canonical version file in repo root based on build argument.
echo "$VERSION" > version.txt
echo "🧾 Version file updated: version.txt -> $VERSION"

# Package version must not contain '-' (pacman parses '-' as pkgver/pkgrel
# boundary; rpm disallows it in Version; historic deb releases used the
# stripped form too). Keep $VERSION as-is for the in-app version string.
PKG_VERSION="${VERSION//-/}"
if [ "$PKG_VERSION" != "$VERSION" ]; then
    echo "🧾 Packaging version normalized: $VERSION -> $PKG_VERSION"
fi


# ====================================================================
#  Docker-based builds
# ====================================================================

find_target() {
    local name="$1"
    for t in "${TARGETS[@]}"; do
        local fields=($t)
        if [ "${fields[0]}" = "$name" ]; then
            echo "$t"
            return 0
        fi
    done
    return 1
}

docker_build_target() {
    local target_name="$1"
    local target_line
    target_line="$(find_target "$target_name")" || {
        echo "Error: unknown target '$target_name'"
        exit 1
    }
    local fields=($target_line)
    local base_image="${fields[1]}"
    local distro_family="${fields[2]}"
    local pkg_type="${fields[3]}"
    local suffix="${fields[4]}"
    local image_tag="hiresti-builder-${target_name}"

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  Building: $target_name ($base_image)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    docker build -t "$image_tag" \
        --build-arg BASE_IMAGE="$base_image" \
        --build-arg DISTRO_FAMILY="$distro_family" \
        --build-arg DISTRO_ID="$target_name" \
        -f Dockerfile.build . || {
        echo "❌ Docker image build failed for $target_name"
        return 1
    }

    mkdir -p dist

    docker run --rm \
        -v "$(pwd)/dist:/output" \
        -e VERSION="$VERSION" \
        -e PKG_TYPE="$pkg_type" \
        -e PKG_SUFFIX="$suffix" \
        "$image_tag" || {
        echo "❌ Package build failed for $target_name"
        return 1
    }

    echo "✅ $target_name build complete"
}

# Handle Docker targets
case "$TYPE" in
    ubuntu2404|ubuntu2604|debian12|debian13|fedora43|fedora44|archlinux|opensuse-tumbleweed)
        docker_build_target "$TYPE"
        echo ""
        echo "🎉 Build Complete! Packages in dist/:"
        ls -lh dist/ 2>/dev/null
        exit 0
        ;;
    all)
        mkdir -p dist
        failed=()
        if [ "$PKG_JOBS" -le 1 ]; then
            for t in "${TARGETS[@]}"; do
                local_fields=($t)
                target_name="${local_fields[0]}"
                docker_build_target "$target_name" || failed+=("$target_name")
            done
        else
            echo "🔀 Parallel multi-target build (HIRESTI_PKG_JOBS=$PKG_JOBS)"
            echo "   Per-target output streamed to dist/build-<target>.log"
            faildir="$(mktemp -d)"
            trap 'rm -rf "$faildir"' EXIT
            for t in "${TARGETS[@]}"; do
                local_fields=($t)
                target_name="${local_fields[0]}"
                while [ "$(jobs -rp | wc -l)" -ge "$PKG_JOBS" ]; do
                    wait -n 2>/dev/null || true
                done
                (
                    log="dist/build-${target_name}.log"
                    echo "[start] ${target_name}  → ${log}"
                    if docker_build_target "${target_name}" >"$log" 2>&1; then
                        echo "[ok]    ${target_name}"
                    else
                        echo "[FAIL]  ${target_name}  → ${log}"
                        touch "$faildir/${target_name}"
                    fi
                ) &
            done
            wait
            for f in "$faildir"/*; do
                [ -e "$f" ] || continue
                failed+=("$(basename "$f")")
            done
        fi
        echo ""
        if [ "${#failed[@]}" -ne 0 ]; then
            echo "❌ Failed targets: ${failed[*]}"
            exit 1
        fi
        echo "🎉 All builds complete! Packages in dist/:"
        ls -lh dist/ 2>/dev/null
        exit 0
        ;;
esac


# ====================================================================
#  Local builds
# ====================================================================

BUILD_ROOT="build_tmp/${TYPE}"
rm -rf "$BUILD_ROOT"
mkdir -p "$BUILD_ROOT"

BIN_DIR="$BUILD_ROOT/usr/bin"
APP_DIR="$BUILD_ROOT/usr/share/applications"
SYSTEM_ICON_DIR="$BUILD_ROOT/usr/share/icons"
LICENSE_DIR="$BUILD_ROOT/usr/share/licenses/$APP_NAME"
UDEV_DIR="$BUILD_ROOT/usr/lib/udev/rules.d"

mkdir -p "$BIN_DIR" "$APP_DIR" "$SYSTEM_ICON_DIR" "$LICENSE_DIR" "$UDEV_DIR"

# udev rule
cat <<'UDEV_EOF' > "$UDEV_DIR/99-hiresti-usb-audio.rules"
# HiresTI USB Rawlink - grant logged-in user access to USB audio devices
SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_device", ENV{ID_USB_INTERFACES}=="*:01????:*", TAG+="uaccess"
UDEV_EOF

# 1. Build / install the Rust binary
PREBUILT_DIR="rust_out"
PREBUILT_BIN="${PREBUILT_DIR}/hiresti"
if [ -f "$PREBUILT_BIN" ]; then
    echo "✅ Using pre-built Rust binary"
    install -Dm755 "$PREBUILT_BIN" "$BIN_DIR/$APP_NAME"
elif [ -f "src_rust/Cargo.toml" ]; then
    if command -v cargo &> /dev/null; then
        echo "🦀 Building Rust binary..."
        cargo build --manifest-path src_rust/Cargo.toml --release --bin hiresti
        BIN_PATH="src_rust/target/release/hiresti"
        if [ ! -f "$BIN_PATH" ]; then
            echo "Error: cargo build finished but $BIN_PATH not found."
            exit 1
        fi
        install -Dm755 "$BIN_PATH" "$BIN_DIR/$APP_NAME"
    else
        echo "Error: 'cargo' not found and no pre-built binary at $PREBUILT_BIN."
        exit 1
    fi
else
    echo "Error: src_rust/Cargo.toml not found."
    exit 1
fi

# 2. Icons
echo "🎨 Installing icons..."
if [ -d "icons/hicolor" ]; then
    cp -r icons/hicolor "$SYSTEM_ICON_DIR/"
fi
if [ -f "icons/hicolor/128x128/apps/hiresti.png" ]; then
    install -Dm644 "icons/hicolor/128x128/apps/hiresti.png" \
        "$SYSTEM_ICON_DIR/hicolor/128x128/apps/$APP_ID.png"
fi
if [ -f "icons/hicolor/scalable/apps/hiresti.svg" ]; then
    install -Dm644 "icons/hicolor/scalable/apps/hiresti.svg" \
        "$SYSTEM_ICON_DIR/hicolor/scalable/apps/$APP_ID.svg"
fi

# 3. Desktop entry
cat <<EOF > "$APP_DIR/$APP_ID.desktop"
[Desktop Entry]
Name=$DISPLAY_NAME
Comment=$DESCRIPTION
Exec=/usr/bin/$APP_NAME
Icon=$APP_NAME
Terminal=false
Type=Application
Categories=AudioVideo;Audio;Player;Music;
StartupWMClass=$DISPLAY_NAME
EOF

# 4. License
if [ -f "LICENSE" ]; then
    install -Dm644 LICENSE "$LICENSE_DIR/LICENSE"
fi

# ================= 输出包名后缀 =================
PKG_SUFFIX="${PKG_SUFFIX:-}"

# ================= 分支处理 =================

build_deb() {
    echo "📦 Building .deb package..."
    mkdir -p "$BUILD_ROOT/DEBIAN"
    cat <<EOF > "$BUILD_ROOT/DEBIAN/control"
Package: $APP_NAME
Version: $PKG_VERSION
Section: sound
Priority: optional
Architecture: $DEB_ARCH
Depends: libgtk-4-1, libadwaita-1-0, libpipewire-0.3-0, libpulse0, libasound2, libusb-1.0-0, libssl3 | libssl1.1
Maintainer: $MAINTAINER
Description: $DESCRIPTION
 $DISPLAY_NAME is a desktop client for Tidal focusing on High-Res audio.
EOF
    cat <<'POSTINST_EOF' > "$BUILD_ROOT/DEBIAN/postinst"
#!/bin/sh
set -e
if [ "$1" = "configure" ]; then
    udevadm control --reload-rules 2>/dev/null || true
    udevadm trigger --subsystem-match=usb 2>/dev/null || true
fi
exit 0
POSTINST_EOF
    chmod 755 "$BUILD_ROOT/DEBIAN/postinst"
    mkdir -p dist

    if [ -n "$PKG_SUFFIX" ]; then
        local deb_name="${APP_NAME}_${PKG_VERSION}_${DEB_ARCH}_${PKG_SUFFIX}.deb"
    else
        local deb_name="${APP_NAME}_${PKG_VERSION}_${DEB_ARCH}.deb"
    fi
    dpkg-deb --build "$BUILD_ROOT" "dist/${deb_name}"
    echo "✅ DEB created: dist/${deb_name}"
}

build_rpm_variant() {
    local variant="$1"
    local dist_tag="$2"
    local requires="$3"
    local arch spec_file rpm_build_root

    arch="$(uname -m)"
    rpm_build_root="$(pwd)/build_rpmbuild_${variant}"
    rm -rf "$rpm_build_root"
    mkdir -p "$rpm_build_root"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
    spec_file="$rpm_build_root/SPECS/$APP_NAME-${variant}.spec"

    cat <<EOF > "$spec_file"
Name:           $APP_NAME
Version:        $PKG_VERSION
Release:        1%{?dist}
Summary:        $DESCRIPTION (${variant})
License:        $LICENSE
BuildArch:      $arch
AutoReq:        no
AutoProv:       no
Requires:       $requires

%description
$DISPLAY_NAME is a desktop client for Tidal (${variant} build).

%prep
%build
%install
cp -r $(pwd)/$BUILD_ROOT/* %{buildroot}

%post
udevadm control --reload-rules 2>/dev/null || true
udevadm trigger --subsystem-match=usb 2>/dev/null || true

%files
/usr/bin/$APP_NAME
/usr/share/applications/$APP_ID.desktop
/usr/share/icons/*
/usr/share/licenses/$APP_NAME/LICENSE
/usr/lib/udev/rules.d/99-hiresti-usb-audio.rules

%changelog
* $(date "+%a %b %d %Y") $MAINTAINER - $PKG_VERSION-1
- Automated ${variant} build
EOF

    rpmbuild -bb "$spec_file" \
        --define "_topdir $rpm_build_root" \
        --define "dist .${dist_tag}"

    mkdir -p dist
    if [ -n "$PKG_SUFFIX" ]; then
        for rpm_file in "$rpm_build_root"/RPMS/"$arch"/${APP_NAME}-${PKG_VERSION}-1*.${arch}.rpm; do
            local base
            base="$(basename "$rpm_file")"
            local newname="${base%.rpm}_${PKG_SUFFIX}.rpm"
            mv "$rpm_file" "dist/${newname}"
            echo "✅ RPM created: dist/${newname}"
        done
    else
        mv "$rpm_build_root"/RPMS/"$arch"/${APP_NAME}-${PKG_VERSION}-1*.${arch}.rpm "dist/"
        echo "✅ RPM created (${variant})."
    fi
}

build_arch_package() {
    local arch pkg_rel pkg_ver_rel pkg_file pkg_root pkg_size build_ts
    arch="$(uname -m)"
    pkg_rel="1"
    pkg_ver_rel="${PKG_VERSION}-${pkg_rel}"
    pkg_root="$(pwd)/build_archpkg/pkgroot"
    rm -rf "$(pwd)/build_archpkg"
    mkdir -p "$pkg_root"
    cp -a "$BUILD_ROOT"/. "$pkg_root"/

    pkg_size="$(du -sb "$pkg_root" | awk '{print $1}')"
    build_ts="$(date +%s)"

    cat <<EOF > "$pkg_root/.PKGINFO"
pkgname = $APP_NAME
pkgbase = $APP_NAME
pkgver = $pkg_ver_rel
pkgdesc = $DESCRIPTION
url = $URL
builddate = $build_ts
packager = $MAINTAINER
size = $pkg_size
arch = $arch
license = $LICENSE
depend = gtk4
depend = libadwaita
depend = pipewire
depend = libpulse
depend = alsa-lib
depend = libusb
depend = openssl
EOF

    cat <<'INSTALL_EOF' > "$pkg_root/.INSTALL"
post_install() {
    udevadm control --reload-rules 2>/dev/null || true
    udevadm trigger --subsystem-match=usb 2>/dev/null || true
}
post_upgrade() {
    post_install
}
INSTALL_EOF

    if [ -n "$PKG_SUFFIX" ]; then
        pkg_file="dist/${APP_NAME}-${pkg_ver_rel}-${arch}_${PKG_SUFFIX}.pkg.tar.zst"
    else
        pkg_file="dist/${APP_NAME}-${pkg_ver_rel}-${arch}.pkg.tar.zst"
    fi

    mkdir -p dist
    tar --sort=name --mtime="@$build_ts" --owner=0 --group=0 --numeric-owner \
        -C "$pkg_root" -I 'zstd -19 -T0' -cf "$pkg_file" .PKGINFO .INSTALL usr
    echo "✅ Arch package created: $pkg_file"
}


# ---- Dispatch local builds ----
FEDORA_REQUIRES="gtk4, libadwaita, pipewire, pulseaudio-libs, alsa-lib, libusb1, openssl-libs"
OPENSUSE_REQUIRES="libgtk-4-1, libadwaita-1-0, pipewire, libpulse0, libasound2, libusb-1_0-0, libopenssl3"

case "$TYPE" in
    deb)
        build_deb
        ;;
    rpm-fedora)
        build_rpm_variant "fedora" "fedora" "$FEDORA_REQUIRES"
        ;;
    rpm-opensuse)
        build_rpm_variant "opensuse" "opensuse" "$OPENSUSE_REQUIRES"
        ;;
    arch)
        build_arch_package
        ;;
    *)
        echo "Error: unsupported local type '$TYPE'. Use deb | rpm-fedora | rpm-opensuse | arch"
        echo "  Or use a Docker target: ubuntu2404 | ubuntu2604 | debian12 | debian13 | fedora43 | fedora44 | archlinux | opensuse-tumbleweed | all"
        exit 1
        ;;
esac

rm -rf "$BUILD_ROOT"
echo "🎉 Build Complete!"
