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

    # Dodatkowa instalacja okay-settings i okay-dock w /usr/local/bin
    install -d /usr/local/bin
    install -m 755 scripts/okay-settings /usr/local/bin/okay-settings
    install -m 755 scripts/okay-dock /usr/local/bin/okay-dock

    # Usunięcie pozostałości starego Waybara i wyczyszczenie procesów
    rm -rf /etc/xdg/waybar 2>/dev/null || true
    pkill -x waybar 2>/dev/null || true

    # Rejestracja plików .desktop w /usr/share/applications
    install -d /usr/share/applications
    install -m 644 sessions/okay-settings.desktop /usr/share/applications/okay-settings.desktop
    install -m 644 sessions/okay-dock.desktop /usr/share/applications/okay-dock.desktop
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

    # Zapis wersji commita dla środowiska
    install -d /etc/okay-de
    if command -v git >/dev/null 2>&1 && git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        git rev-parse --short HEAD > /etc/okay-de/version 2>/dev/null || echo "9eede97" > /etc/okay-de/version
    else
        echo "9eede97" > /etc/okay-de/version
    fi

    echo "Instalacja systemowa zakończona sukcesem."
elif [ "$MODE" = "--user" ]; then
    echo "Instalowanie konfiguracji OKay DE dla bieżącego użytkownika ($HOME/.config)..."

    # Tworzenie katalogów docelowych
    mkdir -p "$HOME/.config/labwc"
    mkdir -p "$HOME/.config/foot"
    mkdir -p "$HOME/.config/fuzzel"
    mkdir -p "$HOME/.config/mako"
    mkdir -p "$HOME/.config/gtk-3.0"
    mkdir -p "$HOME/.config/gtk-4.0"
    mkdir -p "$HOME/.config/pcmanfm"
    mkdir -p "$HOME/.config/okay-de"
    mkdir -p "$HOME/.local/bin"
    mkdir -p "$HOME/.local/share/applications"
    mkdir -p "$HOME/.local/share/themes"
    mkdir -p "$HOME/Desktop"

    # Kopiowanie konfiguracji (w tym masek przycisków XBM i wektorów SVG do labwc)
    cp -r config/labwc/* "$HOME/.config/labwc/"
    cp -r config/foot/* "$HOME/.config/foot/"
    cp -r config/fuzzel/* "$HOME/.config/fuzzel/"
    cp -r config/mako/* "$HOME/.config/mako/"
    cp -r config/gtk-3.0/* "$HOME/.config/gtk-3.0/"
    cp -r config/gtk-4.0/* "$HOME/.config/gtk-4.0/"
    cp -r themes/* "$HOME/.local/share/themes/"

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
    cp sessions/okay-dock.desktop "$HOME/.local/share/applications/"
    cp sessions/foot.desktop "$HOME/.local/share/applications/"
    cp sessions/pcmanfm.desktop "$HOME/.local/share/applications/"

    # Usunięcie starej konfiguracji waybar i wyczyszczenie procesu
    rm -rf "$HOME/.config/waybar" 2>/dev/null || true
    pkill -x waybar 2>/dev/null || true

    # Inicjalizacja domyślnych ikon na pulpicie użytkownika
    echo "Kopiowanie domyślnych ikon na pulpit ($HOME/Desktop)..."
    cp sessions/foot.desktop "$HOME/Desktop/"
    cp sessions/pcmanfm.desktop "$HOME/Desktop/"
    cp sessions/okay-settings.desktop "$HOME/Desktop/"
    chmod 644 "$HOME/Desktop/"*.desktop

    # Zapis wersji commita dla profilu użytkownika
    if command -v git >/dev/null 2>&1 && git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        git rev-parse --short HEAD > "$HOME/.config/okay-de/version" 2>/dev/null || echo "9eede97" > "$HOME/.config/okay-de/version"
    else
        echo "9eede97" > "$HOME/.config/okay-de/version"
    fi

    echo "Konfiguracja użytkownika zainstalowana pomyślnie."
    echo "Wymagane pakiety dla OKay Settings i OKay Dock: python-pyqt6, qt6-wayland, wlrctl."
    echo "Sugerowane pakiety narzędzi: swaybg, wdisplays, pavucontrol, papirus-icon-theme."
    echo "Upewnij się, że \$HOME/.local/bin znajduje się w Twojej zmiennej PATH."

    if [ -f /usr/bin/okay-settings ] || [ -f /usr/local/bin/okay-settings ]; then
        echo ""
        echo "======================================================================"
        echo "UWAGA: Wykryto starszą instalację systemową (/usr/bin/okay-settings)."
        echo "Aby upewnić się, że całe środowisko korzysta z najnowszej wersji:"
        echo "uruchom: sudo ./install.sh --system"
        echo "======================================================================"
    fi
else
    echo "Użycie: $0 [--user | --system]"
    exit 1
fi
