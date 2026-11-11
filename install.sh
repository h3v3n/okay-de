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

    # Instalacja pakietów systemowych dla pełnej obsługi panelu i ustawień GTK3
    if command -v pacman >/dev/null 2>&1; then
        echo "Weryfikacja i instalacja pakietów systemowych (gtk3, gtk-layer-shell, python-gobject, wlr-randr)..."
        pacman -S --needed --noconfirm gtk3 gtk-layer-shell python-gobject wlr-randr
    fi

    # Instalacja reguł uprawnień Polkit i Sudoers dla okay-settings
    if [ -f config/polkit/99-okay-de.rules ]; then
        install -d /etc/polkit-1/rules.d /usr/share/polkit-1/rules.d
        install -m 644 config/polkit/99-okay-de.rules /etc/polkit-1/rules.d/99-okay-de.rules
        install -m 644 config/polkit/99-okay-de.rules /usr/share/polkit-1/rules.d/99-okay-de.rules
    fi
    if [ -f config/sudoers/99-okay-de ]; then
        install -d /etc/sudoers.d
        install -m 440 config/sudoers/99-okay-de /etc/sudoers.d/99-okay-de
    fi

    # Usunięcie pozostałości starych/konfliktujących pasków oraz osieroconych plików poprzednich wersji
    rm -rf /etc/okay-dock /etc/xdg/sfwbar /etc/xdg/waybar /etc/xdg/okay-de/panel 2>/dev/null || true
    rm -rf /usr/share/okay-de/settings /usr/local/share/okay-de/settings 2>/dev/null || true
    rm -f /usr/local/bin/okay-dock /usr/bin/okay-dock 2>/dev/null || true
    rm -f /usr/bin/okay-settings /usr/local/bin/okay-settings 2>/dev/null || true
    rm -f /usr/share/applications/okay-dock.desktop /usr/share/applications/okay-de-settings.desktop /usr/share/applications/oksavi-settings.desktop 2>/dev/null || true
    rm -f /usr/local/share/applications/okay-settings.desktop /usr/local/share/applications/okay-dock.desktop 2>/dev/null || true
    pkill -9 -x okay-dock 2>/dev/null || true
    pkill -9 -x okay-settings 2>/dev/null || true
    pkill -9 -x waybar 2>/dev/null || true
    pkill -9 -x sfwbar 2>/dev/null || true

    # Nadpisanie okay-settings, jego modułów i okay-panel zarówno w /usr/bin, jak i w /usr/local/bin
    install -d /usr/bin /usr/local/bin /usr/share/okay-de/settings /usr/local/share/okay-de/settings
    install -m 755 scripts/okay-settings /usr/bin/okay-settings
    install -m 755 scripts/okay-settings /usr/local/bin/okay-settings
    install -m 755 scripts/okay-panel /usr/bin/okay-panel
    install -m 755 scripts/okay-panel /usr/local/bin/okay-panel
    if [ -d scripts/settings ]; then
        install -m 644 scripts/settings/*.py /usr/share/okay-de/settings/
        install -m 644 scripts/settings/*.py /usr/local/share/okay-de/settings/
    fi

    # Instalacja domyślnej konfiguracji panelu w /etc/xdg/okay-de/panel.json
    install -d /etc/xdg/okay-de
    install -m 644 config/okay-de/panel.json /etc/xdg/okay-de/panel.json

    # Rejestracja plików .desktop w /usr/share/applications, naprawa starych ścieżek Exec i aktualizacja bazy
    for sys_desktop in /usr/share/applications/*.desktop /usr/local/share/applications/*.desktop; do
        if [ -f "$sys_desktop" ] && grep -qE '^Exec=(/usr/bin/|/usr/local/bin/)okay-settings' "$sys_desktop" 2>/dev/null; then
            sed -i -E 's|^Exec=(/usr/bin/|/usr/local/bin/)okay-settings|Exec=okay-settings|' "$sys_desktop" 2>/dev/null || true
        fi
    done
    install -d /usr/share/applications
    install -m 644 sessions/okay-settings.desktop /usr/share/applications/okay-settings.desktop
    install -m 644 sessions/foot.desktop /usr/share/applications/foot.desktop
    install -m 644 sessions/pcmanfm.desktop /usr/share/applications/pcmanfm.desktop
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database /usr/share/applications/ 2>/dev/null || true
    fi

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

    # Weryfikacja obecności oficjalnych bibliotek GTK3 / Layer-Shell
    PKGS_TO_INSTALL=()
    if ! pacman -Qi gtk-layer-shell >/dev/null 2>&1; then PKGS_TO_INSTALL+=("gtk-layer-shell"); fi
    if ! pacman -Qi python-gobject >/dev/null 2>&1; then PKGS_TO_INSTALL+=("python-gobject"); fi
    if ! pacman -Qi gtk3 >/dev/null 2>&1; then PKGS_TO_INSTALL+=("gtk3"); fi
    if ! pacman -Qi wlr-randr >/dev/null 2>&1; then PKGS_TO_INSTALL+=("wlr-randr"); fi
    if [ ${#PKGS_TO_INSTALL[@]} -gt 0 ] && command -v pacman >/dev/null 2>&1; then
        echo "Wykryto brak pakietów dla paska zadań: ${PKGS_TO_INSTALL[*]}"
        INSTALLED=0
        if command -v sudo >/dev/null 2>&1; then
            echo "Instalowanie pakietów przez sudo pacman..."
            sudo pacman -S --needed --noconfirm "${PKGS_TO_INSTALL[@]}" && INSTALLED=1 || true
        fi
        if [ "$INSTALLED" -eq 0 ] && command -v pkexec >/dev/null 2>&1; then
            echo "Instalowanie pakietów przez pkexec pacman..."
            pkexec pacman -S --needed --noconfirm "${PKGS_TO_INSTALL[@]}" && INSTALLED=1 || true
        fi
    fi

    # Zapewnienie ścieżki ~/.local/bin w profilu powłoki
    if [ -f "$HOME/.bashrc" ] && ! grep -q '\.local/bin' "$HOME/.bashrc" 2>/dev/null; then
        echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.bashrc"
    fi

    # Usunięcie konfliktowych konfiguracji waybar / sfwbar
    rm -rf "$HOME/.config/sfwbar" "$HOME/.config/waybar" "$HOME/.config/okay-de/panel" 2>/dev/null || true
    pkill -9 -x waybar 2>/dev/null || true
    pkill -9 -x sfwbar 2>/dev/null || true

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

    # Domyślny plik panel.json (tylko jeśli nie istnieje, aby zachować przypięte aplikacje użytkownika)
    if [ ! -f "$HOME/.config/okay-de/panel.json" ] && [ -f config/okay-de/panel.json ]; then
        cp config/okay-de/panel.json "$HOME/.config/okay-de/panel.json"
    fi

    # Kopiowanie konfiguracji (w tym masek przycisków XBM i wektorów SVG do labwc)
    cp -r config/labwc/* "$HOME/.config/labwc/" 2>/dev/null || true
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

    # Bezpieczne kopiowanie skryptów i aplikacji do ~/.local/bin oraz modułów settings
    rm -rf "$HOME/.local/share/okay-de/settings" 2>/dev/null || true
    rm -f "$HOME/.local/bin/okay-settings" 2>/dev/null || true
    if [ -d scripts ]; then
        for s in scripts/*; do
            if [ -f "$s" ]; then
                cp "$s" "$HOME/.local/bin/"
                chmod +x "$HOME/.local/bin/$(basename "$s")"
            fi
        done
        if [ -d scripts/settings ]; then
            mkdir -p "$HOME/.local/share/okay-de/settings"
            cp scripts/settings/*.py "$HOME/.local/share/okay-de/settings/"
        fi
    fi
    chmod +x "$HOME/.config/labwc/autostart" 2>/dev/null || true

    # Czyszczenie starych/osieroconych wpisów .desktop oraz naprawa ścieżek Exec
    rm -f "$HOME/.local/share/applications/okay-dock.desktop" \
          "$HOME/.local/share/applications/okay-de-settings.desktop" \
          "$HOME/.local/share/applications/oksavi-settings.desktop" \
          "$HOME/.local/share/applications/okay-de.desktop" 2>/dev/null || true
    rm -f "$HOME/Desktop/okay-dock.desktop" \
          "$HOME/Desktop/okay-de-settings.desktop" \
          "$HOME/Desktop/oksavi-settings.desktop" 2>/dev/null || true

    for existing_desktop in "$HOME/.local/share/applications/"*.desktop; do
        if [ -f "$existing_desktop" ] && grep -qE '^Exec=(/usr/bin/|/usr/local/bin/)okay-settings' "$existing_desktop" 2>/dev/null; then
            sed -i -E 's|^Exec=(/usr/bin/|/usr/local/bin/)okay-settings|Exec=okay-settings|' "$existing_desktop" 2>/dev/null || true
        fi
    done

    # Kopiowanie aktualnych wpisów .desktop aplikacji i odświeżenie bazy oraz cache fuzzel
    if [ -d sessions ]; then
        for d in sessions/*.desktop; do
            if [ -f "$d" ] && [ "$(basename "$d")" != "okay-de.desktop" ]; then
                cp "$d" "$HOME/.local/share/applications/"
            fi
        done
    fi
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$HOME/.local/share/applications/" 2>/dev/null || true
    fi
    rm -f "$HOME/.cache/fuzzel" 2>/dev/null || true

    # Całkowite usunięcie pozostałości starego paska docka (okay-dock) i instancji starego okay-settings
    rm -rf "$HOME/.config/okay-dock" 2>/dev/null || true
    rm -f "$HOME/.local/bin/okay-dock" 2>/dev/null || true
    pkill -9 -x okay-dock 2>/dev/null || true
    pkill -9 -x okay-settings 2>/dev/null || true

    # Przeładowanie lub uruchomienie paska zadań OKay DE
    if [ -x "$HOME/.local/bin/okay-panel" ]; then
        "$HOME/.local/bin/okay-panel" --restart >/dev/null 2>&1 &
    elif [ -x /usr/local/bin/okay-panel ]; then
        /usr/local/bin/okay-panel --restart >/dev/null 2>&1 &
    elif [ -x /usr/bin/okay-panel ]; then
        /usr/bin/okay-panel --restart >/dev/null 2>&1 &
    elif command -v okay-panel >/dev/null 2>&1; then
        okay-panel --restart >/dev/null 2>&1 &
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
    echo "Wymagane pakiety: gtk3, gtk-layer-shell, python-gobject, wlr-randr, wireplumber."
    echo "Sugerowane pakiety narzędzi: swaybg, networkmanager, pavucontrol, papirus-icon-theme."
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
