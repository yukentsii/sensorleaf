# Maintainer: SensorLeaf contributors
pkgname=sensorleaf
pkgver=0.1.0
pkgrel=1
pkgdesc="Hardware sensor monitor with a Sensor/Value/Min/Max tree (temperatures, voltages, power, clocks, fans)"
arch=('x86_64')
url="https://github.com/yukentsii/sensorleaf"
license=('MIT')
depends=('gcc-libs' 'glibc' 'libxkbcommon' 'wayland' 'libgl' 'hwdata' 'xdg-desktop-portal')
makedepends=('cargo')
optdepends=('ryzen_smu-dkms-git: per-core voltage on supported AMD APUs (AUR, third-party kernel module)')
install=sensorleaf.install
source=()

build() {
  cd "$startdir"
  export CARGO_TARGET_DIR="$srcdir/target"
  cargo build --release --locked
}

package() {
  cd "$startdir"
  local t="$srcdir/target/release"
  install -Dm755 "$t/sensorleaf"            "$pkgdir/usr/bin/sensorleaf"
  install -Dm755 "$t/sensorleaf-cli"        "$pkgdir/usr/bin/sensorleaf-cli"
  install -Dm755 "$t/sensorleaf-helper"     "$pkgdir/usr/lib/sensorleaf/sensorleaf-helper"
  install -Dm644 packaging/sensorleaf-helper.socket  "$pkgdir/usr/lib/systemd/system/sensorleaf-helper.socket"
  install -Dm644 packaging/sensorleaf-helper.service "$pkgdir/usr/lib/systemd/system/sensorleaf-helper.service"
  install -Dm644 packaging/sensorleaf-modules.conf   "$pkgdir/usr/lib/modules-load.d/sensorleaf.conf"
  install -Dm644 packaging/sensorleaf.desktop "$pkgdir/usr/share/applications/sensorleaf.desktop"
  install -Dm644 packaging/sensorleaf.svg     "$pkgdir/usr/share/icons/hicolor/scalable/apps/sensorleaf.svg"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
