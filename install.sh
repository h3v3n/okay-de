#!/usr/bin/env bash

# OKay DE - Instalator środowiska graficznego dla OKsavi Linux
set -euo pipefail

MODE="${1:---user}"

if [ "$MODE" = "--system" ]; then
    if [ "$(id -u)" -ne 0 ]; then
        echo "Błąd: Instalacja systemowa wymaga uprawnień roota (sudo)." >&2
        exit 1
    fi
    echo "Instalowanie OKay DE w całym systemie (/etc/xdg, /usr/bin, /usr/local/bin, /usr/share)..."
    make install PREFIX=/usr

    # Dodatkowa instalacja okay-settings w /usr/local/bin zgodnie ze specyfikacją
    install -d /usr/local/bin
    install -m 755 scripts/okay-settings /usr/local/bin/okay-settings

    # Rejestracja plików .desktop w /usr/share/applications
    install -d /usr/share/applications
    install -m 644 sessions/okay-settings.desktop /usr/share/applications/okay-settings.desktop
    install -m 644 sessions/foot.desktop /usr/share/applications/foot.desktop
    install -m 644 sessions/pcmanfm.desktop /usr/share/applications/pcmanfm.desktop

    # Szablon pierwszych ikon na pulpicie dla nowych użytkowników (/etc/skel/Desktop)
    echo "Tworzenie szablonu ikon pulpitu w /etc/skel/Desktop/..."
    install -d /etc/skel/Desktop
    install -m 644 sessions/foot.desktop /etc/skel/Desktop/foot.desktop
    install -m 644 sessions/pcmanfm.desktop /etc/skel/Desktop/pcmanfm.desktop
    install -m 644 sessions/okay-settings.desktop /etc/skel/Desktop/okay-settings.desktop

    # Instalacja domyślnej tapety systemowej
    echo "Instalowanie domyślnej tapety w /usr/share/backgrounds/okay-de/..."
    install -d /usr/share/backgrounds/okay-de
    install -m 644 backgrounds/default.png /usr/share/backgrounds/okay-de/default.png

    echo "Instalacja systemowa zakończona sukcesem."
elif [ "$MODE" = "--user" ]; then
    echo "Instalowanie konfiguracji OKay DE dla bieżącego użytkownika ($HOME/.config)..."

    # Tworzenie katalogów docelowych
    mkdir -p "$HOME/.config/labwc"
    mkdir -p "$HOME/.config/waybar"
    mkdir -p "$HOME/.config/foot"
    mkdir -p "$HOME/.config/fuzzel"
    mkdir -p "$HOME/.config/mako"
    mkdir -p "$HOME/.config/gtk-3.0"
    mkdir -p "$HOME/.config/gtk-4.0"
    mkdir -p "$HOME/.config/pcmanfm"
    mkdir -p "$HOME/.config/okay-de"
    mkdir -p "$HOME/.local/bin"
    mkdir -p "$HOME/.local/share/applications"
    mkdir -p "$HOME/Desktop"

    # Kopiowanie konfiguracji (w tym masek przycisków XBM do labwc)
    cp -r config/labwc/* "$HOME/.config/labwc/"
    cp -r config/waybar/* "$HOME/.config/waybar/"
    cp -r config/foot/* "$HOME/.config/foot/"
    cp -r config/fuzzel/* "$HOME/.config/fuzzel/"
    cp -r config/mako/* "$HOME/.config/mako/"
    cp -r config/gtk-3.0/* "$HOME/.config/gtk-3.0/"
    cp -r config/gtk-4.0/* "$HOME/.config/gtk-4.0/"

    # Kopiowanie domyślnej tapety użytkownika jeśli jeszcze nie istnieje
    if [ ! -f "$HOME/.config/okay-de/wallpaper.png" ]; then
        echo "Kopiowanie domyślnej tapety do $HOME/.config/okay-de/wallpaper.png..."
        cp backgrounds/default.png "$HOME/.config/okay-de/wallpaper.png"
    fi

    # Kopiowanie skryptów i aplikacji do ~/.local/bin
    cp scripts/* "$HOME/.local/bin/"
    chmod +x "$HOME/.local/bin/"*
    chmod +x "$HOME/.config/labwc/autostart"

    # Kopiowanie wpisów .desktop
    cp sessions/okay-settings.desktop "$HOME/.local/share/applications/"
    cp sessions/foot.desktop "$HOME/.local/share/applications/"
    cp sessions/pcmanfm.desktop "$HOME/.local/share/applications/"

    # Inicjalizacja domyślnych ikon na pulpicie użytkownika
    echo "Kopiowanie domyślnych ikon na pulpit ($HOME/Desktop)..."
    cp sessions/foot.desktop "$HOME/Desktop/"
    cp sessions/pcmanfm.desktop "$HOME/Desktop/"
    cp sessions/okay-settings.desktop "$HOME/Desktop/"
    chmod 644 "$HOME/Desktop/"*.desktop

    echo "Konfiguracja użytkownika zainstalowana pomyślnie."
    echo "Wymagane pakiety dla OKay Settings: python-gobject, libadwaita, gtk4."
    echo "Sugerowane pakiety narzędzi: swaybg, waybar, wdisplays, pavucontrol, papirus-icon-theme."
    echo "Upewnij się, że \$HOME/.local/bin znajduje się w Twojej zmiennej PATH."
else
    echo "Użycie: $0 [--user | --system]"
    exit 1
fi
