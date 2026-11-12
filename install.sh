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
    rm -rf /usr/share/okay-de/settings /usr/local/share/okay-de/settings /usr/share/okay-de/panel /usr/local/share/okay-de/panel 2>/dev/null || true
    rm -f /usr/local/bin/okay-dock /usr/bin/okay-dock 2>/dev/null || true
    rm -f /usr/bin/okay-settings /usr/local/bin/okay-settings 2>/dev/null || true
    rm -f /usr/share/applications/okay-dock.desktop /usr/share/applications/okay-de-settings.desktop /usr/share/applications/oksavi-settings.desktop 2>/dev/null || true
    rm -f /usr/local/share/applications/okay-settings.desktop /usr/local/share/applications/okay-dock.desktop 2>/dev/null || true
    pkill -9 -x okay-dock 2>/dev/null || true
    pkill -9 -x okay-settings 2>/dev/null || true
    pkill -9 -x waybar 2>/dev/null || true
    pkill -9 -x sfwbar 2>/dev/null || true

    # Nadpisanie okay-settings, okay-panel oraz ich modułów zarówno w /usr/bin, jak i w /usr/local/bin
    install -d /usr/bin /usr/local/bin /usr/share/okay-de/settings /usr/local/share/okay-de/settings /usr/share/okay-de/panel /usr/local/share/okay-de/panel
    install -m 755 scripts/okay-settings /usr/bin/okay-settings
    install -m 755 scripts/okay-settings /usr/local/bin/okay-settings
    install -m 755 scripts/okay-panel /usr/bin/okay-panel
    install -m 755 scripts/okay-panel /usr/local/bin/okay-panel
    if [ -d scripts/settings ]; then
        install -m 644 scripts/settings/*.py /usr/share/okay-de/settings/
        install -m 644 scripts/settings/*.py /usr/local/share/okay-de/settings/
    fi
    if [ -d scripts/panel ]; then
        install -m 644 scripts/panel/*.py /usr/share/okay-de/panel/
        install -m 644 scripts/panel/*.py /usr/local/share/okay-de/panel/
    fi

    # Instalacja domyślnej konfiguracji panelu i szablonów systemowych (read-only fallback)
    install -d /etc/xdg/okay-de /etc/okay-de /usr/share/okay-de/defaults/okay-de /usr/share/okay-de/defaults/labwc /usr/share/okay-de/defaults/backgrounds
    install -m 644 config/okay-de/panel.json /etc/xdg/okay-de/panel.json
    install -m 644 config/okay-de/panel.json /etc/okay-de/panel.json
    install -m 644 config/okay-de/panel.json /usr/share/okay-de/defaults/okay-de/panel.json
    install -m 644 config/labwc/* /usr/share/okay-de/defaults/labwc/ 2>/dev/null || true
    if [ -f backgrounds/default.png ]; then
        install -m 644 backgrounds/default.png /usr/share/okay-de/defaults/backgrounds/default.png
    fi

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

    # Zapis wersji commita dla szablonu systemowego
    install -d /etc/okay-de /usr/share/okay-de
    SYS_VER="${OKAY_DE_VERSION:-local}"
    if [ -z "${OKAY_DE_VERSION:-}" ] && command -v git >/dev/null 2>&1 && git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        SYS_VER="$(git rev-parse --short HEAD 2>/dev/null || echo local)"
    fi
    printf '%s\n' "$SYS_VER" > /etc/okay-de/version
    printf '%s\n' "$SYS_VER" > /usr/share/okay-de/version

    echo "Instalacja systemowa zakończona sukcesem."
elif [ "$MODE" = "--user" ]; then
    XDG_CFG="${XDG_CONFIG_HOME:-$HOME/.config}"
    XDG_DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
    XDG_CACHE="${XDG_CACHE_HOME:-$HOME/.cache}"
    BIN_DIR="$HOME/.local/bin"
    OKAY_CFG_DIR="$XDG_CFG/okay-de"
    LABWC_CFG_DIR="$XDG_CFG/labwc"
    OKAY_DATA_DIR="$XDG_DATA/okay-de"

    echo "Instalowanie OKay DE w przestrzeni użytkownika ($OKAY_CFG_DIR, $BIN_DIR, $OKAY_DATA_DIR)..."

    # Funkcje pomocnicze: zapis atomowy oraz kopiowanie bez nadpisywania ustawień użytkownika
    atomic_copy_file() {
        local src="$1" dst="$2" mode="${3:-644}"
        mkdir -p "$(dirname "$dst")"
        local tmp
        tmp="$(mktemp "$(dirname "$dst")/.tmp-XXXXXX")"
        cp "$src" "$tmp"
        chmod "$mode" "$tmp"
        mv -f "$tmp" "$dst"
    }

    copy_if_missing() {
        local src="$1" dst="$2" mode="${3:-644}"
        if [ ! -e "$dst" ] && [ -f "$src" ]; then
            atomic_copy_file "$src" "$dst" "$mode"
        fi
    }

    copy_dir_missing() {
        local src_dir="$1" dst_dir="$2" mode="${3:-644}"
        local item base
        [ -d "$src_dir" ] || return 0
        mkdir -p "$dst_dir"
        for item in "$src_dir"/*; do
            [ -e "$item" ] || continue
            base="$(basename "$item")"
            if [ -d "$item" ]; then
                copy_dir_missing "$item" "$dst_dir/$base" "$mode"
            elif [ -f "$item" ]; then
                copy_if_missing "$item" "$dst_dir/$base" "$mode"
            fi
        done
    }

    # Lekka weryfikacja zależności (wyłącznie informacyjna, zero sudo/pkexec)
    MISSING_DEPS=()
    if command -v python3 >/dev/null 2>&1; then
        python3 -c "import gi; gi.require_version('Gtk', '3.0'); gi.require_version('GtkLayerShell', '0.1')" 2>/dev/null || MISSING_DEPS+=("gtk3/python-gobject/gtk-layer-shell")
    else
        MISSING_DEPS+=("python3")
    fi
    command -v wlr-randr >/dev/null 2>&1 || MISSING_DEPS+=("wlr-randr")
    command -v wpctl >/dev/null 2>&1 || MISSING_DEPS+=("wireplumber")
    if [ ${#MISSING_DEPS[@]} -gt 0 ]; then
        echo "Uwaga: Brak części zależności systemowych (${MISSING_DEPS[*]}). Kontynuuję bez uprawnień root." >&2
    fi

    # Zapewnienie ścieżki ~/.local/bin w profilu powłoki
    if [ -f "$HOME/.bashrc" ] && ! grep -q '\.local/bin' "$HOME/.bashrc" 2>/dev/null; then
        echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.bashrc"
    fi

    # Usunięcie starych konfliktowych konfiguracji wyłącznie w katalogu użytkownika
    rm -rf "$XDG_CFG/sfwbar" "$XDG_CFG/waybar" "$OKAY_CFG_DIR/panel" 2>/dev/null || true
    pkill -9 -x waybar 2>/dev/null || true
    pkill -9 -x sfwbar 2>/dev/null || true

    # Tworzenie katalogów docelowych XDG użytkownika
    mkdir -p "$LABWC_CFG_DIR" \
             "$XDG_CFG/foot" \
             "$XDG_CFG/fuzzel" \
             "$XDG_CFG/mako" \
             "$XDG_CFG/gtk-3.0" \
             "$XDG_CFG/gtk-4.0" \
             "$XDG_CFG/pcmanfm" \
             "$OKAY_CFG_DIR" \
             "$BIN_DIR" \
             "$OKAY_DATA_DIR/settings" \
             "$OKAY_DATA_DIR/panel" \
             "$OKAY_DATA_DIR/defaults/okay-de" \
             "$OKAY_DATA_DIR/defaults/labwc" \
             "$OKAY_DATA_DIR/defaults/backgrounds" \
             "$XDG_DATA/applications" \
             "$XDG_DATA/themes" \
             "$HOME/Desktop"

    # Szablony read-only dla pierwszego uruchomienia okay-settings w przestrzeni użytkownika
    if [ -f config/okay-de/panel.json ]; then
        atomic_copy_file config/okay-de/panel.json "$OKAY_DATA_DIR/defaults/okay-de/panel.json" 644
    fi
    if [ -d config/labwc ]; then
        for lf in config/labwc/*; do
            [ -f "$lf" ] || continue
            lbase="$(basename "$lf")"
            if [ "$lbase" = "autostart" ]; then
                atomic_copy_file "$lf" "$OKAY_DATA_DIR/defaults/labwc/$lbase" 755
            else
                atomic_copy_file "$lf" "$OKAY_DATA_DIR/defaults/labwc/$lbase" 644
            fi
        done
    fi
    if [ -f backgrounds/default.png ]; then
        atomic_copy_file backgrounds/default.png "$OKAY_DATA_DIR/defaults/backgrounds/default.png" 644
    fi

    # Konfiguracja ~/.config/okay-de/panel.json: zachowaj ustawienia użytkownika i dopisz ewentualne brakujące klucze
    if [ -f config/okay-de/panel.json ]; then
        if [ ! -f "$OKAY_CFG_DIR/panel.json" ]; then
            atomic_copy_file config/okay-de/panel.json "$OKAY_CFG_DIR/panel.json" 644
        elif command -v python3 >/dev/null 2>&1; then
            python3 - "$OKAY_CFG_DIR/panel.json" config/okay-de/panel.json <<'PYEOF'
import copy, json, os, sys, tempfile
user_p, tmpl_p = sys.argv[1], sys.argv[2]
try:
    with open(user_p, "r", encoding="utf-8") as f:
        user_d = json.load(f)
    with open(tmpl_p, "r", encoding="utf-8") as f:
        tmpl_d = json.load(f)
    if isinstance(user_d, dict) and isinstance(tmpl_d, dict):
        def merge_missing(dst, src):
            changed = False
            for k, v in src.items():
                if k not in dst:
                    dst[k] = copy.deepcopy(v)
                    changed = True
                elif isinstance(dst[k], dict) and isinstance(v, dict):
                    if merge_missing(dst[k], v):
                        changed = True
            return changed
        if merge_missing(user_d, tmpl_d):
            fd, tmp = tempfile.mkstemp(prefix=".tmp-", dir=os.path.dirname(os.path.abspath(user_p)))
            with os.fdopen(fd, "w", encoding="utf-8") as out:
                json.dump(user_d, out, indent=2, ensure_ascii=False)
                out.write("\n")
                out.flush()
                os.fsync(out.fileno())
            os.replace(tmp, user_p)
except Exception:
    pass
PYEOF
        fi
    fi

    # Konfiguracja ~/.config/labwc: zachowujemy istniejące pliki użytkownika, kopiujemy brakujące i ikony przycisków
    if [ -d config/labwc ]; then
        for lf in config/labwc/*; do
            [ -f "$lf" ] || continue
            lbase="$(basename "$lf")"
            case "$lbase" in
                *.xbm|*.svg)
                    copy_if_missing "$lf" "$LABWC_CFG_DIR/$lbase" 644
                    ;;
                autostart)
                    copy_if_missing "$lf" "$LABWC_CFG_DIR/$lbase" 755
                    chmod +x "$LABWC_CFG_DIR/autostart" 2>/dev/null || true
                    ;;
                *)
                    copy_if_missing "$lf" "$LABWC_CFG_DIR/$lbase" 644
                    ;;
            esac
        done
    fi

    copy_dir_missing config/foot "$XDG_CFG/foot" 644
    copy_dir_missing config/fuzzel "$XDG_CFG/fuzzel" 644
    copy_dir_missing config/mako "$XDG_CFG/mako" 644
    copy_dir_missing config/gtk-3.0 "$XDG_CFG/gtk-3.0" 644
    copy_dir_missing config/gtk-4.0 "$XDG_CFG/gtk-4.0" 644
    copy_dir_missing config/pcmanfm "$XDG_CFG/pcmanfm" 644
    cp -r themes/* "$XDG_DATA/themes/" 2>/dev/null || true

    # Domyślna tapeta użytkownika (tylko jeśli nie istnieje)
    copy_if_missing backgrounds/default.png "$OKAY_CFG_DIR/wallpaper.png" 644

    # Atomowa instalacja skryptów wykonawczych i modułów w ~/.local/bin i ~/.local/share/okay-de
    rm -rf "$OKAY_DATA_DIR/settings" "$OKAY_DATA_DIR/panel" 2>/dev/null || true
    mkdir -p "$OKAY_DATA_DIR/settings" "$OKAY_DATA_DIR/panel"
    if [ -d scripts ]; then
        for s in scripts/*; do
            if [ -f "$s" ]; then
                atomic_copy_file "$s" "$BIN_DIR/$(basename "$s")" 755
            fi
        done
        if [ -d scripts/settings ]; then
            for pyf in scripts/settings/*.py; do
                [ -f "$pyf" ] && atomic_copy_file "$pyf" "$OKAY_DATA_DIR/settings/$(basename "$pyf")" 644
            done
        fi
        if [ -d scripts/panel ]; then
            for pyf in scripts/panel/*.py; do
                [ -f "$pyf" ] && atomic_copy_file "$pyf" "$OKAY_DATA_DIR/panel/$(basename "$pyf")" 644
            done
        fi
    fi

    # Czyszczenie starych wpisów .desktop wyłącznie w katalogu użytkownika i naprawa ścieżek Exec
    rm -f "$XDG_DATA/applications/okay-dock.desktop" \
          "$XDG_DATA/applications/okay-de-settings.desktop" \
          "$XDG_DATA/applications/oksavi-settings.desktop" \
          "$XDG_DATA/applications/okay-de.desktop" 2>/dev/null || true
    rm -f "$HOME/Desktop/okay-dock.desktop" \
          "$HOME/Desktop/okay-de-settings.desktop" \
          "$HOME/Desktop/oksavi-settings.desktop" 2>/dev/null || true

    for existing_desktop in "$XDG_DATA/applications/"*.desktop "$HOME/Desktop/"*.desktop; do
        if [ -f "$existing_desktop" ] && grep -qE '^Exec=(/usr/bin/|/usr/local/bin/)okay-settings' "$existing_desktop" 2>/dev/null; then
            sed -i -E 's|^Exec=(/usr/bin/|/usr/local/bin/)okay-settings|Exec=okay-settings|' "$existing_desktop" 2>/dev/null || true
        fi
    done

    if [ -d sessions ]; then
        for d in sessions/*.desktop; do
            if [ -f "$d" ] && [ "$(basename "$d")" != "okay-de.desktop" ]; then
                atomic_copy_file "$d" "$XDG_DATA/applications/$(basename "$d")" 644
            fi
        done
    fi
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$XDG_DATA/applications/" 2>/dev/null || true
    fi
    rm -f "$XDG_CACHE/fuzzel" 2>/dev/null || true

    # Czyszczenie przestarzałych plików okay-dock (bez ubijania aktywnego okay-settings w trakcie aktualizacji)
    rm -rf "$OKAY_CFG_DIR/dock" 2>/dev/null || true
    rm -f "$BIN_DIR/okay-dock" "$BIN_DIR/oksavi-ghost-runner" 2>/dev/null || true
    pkill -9 -x okay-dock 2>/dev/null || true
    if [ "${OKAY_DE_FROM_UPDATER:-0}" != "1" ]; then
        pkill -9 -x okay-settings 2>/dev/null || true
    fi

    # Domyślne ikony na pulpicie użytkownika (nie nadpisuj już istniejących)
    copy_if_missing sessions/foot.desktop "$HOME/Desktop/foot.desktop" 644
    copy_if_missing sessions/pcmanfm.desktop "$HOME/Desktop/pcmanfm.desktop" 644
    copy_if_missing sessions/okay-settings.desktop "$HOME/Desktop/okay-settings.desktop" 644

    # Atomowy zapis wersji commita dla profilu użytkownika i kopii defaults
    USER_VER="${OKAY_DE_VERSION:-local}"
    if [ -z "${OKAY_DE_VERSION:-}" ] && command -v git >/dev/null 2>&1 && git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        USER_VER="$(git rev-parse --short HEAD 2>/dev/null || echo local)"
    fi
    TMP_VER="$(mktemp "$OKAY_CFG_DIR/.tmp-ver-XXXXXX")"
    printf '%s\n' "$USER_VER" > "$TMP_VER"
    mv -f "$TMP_VER" "$OKAY_CFG_DIR/version"
    atomic_copy_file "$OKAY_CFG_DIR/version" "$OKAY_DATA_DIR/defaults/version" 644

    # Przeładowanie sesji Wayland bez błędu (labwc --reconfigure oraz restart okay-panel)
    if [ "${OKAY_DE_SKIP_RELOAD:-0}" != "1" ] && [ -n "${WAYLAND_DISPLAY:-}" ]; then
        if command -v labwc >/dev/null 2>&1 && pgrep -u "$(id -u)" -x labwc >/dev/null 2>&1; then
            labwc --reconfigure >/dev/null 2>&1 || true
        fi
        if [ -x "$BIN_DIR/okay-panel" ]; then
            nohup "$BIN_DIR/okay-panel" --restart </dev/null >/dev/null 2>&1 &
        elif command -v okay-panel >/dev/null 2>&1; then
            nohup okay-panel --restart </dev/null >/dev/null 2>&1 &
        fi
    fi

    echo "Konfiguracja użytkownika zainstalowana pomyślnie (bez uprawnień root)."
    echo "Ścieżki XDG: config=$OKAY_CFG_DIR, bin=$BIN_DIR, data=$OKAY_DATA_DIR"
    if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
        echo "Uwaga: Upewnij się, że $BIN_DIR znajduje się na początku zmiennej PATH."
    fi
else
    echo "Użycie: $0 [--user | --system]"
    exit 1
fi
