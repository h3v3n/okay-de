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

    # Instalacja pakietów systemowych (Wayfire jako kompozytor bazowy oraz zależności GTK3/Wayland)
    if command -v pacman >/dev/null 2>&1; then
        echo "Weryfikacja i instalacja pakietów systemowych (wayfire, gtk3, gtk-layer-shell, python-gobject, wlr-randr)..."
        pacman -S --needed --noconfirm wayfire swaybg foot mako pcmanfm swaylock grim slurp wl-clipboard cliphist brightnessctl wireplumber gtk3 gtk-layer-shell python python-gobject wlr-randr xdg-user-dirs papirus-icon-theme adwaita-icon-theme librsvg meson ninja git pkgconf cairo pango glm gcc || \
            pacman -S --needed --noconfirm wayfire swaybg foot mako pcmanfm swaylock grim slurp wl-clipboard cliphist brightnessctl wireplumber gtk3 gtk-layer-shell python python-gobject wlr-randr xdg-user-dirs papirus-icon-theme adwaita-icon-theme || \
            pacman -S --needed --noconfirm wayfire gtk3 gtk-layer-shell python-gobject wlr-randr
    fi

    # Opcjonalna kompilacja i instalacja wtyczki pixdecor (zaokrąglone narożniki okien i własne przyciski dla Wayfire)
    if [ ! -f /usr/lib/wayfire/libpixdecor.so ] && [ ! -f /usr/local/lib/wayfire/libpixdecor.so ]; then
        if command -v git >/dev/null 2>&1 && command -v meson >/dev/null 2>&1 && command -v ninja >/dev/null 2>&1 && { command -v g++ >/dev/null 2>&1 || command -v c++ >/dev/null 2>&1; } && { [ -f /usr/include/glm/glm.hpp ] || pkg-config --exists glm 2>/dev/null; } && pkg-config --exists wayfire cairo pango librsvg-2.0 2>/dev/null; then
            echo "Kompilowanie wtyczki Wayfire pixdecor (zaokrąglone narożniki i styl przycisków)..."
            PIX_TMP="$(mktemp -d /tmp/okay-pixdecor-XXXXXX)"
            PIX_LOG="/tmp/okay-pixdecor-build.log"
            if git clone --depth 1 https://github.com/soreau/pixdecor.git "$PIX_TMP/src" >"$PIX_LOG" 2>&1; then
                if meson setup "$PIX_TMP/build" "$PIX_TMP/src" --prefix=/usr --buildtype=release >>"$PIX_LOG" 2>&1 && ninja -C "$PIX_TMP/build" >>"$PIX_LOG" 2>&1; then
                    ninja -C "$PIX_TMP/build" install >>"$PIX_LOG" 2>&1 || true
                    echo "Zainstalowano wtyczkę pixdecor w /usr/lib/wayfire."
                else
                    echo "Uwaga: Kompilacja pixdecor nie powiodła się (log: $PIX_LOG) — używam wbudowanej wtyczki decoration." >&2
                fi
            fi
            rm -rf "$PIX_TMP" 2>/dev/null || true
        else
            echo "Uwaga: Pominięto kompilację pixdecor (brak pakietów: glm, gcc, meson, ninja, wayfire, cairo, pango, librsvg)." >&2
        fi
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

    # Usunięcie pozostałości starych/konfliktujących pasków, przestarzałego labwc oraz osieroconych plików poprzednich wersji
    rm -rf /etc/okay-dock /etc/xdg/sfwbar /etc/xdg/waybar /etc/xdg/okay-de/panel 2>/dev/null || true
    rm -rf /etc/xdg/labwc /etc/okay-de/labwc /usr/share/okay-de/defaults/labwc /usr/local/share/okay-de/defaults/labwc 2>/dev/null || true
    rm -rf /usr/share/themes/OKay/labwc /usr/share/themes/OKay/openbox-3 2>/dev/null || true
    rm -rf /usr/share/okay-de/settings /usr/local/share/okay-de/settings /usr/share/okay-de/panel /usr/local/share/okay-de/panel 2>/dev/null || true
    rm -f /usr/local/bin/okay-dock /usr/bin/okay-dock 2>/dev/null || true
    rm -f /usr/bin/okay-settings /usr/local/bin/okay-settings 2>/dev/null || true
    rm -f /usr/share/applications/okay-dock.desktop /usr/share/applications/okay-de-settings.desktop /usr/share/applications/oksavi-settings.desktop 2>/dev/null || true
    rm -f /usr/local/share/applications/okay-settings.desktop /usr/local/share/applications/okay-dock.desktop 2>/dev/null || true
    pkill -9 -x okay-dock 2>/dev/null || true
    pkill -9 -x okay-settings 2>/dev/null || true
    pkill -9 -x waybar 2>/dev/null || true
    pkill -9 -x sfwbar 2>/dev/null || true

    # Nadpisanie skryptów wykonawczych oraz modułów zarówno w /usr/bin, jak i w /usr/local/bin
    install -d /usr/bin /usr/local/bin /usr/share/okay-de/settings /usr/local/share/okay-de/settings /usr/share/okay-de/panel /usr/local/share/okay-de/panel /usr/share/wayland-sessions
    if [ -d scripts ]; then
        for s in scripts/*; do
            if [ -f "$s" ]; then
                sbase="$(basename "$s")"
                install -m 755 "$s" "/usr/bin/$sbase"
                install -m 755 "$s" "/usr/local/bin/$sbase"
            fi
        done
    fi
    if [ -f sessions/okay-de.desktop ]; then
        install -m 644 sessions/okay-de.desktop /usr/share/wayland-sessions/okay-de.desktop
    fi
    if [ -d scripts/settings ]; then
        install -m 644 scripts/settings/*.py /usr/share/okay-de/settings/
        install -m 644 scripts/settings/*.py /usr/local/share/okay-de/settings/
    fi
    if [ -d scripts/panel ]; then
        install -m 644 scripts/panel/*.py /usr/share/okay-de/panel/
        install -m 644 scripts/panel/*.py /usr/local/share/okay-de/panel/
    fi

    # Instalacja domyślnej konfiguracji Wayfire, autostartu, panelu i szablonów systemowych (read-only fallback)
    install -d /etc/xdg /etc/xdg/okay-de /etc/okay-de /etc/skel/.config/okay-de \
        /usr/share/okay-de/defaults/okay-de /usr/share/okay-de/defaults/wayfire /usr/share/okay-de/defaults/backgrounds
    install -m 644 config/okay-de/panel.json /etc/xdg/okay-de/panel.json
    install -m 644 config/okay-de/panel.json /etc/okay-de/panel.json
    install -m 644 config/okay-de/panel.json /etc/skel/.config/okay-de/panel.json
    install -m 644 config/okay-de/panel.json /usr/share/okay-de/defaults/okay-de/panel.json
    if [ -f config/okay-de/autostart ]; then
        install -m 755 config/okay-de/autostart /etc/xdg/okay-de/autostart
        install -m 755 config/okay-de/autostart /etc/okay-de/autostart
        install -m 755 config/okay-de/autostart /etc/skel/.config/okay-de/autostart
        install -m 755 config/okay-de/autostart /usr/share/okay-de/defaults/okay-de/autostart
    fi
    if [ -f config/wayfire/wayfire.ini ]; then
        install -m 644 config/wayfire/wayfire.ini /etc/xdg/wayfire.ini
        install -m 644 config/wayfire/wayfire.ini /etc/xdg/okay-de/wayfire.ini
        install -m 644 config/wayfire/wayfire.ini /etc/okay-de/wayfire.ini
        install -m 644 config/wayfire/wayfire.ini /etc/skel/.config/wayfire.ini
        install -m 644 config/wayfire/wayfire.ini /usr/share/okay-de/defaults/wayfire/wayfire.ini
        if [ -f /usr/lib/wayfire/libpixdecor.so ] || [ -f /usr/local/lib/wayfire/libpixdecor.so ]; then
            for wf_target in /etc/xdg/wayfire.ini /etc/xdg/okay-de/wayfire.ini /etc/okay-de/wayfire.ini /etc/skel/.config/wayfire.ini /usr/share/okay-de/defaults/wayfire/wayfire.ini; do
                if [ -f "$wf_target" ] && grep -qE '^plugins\s*=.*\bdecoration\b' "$wf_target" 2>/dev/null && ! grep -qE '^plugins\s*=.*\bpixdecor\b' "$wf_target" 2>/dev/null; then
                    sed -i -E 's/(^plugins\s*=.*)\bdecoration\b/\1pixdecor/' "$wf_target" 2>/dev/null || true
                fi
            done
        fi
    fi
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
    command -v wayfire >/dev/null 2>&1 || MISSING_DEPS+=("wayfire")
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
    rm -rf "$XDG_CFG/sfwbar" "$XDG_CFG/waybar" "$OKAY_CFG_DIR/panel" "$OKAY_DATA_DIR/defaults/labwc" 2>/dev/null || true
    pkill -9 -x waybar 2>/dev/null || true
    pkill -9 -x sfwbar 2>/dev/null || true

    # Tworzenie katalogów docelowych XDG użytkownika
    mkdir -p "$XDG_CFG/foot" \
             "$XDG_CFG/mako" \
             "$XDG_CFG/gtk-3.0" \
             "$XDG_CFG/gtk-4.0" \
             "$XDG_CFG/pcmanfm" \
             "$OKAY_CFG_DIR" \
             "$BIN_DIR" \
             "$OKAY_DATA_DIR/settings" \
             "$OKAY_DATA_DIR/panel" \
             "$OKAY_DATA_DIR/defaults/okay-de" \
             "$OKAY_DATA_DIR/defaults/wayfire" \
             "$OKAY_DATA_DIR/defaults/backgrounds" \
             "$XDG_DATA/applications" \
             "$HOME/Desktop"

    # Szablony read-only dla pierwszego uruchomienia okay-settings oraz sesji Wayfire
    if [ -f config/okay-de/panel.json ]; then
        atomic_copy_file config/okay-de/panel.json "$OKAY_DATA_DIR/defaults/okay-de/panel.json" 644
    fi
    if [ -f config/okay-de/autostart ]; then
        atomic_copy_file config/okay-de/autostart "$OKAY_DATA_DIR/defaults/okay-de/autostart" 755
        copy_if_missing config/okay-de/autostart "$OKAY_CFG_DIR/autostart" 755
        chmod +x "$OKAY_CFG_DIR/autostart" 2>/dev/null || true
    fi
    if [ -f config/wayfire/wayfire.ini ]; then
        atomic_copy_file config/wayfire/wayfire.ini "$OKAY_DATA_DIR/defaults/wayfire/wayfire.ini" 644
        copy_if_missing config/wayfire/wayfire.ini "$XDG_CFG/wayfire.ini" 644

        # Opcjonalna kompilacja pixdecor do przestrzeni użytkownika (~/.local/share/wayfire), jeśli systemowo brak wtyczki
        if [ ! -f /usr/lib/wayfire/libpixdecor.so ] && [ ! -f /usr/local/lib/wayfire/libpixdecor.so ] && [ ! -f "$XDG_DATA/wayfire/plugins/libpixdecor.so" ]; then
            if command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null; then
                if [ ! -f /usr/include/glm/glm.hpp ] || ! command -v g++ >/dev/null 2>&1 || ! command -v meson >/dev/null 2>&1; then
                    sudo pacman -S --needed --noconfirm glm gcc meson ninja pkgconf cairo pango librsvg >/dev/null 2>&1 || true
                fi
            fi
            if command -v git >/dev/null 2>&1 && command -v meson >/dev/null 2>&1 && command -v ninja >/dev/null 2>&1 && { command -v g++ >/dev/null 2>&1 || command -v c++ >/dev/null 2>&1; } && { [ -f /usr/include/glm/glm.hpp ] || pkg-config --exists glm 2>/dev/null; } && pkg-config --exists wayfire cairo pango librsvg-2.0 2>/dev/null; then
                echo "Budowanie wtyczki pixdecor w przestrzeni użytkownika ($XDG_DATA/wayfire)..."
                PIX_TMP="$(mktemp -d "${TMPDIR:-/tmp}/okay-pixdecor-XXXXXX")"
                PIX_LOG="/tmp/okay-pixdecor-build.log"
                if git clone --depth 1 https://github.com/soreau/pixdecor.git "$PIX_TMP/src" >"$PIX_LOG" 2>&1 && \
                   meson setup "$PIX_TMP/build" "$PIX_TMP/src" --prefix=/usr --buildtype=release >>"$PIX_LOG" 2>&1 && \
                   ninja -C "$PIX_TMP/build" >>"$PIX_LOG" 2>&1; then
                    mkdir -p "$XDG_DATA/wayfire/plugins" "$XDG_DATA/wayfire/metadata"
                    SO_BUILT="$(find "$PIX_TMP/build" -maxdepth 4 -name 'libpixdecor.so' | head -n 1)"
                    XML_BUILT="$(find "$PIX_TMP/src" "$PIX_TMP/build" -maxdepth 4 -name 'pixdecor.xml' | head -n 1)"
                    if [ -n "$SO_BUILT" ]; then
                        install -m 755 "$SO_BUILT" "$XDG_DATA/wayfire/plugins/libpixdecor.so" 2>/dev/null || true
                    fi
                    if [ -n "$XML_BUILT" ]; then
                        install -m 644 "$XML_BUILT" "$XDG_DATA/wayfire/metadata/pixdecor.xml" 2>/dev/null || true
                    fi
                else
                    echo "Uwaga: Kompilacja pixdecor nie powiodła się (log: $PIX_LOG)." >&2
                fi
                rm -rf "$PIX_TMP" 2>/dev/null || true
            else
                echo "Uwaga: Brak narzędzi do kompilacji pixdecor (zainstaluj: sudo pacman -S --needed glm gcc meson ninja librsvg)." >&2
            fi
        fi

        if [ -f "$XDG_DATA/wayfire/plugins/libpixdecor.so" ] || [ -f /usr/lib/wayfire/libpixdecor.so ] || [ -f /usr/local/lib/wayfire/libpixdecor.so ]; then
            if grep -qE '^plugins\s*=.*\bdecoration\b' "$XDG_CFG/wayfire.ini" 2>/dev/null && ! grep -qE '^plugins\s*=.*\bpixdecor\b' "$XDG_CFG/wayfire.ini" 2>/dev/null; then
                sed -i -E 's/(^plugins\s*=.*)\bdecoration\b/\1pixdecor/' "$XDG_CFG/wayfire.ini" 2>/dev/null || true
            fi
        fi

        # Migracja istniejącego ~/.config/wayfire.ini: poprawne animacje ([animate]), kolory float RGBA i klucze pixdecor
        if [ -f "$XDG_CFG/wayfire.ini" ] && command -v python3 >/dev/null 2>&1; then
            python3 - "$XDG_CFG/wayfire.ini" "$OKAY_CFG_DIR/panel.json" <<'PYEOF'
import json, os, re, sys, tempfile
wf_path, panel_path = sys.argv[1], sys.argv[2]

def parse_hex(h, default=(0, 230, 118)):
    h = (h or "").strip()
    if h.startswith("#"):
        h = h[1:]
    if len(h) >= 6:
        try:
            return tuple(int(h[i:i+2], 16) for i in (0, 2, 4))
        except Exception:
            pass
    return default

def to_rgba(rgb, a=1.0):
    return f"{rgb[0]/255.0:.4f} {rgb[1]/255.0:.4f} {rgb[2]/255.0:.4f} {a:.4f}"

accent = "#00e676"
btn_style = "rectangles"
radius = 8
anim_on = True
if os.path.isfile(panel_path):
    try:
        with open(panel_path, "r", encoding="utf-8") as f:
            pc = json.load(f)
        accent = pc.get("accent_color", accent)
        btn_style = str(pc.get("window_buttons_style", btn_style)).lower()
        radius = int(pc.get("window_corner_radius", radius))
        anim_on = bool(pc.get("animations", True))
    except Exception:
        pass

acc_rgb = parse_hex(accent)
title_rgb = tuple(int(b * 0.88 + a * 0.12) for b, a in zip((24, 24, 37), acc_rgb))
wf_layout = "close minimize maximize" if btn_style == "circles_left" else "minimize maximize close"
pix_layout = "close,maximize,minimize:" if btn_style == "circles_left" else ":minimize,maximize,close"

with open(wf_path, "r", encoding="utf-8") as f:
    raw = f.read()

def patch_section(txt, sec, updates, drop=()):
    lines = txt.splitlines()
    out = []
    cur = ""
    found = False
    skip_dup = False
    pend = dict(updates)
    drop_s = set(drop)
    for ln in lines:
        s = ln.strip()
        if s.startswith("[") and s.endswith("]"):
            if cur == sec and pend:
                for k, v in pend.items():
                    out.append(f"{k} = {v}")
                pend.clear()
            sec_name = s[1:-1].strip()
            if sec_name == sec:
                if not found:
                    found = True
                    cur = sec_name
                    skip_dup = False
                    out.append(ln)
                else:
                    cur = ""
                    skip_dup = True
            else:
                cur = sec_name
                skip_dup = False
                out.append(ln)
            continue
        if skip_dup:
            continue
        if cur == sec and "=" in s and not s.startswith("#") and not s.startswith(";"):
            k = s.split("=", 1)[0].strip()
            if k in drop_s and k not in pend:
                continue
            if k in pend:
                out.append(f"{k} = {pend.pop(k)}")
                continue
        out.append(ln)
    if cur == sec and pend:
        for k, v in pend.items():
            out.append(f"{k} = {v}")
        pend.clear()
    elif not found and pend:
        if out and out[-1].strip():
            out.append("")
        out.append(f"[{sec}]")
        for k, v in pend.items():
            out.append(f"{k} = {v}")
    return "\n".join(out).rstrip() + "\n"

if anim_on:
    raw = patch_section(raw, "animate", {
        "open_animation": "zoom",
        "close_animation": "zoom",
        "minimize_animation": "squeezimize",
        "duration": "220",
        "zoom_duration": "220",
        "fade_duration": "180",
        "squeezimize_duration": "250",
        "fade_enabled_for": '(type is "overlay" | (type is "x-or" & focusable is true))',
    })
raw = patch_section(raw, "decoration", {
    "button_order": wf_layout,
    "active_color": to_rgba(acc_rgb, 1.0),
    "inactive_color": to_rgba((49, 50, 68), 1.0),
    "border_size": "2",
})
raw = patch_section(raw, "pixdecor", {
    "titlebar": "true",
    "csd_titlebar_height": "28",
    "border_size": "2",
    "rounded_corner_radius": str(max(0, min(32, radius))),
    "overlay_engine": "rounded_corners" if radius > 0 else "none",
    "always_decorate": "all",
    "button_layout": pix_layout,
    "fg_color": to_rgba(title_rgb, 1.0),
    "bg_color": to_rgba((30, 30, 46), 1.0),
    "fg_text_color": "0.9412 0.9490 0.9765 1.0000",
    "bg_text_color": "0.6510 0.6784 0.7843 1.0000",
    "effect_color": to_rgba(acc_rgb, 1.0),
    "maximized_borders": "true",
    "maximized_shadows": "true",
}, drop=("rounded_corners", "active_color", "inactive_color", "button_order"))

fd, tmp = tempfile.mkstemp(prefix=".tmp-", dir=os.path.dirname(os.path.abspath(wf_path)))
with os.fdopen(fd, "w", encoding="utf-8") as out:
    out.write(raw)
    out.flush()
    os.fsync(out.fileno())
os.replace(tmp, wf_path)
PYEOF
        fi
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

    copy_dir_missing config/foot "$XDG_CFG/foot" 644
    copy_dir_missing config/mako "$XDG_CFG/mako" 644
    copy_dir_missing config/gtk-3.0 "$XDG_CFG/gtk-3.0" 644
    copy_dir_missing config/gtk-4.0 "$XDG_CFG/gtk-4.0" 644
    copy_dir_missing config/pcmanfm "$XDG_CFG/pcmanfm" 644

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
    # Usunięcie przestarzałego fuzzel (zastąpionego natywnym menu w okay-panel) + migracja wayfire.ini
    rm -rf "$XDG_CFG/fuzzel" "$XDG_CACHE/fuzzel" 2>/dev/null || true
    if [ -f "$XDG_CFG/wayfire.ini" ]; then
        sed -i -E 's|^command_launcher[[:space:]]*=[[:space:]]*fuzzel$|command_launcher = okay-panel --toggle-menu|' "$XDG_CFG/wayfire.ini" 2>/dev/null || true
        sed -i -E 's/\|[[:space:]]*app_id is "fuzzel"/| app_id is "okay-menu" | app_id is "okay-powermenu" | app_id is "okay-clipboard"/g' "$XDG_CFG/wayfire.ini" 2>/dev/null || true
    fi

    # Migracja istniejącego autostartu użytkownika dla okay-desktop oraz układu przycisków okien CSD (gsettings)
    if [ -f "$OKAY_CFG_DIR/autostart" ] && ! grep -q "okay-desktop" "$OKAY_CFG_DIR/autostart" 2>/dev/null; then
        printf '\n# Natywny pulpit OKay DE\n(\n    sleep 0.3\n    DESKTOP_LOG="${XDG_RUNTIME_DIR:-/tmp}/okay-desktop.log"\n    if [ -x "$HOME/.local/bin/okay-desktop" ]; then\n        "$HOME/.local/bin/okay-desktop" > "$DESKTOP_LOG" 2>&1 &\n    elif command -v okay-desktop >/dev/null 2>&1; then\n        okay-desktop > "$DESKTOP_LOG" 2>&1 &\n    fi\n) &\n' >> "$OKAY_CFG_DIR/autostart"
    fi
    if [ -f "$OKAY_CFG_DIR/autostart" ] && ! grep -q "org.gnome.desktop.wm.preferences" "$OKAY_CFG_DIR/autostart" 2>/dev/null; then
        printf '\n# Układ przycisków okien CSD (minimalizacja, maksymalizacja, zamknięcie zamiast stylu GNOME)\nif command -v gsettings >/dev/null 2>&1; then\n    (\n        BTN_LAYOUT=":minimize,maximize,close"\n        PANEL_JSON="${XDG_CONFIG_HOME:-$HOME/.config}/okay-de/panel.json"\n        if [ -f "$PANEL_JSON" ] && grep -qE '\''"window_buttons_style"[[:space:]]*:[[:space:]]*"circles_left"'\'' "$PANEL_JSON" 2>/dev/null; then\n            BTN_LAYOUT="close,maximize,minimize:"\n        fi\n        gsettings set org.gnome.desktop.wm.preferences button-layout "$BTN_LAYOUT" 2>/dev/null || true\n    ) &\nfi\n' >> "$OKAY_CFG_DIR/autostart"
    fi

    # Synchronizacja ustawień GTK3/GTK4 (settings.ini, gtk.css), GSettings (button-layout) oraz Wayfire z nowym kodem
    if command -v python3 >/dev/null 2>&1 && [ -f "$OKAY_DATA_DIR/settings/helpers.py" ]; then
        PYTHONPATH="$OKAY_DATA_DIR" python3 -c "from settings.helpers import apply_window_decorations; apply_window_decorations(_from_ensure=True)" >/dev/null 2>&1 || true
    fi

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

    # Przeładowanie sesji Wayfire bez błędu (restart okay-panel i okay-desktop)
    if [ "${OKAY_DE_SKIP_RELOAD:-0}" != "1" ] && [ -n "${WAYLAND_DISPLAY:-}" ]; then
        if [ -x "$BIN_DIR/okay-panel" ]; then
            nohup "$BIN_DIR/okay-panel" --restart </dev/null >/dev/null 2>&1 &
        elif command -v okay-panel >/dev/null 2>&1; then
            nohup okay-panel --restart </dev/null >/dev/null 2>&1 &
        fi
        if [ -x "$BIN_DIR/okay-desktop" ]; then
            nohup "$BIN_DIR/okay-desktop" --restart </dev/null >/dev/null 2>&1 &
        elif command -v okay-desktop >/dev/null 2>&1; then
            nohup okay-desktop --restart </dev/null >/dev/null 2>&1 &
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
