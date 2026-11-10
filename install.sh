#!/usr/bin/env bash

# OKay DE - Instalator środowiska graficznego dla OKsavi Linux
set -euo pipefail

MODE="${1:---user}"

if [ "$MODE" = "--system" ]; then
    if [ "$(id -u)" -ne 0 ]; then
        echo "Błąd: Instalacja systemowa wymaga uprawnień roota (sudo)." >&2
        exit 1
    fi
    echo "Instalowanie OKay DE w całym systemie (/etc/xdg, /usr/bin)..."
    make install PREFIX=/usr
    echo "Instalacja systemowa zakończona sukcesem."
elif [ "$MODE" = "--user" ]; then
    echo "Instalowanie konfiguracji OKay DE dla bieżącego użytkownika ($HOME/.config)..."

    # Tworzenie katalogów docelowych
    mkdir -p "$HOME/.config/labwc"
    mkdir -p "$HOME/.config/waybar"
    mkdir -p "$HOME/.config/foot"
    mkdir -p "$HOME/.config/fuzzel"
    mkdir -p "$HOME/.config/mako"
    mkdir -p "$HOME/.local/bin"

    # Kopiowanie konfiguracji
    cp -r config/labwc/* "$HOME/.config/labwc/"
    cp -r config/waybar/* "$HOME/.config/waybar/"
    cp -r config/foot/* "$HOME/.config/foot/"
    cp -r config/fuzzel/* "$HOME/.config/fuzzel/"
    cp -r config/mako/* "$HOME/.config/mako/"

    # Kopiowanie skryptów
    cp scripts/* "$HOME/.local/bin/"
    chmod +x "$HOME/.local/bin/okay-"*
    chmod +x "$HOME/.config/labwc/autostart"

    echo "Konfiguracja użytkownika zainstalowana."
    echo "Upewnij się, że \$HOME/.local/bin znajduje się w Twojej zmiennej PATH."
else
    echo "Użycie: $0 [--user | --system]"
    exit 1
fi
