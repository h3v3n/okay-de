# Maintainer: Zespół OKsavi Linux <dev@oksavi.org>
pkgname=okay-de
pkgver=1.1.0
pkgrel=1
pkgdesc="Ultra-lekkie, nowoczesne środowisko graficzne Wayland dla OKsavi Linux oparte na Wayfire"
arch=('any')
url="https://github.com/h3v3n/okay-de"
license=('GPL-3.0-or-later')
depends=(
    'wayfire'
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
    'librsvg'
    'cairo'
    'pango'
    'wlr-randr'
)
optdepends=(
    'pixdecor: zaokrąglone narożniki okien i rozszerzone dekoracje SSD dla Wayfire (z AUR lub budowane przez install.sh)'
    'wayfire-plugins-extra: dodatkowe efekty i wtyczki dla Wayfire (z AUR)'
    'wlrctl: opcjonalne narzędzie CLI dla wlroots (z AUR)'
    'pavucontrol: zaawansowany mikser dźwięku dla PipeWire'
    'networkmanager: zarządzanie połączeniami Wi-Fi i sieciowymi w okay-settings'
    'whitesur-icon-theme: alternatywny minimalistyczny motyw ikon'
)
provides=('wayland-desktop-environment')

package() {
    cd "${startdir}" || exit 1
    make DESTDIR="${pkgdir}" PREFIX=/usr install
}
