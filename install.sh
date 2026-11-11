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

    # Instalacja pakietów systemowych dla pełnej obsługi okien i taskbara
    if command -v pacman >/dev/null 2>&1; then
        echo "Weryfikacja i instalacja pakietów (sfwbar, wlrctl, python-pyqt6, qt6-wayland)..."
        pacman -S --needed --noconfirm sfwbar wlrctl python-pyqt6 qt6-wayland 2>/dev/null || true
    fi

    # Usunięcie pozostałości starego paska docka (okay-dock) oraz waybar
    rm -rf /etc/xdg/waybar /etc/okay-dock 2>/dev/null || true
    rm -f /usr/local/bin/okay-dock /usr/bin/okay-dock 2>/dev/null || true
    rm -f /usr/share/applications/okay-dock.desktop 2>/dev/null || true
    pkill -9 -x okay-dock 2>/dev/null || true
    pkill -x waybar 2>/dev/null || true

    # Dodatkowa instalacja okay-settings w /usr/local/bin
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

    # Weryfikacja obecności sfwbar i wlrctl
    MISSING_PKGS=()
    if ! command -v sfwbar >/dev/null 2>&1; then
        MISSING_PKGS+=("sfwbar")
    fi
    if ! command -v wlrctl >/dev/null 2>&1; then
        MISSING_PKGS+=("wlrctl")
    fi
    if [ ${#MISSING_PKGS[@]} -gt 0 ]; then
        if command -v pkexec >/dev/null 2>&1; then
            echo "Wykryto brak pakietów (${MISSING_PKGS[*]}). Instalowanie..."
            pkexec pacman -S --needed --noconfirm "${MISSING_PKGS[@]}" 2>/dev/null || true
        elif command -v sudo >/dev/null 2>&1; then
            sudo pacman -S --needed --noconfirm "${MISSING_PKGS[@]}" 2>/dev/null || true
        fi
    fi

    # Tworzenie katalogów docelowych
    mkdir -p "$HOME/.config/labwc"
    mkdir -p "$HOME/.config/sfwbar"
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
    cp -r config/labwc/* "$HOME/.config/labwc/" 2>/dev/null || true
    cp -r config/sfwbar/* "$HOME/.config/sfwbar/" 2>/dev/null || true
    cp -r config/foot/* "$HOME/.config/foot/" 2>/dev/null || true
    cp -r config/fuzzel/* "$HOME/.config/fuzzel/" 2>/dev/null || true
    cp -r config/mako/* "$HOME/.config/mako/" 2>/dev/null || true
    cp -r config/gtk-3.0/* "$HOME/.config/gtk-3.0/" 2>/dev/null || true
    cp -r config/gtk-4.0/* "$HOME/.config/gtk-4.0/" 2>/dev/null || true
    cp -r themes/* "$HOME/.local/share/themes/" 2>/dev/null || true

    # Kopiowanie domyślnej tapety użytkownika jeśli jeszcze nie istnieje
    if [ ! -f "$HOME/.config/okay-de/wallpaper.png" ] && [ -f backgrounds/default.png ]; then
        echo "Kopiowanie domyślnej tapety do $HOME/.config/okay-de/wallpaper.png..."
        cp backgrounds/default.png "$HOME/.config/okay-de/wallpaper.png"
    fi

    # Bezpieczne kopiowanie skryptów i aplikacji do ~/.local/bin
    if [ -d scripts ]; then
        for s in scripts/*; do
            if [ -f "$s" ]; then
                cp "$s" "$HOME/.local/bin/"
                chmod +x "$HOME/.local/bin/$(basename "$s")"
            fi
        done
    fi
    chmod +x "$HOME/.config/labwc/autostart" 2>/dev/null || true

    # Bezpieczne kopiowanie wpisów .desktop
    if [ -d sessions ]; then
        for d in sessions/*.desktop; do
            if [ -f "$d" ]; then
                cp "$d" "$HOME/.local/share/applications/"
            fi
        done
    fi

    # Całkowite usunięcie pozostałości starego paska docka (okay-dock) i waybar
    rm -rf "$HOME/.config/waybar" "$HOME/.config/okay-dock" 2>/dev/null || true
    rm -f "$HOME/.local/bin/okay-dock" "$HOME/.local/share/applications/okay-dock.desktop" 2>/dev/null || true
    pkill -9 -x okay-dock 2>/dev/null || true
    pkill -x waybar 2>/dev/null || true
    if pgrep -x sfwbar >/dev/null 2>&1; then
        echo "Przeładowywanie uruchomionego paska sfwbar (SIGHUP)..."
        killall -SIGHUP sfwbar 2>/dev/null || true
    fi

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
    echo "Wymagane pakiety: sfwbar, python-pyqt6, qt6-wayland, wlrctl."
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
