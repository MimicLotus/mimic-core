# Maintainer: MimicOS Core Team <dev@mimicos.org>
pkgname=mimic
pkgver=4.0.0
pkgrel=1
pkgdesc="🦖 Unified Package Engine, Hermetic Builder & AI Mentor for Arch Linux"
arch=('x86_64' 'x86_64_v3' 'x86_64_v4')
url="https://github.com/mimicos/mimic"
license=('GPL-3.0-or-later')
depends=(
    'glibc'
    'gcc-libs'
    'pacman'
    'bubblewrap'
    'sccache'
    'mold'
    'libalpm.so=16'
    'openssl'
)
provides=(
    'aur-helper'
    'makepkg'
)
source=()
sha256sums=()

build() {
    cd "${startdir:-${srcdir}/..}" || exit 1
    export RUSTFLAGS="-C target-cpu=native -C opt-level=3"
    cargo build --release --locked --workspace
}

package() {
    cd "${startdir:-${srcdir}/..}" || exit 1

    # Install binaries
    install -Dm755 "target/release/mimic" "${pkgdir}/usr/bin/mimic"
    install -Dm755 "target/release/mimic-brain" "${pkgdir}/usr/bin/mimic-brain"

    # Install systemd socket activation units
    install -Dm644 "packaging/mimic-brain.socket" "${pkgdir}/usr/lib/systemd/system/mimic-brain.socket"
    install -Dm644 "packaging/mimic-brain.service" "${pkgdir}/usr/lib/systemd/system/mimic-brain.service"

    # Shared cache directories
    install -dm1777 "${pkgdir}/var/cache/mimic/sccache"
    install -dm755 "${pkgdir}/var/cache/mimic/pkg"

    # Makepkg compatibility symlink
    ln -sf mimic "${pkgdir}/usr/bin/mimic-build"
}
