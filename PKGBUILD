# Maintainer: Zespół OKsavi Linux <dev@oksavi.org>
pkgname=okay-de
pkgver=1.0.0
pkgrel=1
pkgdesc="Ultra-lekkie, nowoczesne środowisko graficzne Wayland dla OKsavi Linux oparte na labwc"
arch=('any')
url="https://github.com/oksavi/okay-de"
license=('GPL-3.0-or-later')
depends=(
    'labwc'
    'swaybg'
    'pcmanfm'
    'fuzzel'
    'foot'
    'mako'
    'swaylock'
    'grim'
    'slurp'
    'wl-clipboard'
    'cliphist'
    'brightnessctl'
    'wireplumber'
    'pipewire-pulse'
    'papirus-icon-theme'
    'adwaita-icon-theme'
    'xdg-user-dirs'
    'gtk3'
    'gtk-layer-shell'
    'python'
    'python-gobject'
    'wlr-randr'
)
optdepends=(
    'wlrctl: opcjonalne narzędzie CLI dla wlroots (z AUR)'
    'util-linux: obsługa zramctl dla zarządcy pamięci oksavi-ghost-runner'
    'pavucontrol: zaawansowany mikser dźwięku dla PipeWire'
    'networkmanager: zarządzanie połączeniami Wi-Fi i sieciowymi w okay-settings'
    'whitesur-icon-theme: alternatywny minimalistyczny motyw ikon'
)
provides=('wayland-desktop-environment')

package() {
    cd "${startdir}" || exit 1
    make DESTDIR="${pkgdir}" PREFIX=/usr install
}
