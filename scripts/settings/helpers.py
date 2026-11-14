# -*- coding: utf-8 -*-

"""
Wspólne narzędzia GUI (GTK3), stylistyka OKsavi oraz asynchroniczne wykonywanie poleceń
dla modułów centrum sterowania OKay Settings.
"""

import os
import re
import sys
import copy
import glob
import json
import stat
import time
import shutil
import tarfile
import tempfile
import platform
import threading
import subprocess
import urllib.request
import urllib.error

import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gtk, Gdk, Gio, GLib, Pango


DEFAULT_ACCENT_COLOR = "#00e676"

COLOR_PRESETS = [
    ("#00e676", "OKsavi Neon"),
    ("#c9a0dc", "Wisteria Fiolet"),
    ("#89b4fa", "Catppuccin Blue"),
    ("#cba6f7", "Mauve Fiolet"),
    ("#94e2d5", "Teal Morski"),
    ("#fab387", "Peach Brzoskwinia"),
    ("#f5c2e7", "Flamingo Róż"),
    ("#f38ba8", "Red Czerwień"),
    ("#f9e2af", "Sun Słoneczny"),
    ("#b4befe", "Lavender Lawenda"),
]


def sanitize_hex_color(value: str, default: str = DEFAULT_ACCENT_COLOR) -> str:
    if not isinstance(value, str):
        return default
    raw = value.strip()
    if not raw.startswith("#"):
        raw = "#" + raw
    if re.fullmatch(r"#[0-9a-fA-F]{3}", raw):
        raw = "#" + "".join(c * 2 for c in raw[1:])
    if re.fullmatch(r"#[0-9a-fA-F]{6}", raw):
        return raw.lower()
    return default


def parse_hex_rgb(hex_color: str, default: tuple = (0, 230, 118)) -> tuple:
    clean = sanitize_hex_color(hex_color, "")
    if not clean:
        return default
    return (int(clean[1:3], 16), int(clean[3:5], 16), int(clean[5:7], 16))


def rgb_to_hex(r: int, g: int, b: int) -> str:
    return f"#{max(0, min(255, int(r))):02x}{max(0, min(255, int(g))):02x}{max(0, min(255, int(b))):02x}"


def compute_hover_hex(hex_color: str) -> str:
    r, g, b = parse_hex_rgb(hex_color)
    return rgb_to_hex(
        r + (255 - r) * 0.18,
        g + (255 - g) * 0.18,
        b + (255 - b) * 0.18,
    )


def compute_contrast_text(hex_color: str) -> str:
    r, g, b = parse_hex_rgb(hex_color)
    luminance = 0.299 * r + 0.587 * g + 0.114 * b
    return "#11111b" if luminance > 140 else "#ffffff"


def blend_hex_color(base_hex: str, accent_hex: str, ratio: float = 0.08) -> str:
    br, bg, bb = parse_hex_rgb(base_hex, (30, 30, 46))
    ar, ag, ab = parse_hex_rgb(accent_hex)
    return rgb_to_hex(
        br * (1.0 - ratio) + ar * ratio,
        bg * (1.0 - ratio) + ag * ratio,
        bb * (1.0 - ratio) + ab * ratio,
    )


def hex_to_wf_rgba(hex_color: str, alpha: float = 1.0) -> str:
    """Konwertuje kolor #RRGGBB na format RGBA (float 0.0-1.0) wymagany przez wf-config / Wayfire."""
    r, g, b = parse_hex_rgb(hex_color)
    a = max(0.0, min(1.0, float(alpha)))
    return f"{r / 255.0:.4f} {g / 255.0:.4f} {b / 255.0:.4f} {a:.4f}"


def is_pixdecor_installed() -> bool:
    """Sprawdza, czy wtyczka Wayfire pixdecor (libpixdecor.so) jest dostępna w systemie lub katalogu użytkownika."""
    candidates = [
        os.path.join(xdg_data_home(), "wayfire", "plugins", "libpixdecor.so"),
        "/usr/lib/wayfire/libpixdecor.so",
        "/usr/local/lib/wayfire/libpixdecor.so",
        "/usr/lib/x86_64-linux-gnu/wayfire/libpixdecor.so",
        "/usr/lib/aarch64-linux-gnu/wayfire/libpixdecor.so",
    ]
    extra_env = os.environ.get("WAYFIRE_PLUGIN_PATH", "")
    for d in extra_env.split(":"):
        if d.strip():
            candidates.append(os.path.join(d.strip(), "libpixdecor.so"))
    return any(os.path.isfile(p) for p in candidates)


def ensure_wayfire_decorator_plugin() -> bool:
    """Synchronizuje w [core] plugins aktywną wtyczkę dekoracji (pixdecor gdy dostępna, inaczej decoration)."""
    wf_ini = wayfire_config_file()
    if not os.path.isfile(wf_ini):
        return False
    plugins_line = read_wayfire_value("core", "plugins", "")
    if not plugins_line:
        return False
    tokens = plugins_line.split()
    has_pix = is_pixdecor_installed()
    if has_pix and "pixdecor" not in tokens:
        if "decoration" in tokens:
            new_tokens = ["pixdecor" if t == "decoration" else t for t in tokens]
        else:
            new_tokens = tokens + ["pixdecor"]
        update_wayfire_section("core", {"plugins": " ".join(new_tokens)})
        return True
    if not has_pix and "pixdecor" in tokens:
        if "decoration" not in tokens:
            new_tokens = ["decoration" if t == "pixdecor" else t for t in tokens]
        else:
            new_tokens = [t for t in tokens if t != "pixdecor"]
        update_wayfire_section("core", {"plugins": " ".join(new_tokens)})
        return True
    return False


SETTINGS_CSS_TEMPLATE = """
window#okay-settings {
    background-color: __BG_WIN__;
    color: __FG_MAIN__;
    font-family: "Inter", "JetBrains Mono", system-ui, sans-serif;
    font-size: 13px;
}

.settings-sidebar {
    background-color: __BG_SIDEBAR__;
    border-right: 1px solid __BORDER_COL__;
    padding: 8px 6px;
}

.settings-sidebar list {
    background-color: __BG_SIDEBAR__;
}

.settings-sidebar row {
    color: __FG_MAIN__;
    border-radius: 6px;
    margin: 2px 4px;
    padding: 10px 14px;
    font-size: 13.5px;
    font-weight: 500;
}

.settings-sidebar row:hover {
    background-color: __BG_HOVER__;
    color: __FG_MAIN__;
}

.settings-sidebar row:selected {
    background-color: rgba(__ACCENT_RGB__, 0.16);
    color: __ACCENT__;
    font-weight: 700;
    border-left: 3px solid __ACCENT__;
}

.settings-brand {
    background-color: __BG_SIDEBAR__;
    border-right: 1px solid __BORDER_COL__;
    border-bottom: 1px solid __BORDER_COL__;
    padding: 14px 16px;
}

.brand-title {
    color: __ACCENT__;
    font-size: 15.5px;
    font-weight: 800;
    letter-spacing: 0.5px;
}

.brand-subtitle {
    color: __FG_SUB__;
    font-size: 12px;
}

.page-title {
    color: __FG_MAIN__;
    font-size: 20px;
    font-weight: 700;
}

.page-subtitle {
    color: __FG_SUB__;
    font-size: 13px;
}

.card-frame {
    background-color: __BG_CARD__;
    border: 1px solid rgba(__ACCENT_RGB__, 0.22);
    border-radius: 10px;
    padding: 14px 16px;
}

.card-title {
    color: __ACCENT__;
    font-size: 12.5px;
    font-weight: 700;
    letter-spacing: 0.6px;
    margin-bottom: 8px;
}

.row-title {
    color: __FG_MAIN__;
    font-size: 13.5px;
    font-weight: 600;
}

.row-subtitle {
    color: __FG_SUB__;
    font-size: 12px;
}

.status-ok {
    color: __ACCENT__;
    font-weight: 600;
    font-size: 12.5px;
}

.status-warn {
    color: #f9e2af;
    font-weight: 600;
    font-size: 12.5px;
}

.status-err {
    color: #f38ba8;
    font-weight: 600;
    font-size: 12.5px;
}

.badge-connected {
    background-color: rgba(__ACCENT_RGB__, 0.16);
    color: __ACCENT__;
    border: 1px solid rgba(__ACCENT_RGB__, 0.45);
    border-radius: 5px;
    padding: 3px 8px;
    font-size: 12px;
    font-weight: 700;
}

list.settings-list {
    background-color: __BG_LIST__;
    border: 1px solid __BORDER_COL__;
    border-radius: 8px;
}

list.settings-list row {
    padding: 9px 12px;
    border-bottom: 1px solid __BORDER_COL__;
    color: __FG_MAIN__;
    font-size: 13px;
}

list.settings-list row:last-child {
    border-bottom: none;
}

list.settings-list row:hover {
    background-color: __BG_HOVER__;
}

list.settings-list row:selected {
    background-color: rgba(__ACCENT_RGB__, 0.14);
    color: __FG_MAIN__;
}

button.accent-btn {
    background-color: __ACCENT__;
    background-image: none;
    color: __ACCENT_CONTRAST__;
    border: none;
    border-radius: 6px;
    padding: 7px 15px;
    font-weight: 700;
    font-size: 12.5px;
}

button.accent-btn:hover {
    background-color: __ACCENT_HOVER__;
}

button.accent-btn:disabled {
    background-color: __BG_BTN__;
    color: __FG_SUB__;
}

button.secondary-btn {
    background-color: __BG_BTN__;
    background-image: none;
    color: __FG_MAIN__;
    border: 1px solid __BORDER_COL__;
    border-radius: 6px;
    padding: 7px 13px;
    font-weight: 600;
    font-size: 12.5px;
}

button.secondary-btn:hover {
    background-color: __BG_HOVER__;
    border-color: __ACCENT__;
}

button.danger-btn {
    background-color: #2b181c;
    background-image: none;
    color: #f38ba8;
    border: 1px solid #592632;
    border-radius: 6px;
    padding: 7px 13px;
    font-weight: 600;
    font-size: 12.5px;
}

button.danger-btn:hover {
    background-color: #3a1f25;
}

button.scale-pill {
    background-color: __BG_BTN__;
    background-image: none;
    color: __FG_MAIN__;
    border: 1px solid __BORDER_COL__;
    border-radius: 6px;
    padding: 6px 13px;
    font-weight: 600;
    font-size: 12.5px;
}

button.scale-pill:checked {
    background-color: rgba(__ACCENT_RGB__, 0.18);
    color: __ACCENT__;
    border-color: __ACCENT__;
}

button.color-preset-btn {
    background-color: __BG_BTN__;
    background-image: none;
    color: __FG_MAIN__;
    border: 1px solid __BORDER_COL__;
    border-radius: 6px;
    padding: 6px 10px;
    font-weight: 600;
    font-size: 12.5px;
}

button.color-preset-btn:hover {
    background-color: __BG_HOVER__;
    border-color: __ACCENT__;
}

button.color-preset-btn:checked {
    background-color: rgba(__ACCENT_RGB__, 0.16);
    color: __FG_MAIN__;
    border: 1px solid __ACCENT__;
}

switch {
    background-color: __BG_BTN__;
    border: 1px solid __BORDER_COL__;
    border-radius: 12px;
}

switch:checked {
    background-color: __ACCENT__;
    border-color: __ACCENT__;
}

switch slider {
    background-color: #ffffff;
    border-radius: 10px;
}

scale highlight {
    background-color: __ACCENT__;
    border-radius: 3px;
}

scale trough {
    background-color: __BG_BTN__;
    border-radius: 3px;
    min-height: 6px;
}

scale slider {
    background-color: __ACCENT__;
    border: 2px solid __BG_WIN__;
    border-radius: 10px;
    min-width: 16px;
    min-height: 16px;
}

entry, combobox button {
    background-color: __BG_ENTRY__;
    background-image: none;
    color: __FG_MAIN__;
    border: 1px solid __BORDER_COL__;
    border-radius: 6px;
    padding: 6px 10px;
    font-size: 12.5px;
}

entry:focus {
    border-color: __ACCENT__;
}

menu,
combobox menu,
combobox window menu {
    background-color: __BG_CARD__;
    background-image: none;
    color: __FG_MAIN__;
    border: 1px solid rgba(__ACCENT_RGB__, 0.45);
    border-radius: 6px;
    padding: 4px 0;
}

menu menuitem,
combobox menuitem {
    background-color: transparent;
    background-image: none;
    color: __FG_MAIN__;
    padding: 6px 12px;
    font-size: 12.5px;
}

menu menuitem label,
combobox menuitem label,
combobox cellview {
    color: __FG_MAIN__;
}

menu menuitem:hover,
combobox menuitem:hover {
    background-color: rgba(__ACCENT_RGB__, 0.22);
    color: __FG_MAIN__;
}

menu menuitem:hover label,
combobox menuitem:hover label {
    color: __FG_MAIN__;
}

.settings-sidebar row,
list.settings-list row,
button.accent-btn,
button.secondary-btn,
button.danger-btn,
button.scale-pill,
button.color-preset-btn {
    transition: all 140ms ease-out;
}

window.no-animations .settings-sidebar row,
window.no-animations list.settings-list row,
window.no-animations button.accent-btn,
window.no-animations button.secondary-btn,
window.no-animations button.danger-btn,
window.no-animations button.scale-pill,
window.no-animations button.color-preset-btn,
window.no-animations * {
    transition: none;
}
"""

_SETTINGS_CSS_PROVIDER = None


def get_current_theme_mode() -> str:
    panel_json = os.path.join(okay_config_dir(), "panel.json")
    candidates = (panel_json, okay_template_file("panel.json"))
    for path in candidates:
        if path and os.path.exists(path):
            try:
                with open(path, "r", encoding="utf-8") as f:
                    parsed = json.load(f)
                if isinstance(parsed, dict) and "theme_mode" in parsed:
                    m = str(parsed["theme_mode"]).strip().lower()
                    if m in ("dark", "light"):
                        return m
            except Exception:
                pass
    return "dark"


def get_settings_css(accent_hex: str = DEFAULT_ACCENT_COLOR, theme_mode: str = None) -> str:
    clean = sanitize_hex_color(accent_hex)
    mode = (theme_mode or get_current_theme_mode() or "dark").lower()
    r, g, b = parse_hex_rgb(clean)
    hover = compute_hover_hex(clean)
    contrast = compute_contrast_text(clean)
    if mode == "light":
        bg_win = blend_hex_color("#f4f5f8", clean, 0.03)
        bg_sidebar = blend_hex_color("#ebedf2", clean, 0.04)
        bg_card = "#ffffff"
        bg_list = "#f8f9fc"
        bg_btn = "#e6e8f0"
        bg_hover = "#dcdff0"
        bg_entry = "#ffffff"
        fg_main = "#181825"
        fg_sub = "#4c4f69"
        border_col = "#d0d4e0"
    else:
        bg_win = blend_hex_color("#111111", clean, 0.035)
        bg_sidebar = blend_hex_color("#151515", clean, 0.045)
        bg_card = blend_hex_color("#1a1a1a", clean, 0.045)
        bg_list = "#161616"
        bg_btn = "#242424"
        bg_hover = "#2c2c34"
        bg_entry = "#121212"
        fg_main = "#f5f5f5"
        fg_sub = "#bac2de"
        border_col = "#282828"

    return (
        SETTINGS_CSS_TEMPLATE
        .replace("__ACCENT__", clean)
        .replace("__ACCENT_RGB__", f"{r}, {g}, {b}")
        .replace("__ACCENT_HOVER__", hover)
        .replace("__ACCENT_CONTRAST__", contrast)
        .replace("__BG_WIN__", bg_win)
        .replace("__BG_SIDEBAR__", bg_sidebar)
        .replace("__BG_CARD__", bg_card)
        .replace("__BG_LIST__", bg_list)
        .replace("__BG_BTN__", bg_btn)
        .replace("__BG_HOVER__", bg_hover)
        .replace("__BG_ENTRY__", bg_entry)
        .replace("__FG_MAIN__", fg_main)
        .replace("__FG_SUB__", fg_sub)
        .replace("__BORDER_COL__", border_col)
    )


def get_current_accent_color() -> str:
    panel_json = os.path.join(okay_config_dir(), "panel.json")
    candidates = (panel_json, okay_template_file("panel.json"))
    for path in candidates:
        if path and os.path.exists(path):
            try:
                with open(path, "r", encoding="utf-8") as f:
                    parsed = json.load(f)
                if isinstance(parsed, dict) and "accent_color" in parsed:
                    return sanitize_hex_color(str(parsed["accent_color"]))
            except Exception:
                pass
    return DEFAULT_ACCENT_COLOR


def apply_settings_css(accent_hex: str = None, theme_mode: str = None):
    """Rejestruje lub przeładowuje dynamiczny styl CSS OKay DE dla okna OKay Settings."""
    global _SETTINGS_CSS_PROVIDER
    target_hex = sanitize_hex_color(accent_hex) if accent_hex else get_current_accent_color()
    target_mode = theme_mode if theme_mode in ("dark", "light") else get_current_theme_mode()
    css_data = get_settings_css(target_hex, target_mode).encode("utf-8")
    if _SETTINGS_CSS_PROVIDER is None:
        _SETTINGS_CSS_PROVIDER = Gtk.CssProvider()
        screen = Gdk.Screen.get_default()
        if screen is not None:
            Gtk.StyleContext.add_provider_for_screen(
                screen,
                _SETTINGS_CSS_PROVIDER,
                Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
            )
    try:
        _SETTINGS_CSS_PROVIDER.load_from_data(css_data)
    except Exception as exc:
        print(f"[okay-settings] Ostrzeżenie ładowania CSS: {exc}", file=sys.stderr)


def run_async(worker_fn, on_done=None, on_error=None):
    """Uruchamia funkcję w osobnym wątku i bezpiecznie przekazuje wynik do pętli GTK przez GLib.idle_add."""
    def _target():
        try:
            res = worker_fn()
            if on_done is not None:
                GLib.idle_add(lambda r=res: (on_done(r), False)[1])
        except Exception as exc:
            if on_error is not None:
                GLib.idle_add(lambda e=exc: (on_error(e), False)[1])

    t = threading.Thread(target=_target, daemon=True)
    t.start()
    return t


def run_cmd(cmd, timeout=8) -> tuple:
    """Wykonuje polecenie systemowe i zwraca (returncode, stdout, stderr)."""
    try:
        proc = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False
        )
        return proc.returncode, proc.stdout, proc.stderr
    except FileNotFoundError:
        return 127, "", f"Nie znaleziono polecenia: {cmd[0]}"
    except subprocess.TimeoutExpired:
        return 124, "", f"Przekroczono limit czasu polecenia: {' '.join(cmd)}"
    except Exception as exc:
        return 1, "", str(exc)


def clear_container(container) -> None:
    """Usuwa i niszczy wszystkie widżety podrzędne kontenera GTK3, zwalniając zasoby."""
    for child in container.get_children():
        container.remove(child)
        child.destroy()


def make_scrolled_page() -> tuple:
    """Tworzy przewijalną stronę z wewnętrznym kontenerem pionowym Gtk.Box."""
    sw = Gtk.ScrolledWindow()
    sw.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
    sw.set_shadow_type(Gtk.ShadowType.NONE)

    vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=14)
    vbox.set_margin_top(20)
    vbox.set_margin_bottom(24)
    vbox.set_margin_start(24)
    vbox.set_margin_end(24)
    sw.add(vbox)
    return sw, vbox


def make_page_header(title: str, subtitle: str, right_widget=None) -> Gtk.Box:
    """Buduje jednolity nagłówek zakładki ustawień."""
    hbox = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)

    text_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=3)
    lbl_title = Gtk.Label(label=title, xalign=0.0)
    lbl_title.get_style_context().add_class("page-title")
    lbl_sub = Gtk.Label(label=subtitle, xalign=0.0)
    lbl_sub.set_line_wrap(True)
    lbl_sub.get_style_context().add_class("page-subtitle")

    text_box.pack_start(lbl_title, False, False, 0)
    text_box.pack_start(lbl_sub, False, False, 0)
    hbox.pack_start(text_box, True, True, 0)

    if right_widget is not None:
        right_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        right_box.set_valign(Gtk.Align.CENTER)
        right_box.pack_start(right_widget, False, False, 0)
        hbox.pack_end(right_box, False, False, 0)

    return hbox


def make_card(title: str = "") -> tuple:
    """Tworzy kartę sekcji (.card-frame) i zwraca (frame, inner_vbox)."""
    frame = Gtk.Frame()
    frame.set_shadow_type(Gtk.ShadowType.NONE)
    frame.get_style_context().add_class("card-frame")

    inner = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
    frame.add(inner)

    if title:
        lbl = Gtk.Label(label=title.upper(), xalign=0.0)
        lbl.get_style_context().add_class("card-title")
        inner.pack_start(lbl, False, False, 0)

    return frame, inner


def make_setting_row(title: str, subtitle: str, control_widget) -> Gtk.Box:
    """Tworzy pojedynczy wiersz opcji z tytułem, opisem i kontrolką po prawej."""
    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
    row.set_margin_top(2)
    row.set_margin_bottom(2)

    txt = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
    lbl_t = Gtk.Label(label=title, xalign=0.0)
    lbl_t.get_style_context().add_class("row-title")
    txt.pack_start(lbl_t, False, False, 0)

    if subtitle:
        lbl_s = Gtk.Label(label=subtitle, xalign=0.0)
        lbl_s.set_line_wrap(True)
        lbl_s.set_ellipsize(Pango.EllipsizeMode.NONE)
        lbl_s.get_style_context().add_class("row-subtitle")
        txt.pack_start(lbl_s, False, False, 0)

    row.pack_start(txt, True, True, 0)
    control_widget.set_valign(Gtk.Align.CENTER)
    row.pack_end(control_widget, False, False, 0)
    return row


class SystemHelper:
    """Odczytuje podstawowe dane sprzętowe i systemowe."""

    @staticmethod
    def get_distro_name() -> str:
        os_release = "/etc/os-release"
        if os.path.exists(os_release):
            try:
                with open(os_release, "r", encoding="utf-8") as f:
                    for line in f:
                        if line.startswith("PRETTY_NAME="):
                            return line.split("=", 1)[1].strip().strip('"')
                        if line.startswith("NAME="):
                            return line.split("=", 1)[1].strip().strip('"')
            except Exception:
                pass
        return "OKsavi Linux"

    @staticmethod
    def get_kernel_release() -> str:
        return platform.release()

    @staticmethod
    def get_cpu_info() -> str:
        if os.path.exists("/proc/cpuinfo"):
            try:
                with open("/proc/cpuinfo", "r", encoding="utf-8") as f:
                    for line in f:
                        low = line.lower()
                        if low.startswith(("model name", "hardware", "model\t", "model :")) and ":" in line:
                            return line.split(":", 1)[1].strip()
            except Exception:
                pass
        return platform.processor() or "Nieznany procesor"

    @staticmethod
    def get_memory_info() -> str:
        if os.path.exists("/proc/meminfo"):
            try:
                with open("/proc/meminfo", "r", encoding="utf-8") as f:
                    for line in f:
                        if line.startswith("MemTotal:"):
                            kb = int(line.split()[1])
                            return f"{kb / (1024 * 1024):.1f} GiB RAM"
            except Exception:
                pass
        return "Nieznana"


# ============================================================================
# Ścieżki XDG Base Directory, szablony read-only i zapis atomowy
# ============================================================================

DEFAULT_PANEL_CONFIG = {
    "theme_mode": "dark",
    "icon_only": True,
    "show_labels": False,
    "position": "bottom",
    "mode": "panel",
    "floating": False,
    "height": 44,
    "opacity": 0.90,
    "animations": True,
    "reduce_motion": False,
    "animation_speed": 1.0,
    "open_effect": "zoom",
    "minimize_effect": "squeezimize",
    "blur_enabled": False,
    "wobbly_enabled": False,
    "window_opacity": 1.0,
    "focus_mode": False,
    "idle_timeout": 600,
    "workspace_count": 3,
    "accent_color": DEFAULT_ACCENT_COLOR,
    "window_buttons_style": "rectangles",
    "window_corner_radius": 12,
    "window_rounding": 12,
    "show_clock": True,
    "show_seconds": False,
    "show_volume": True,
    "show_network": True,
    "show_battery": True,
    "show_sysload": True,
    "show_workspaces": True,
    "pinned_apps": [
        {
            "app_id": "foot",
            "name": "Terminal (foot)",
            "cmd": "foot",
            "icon": "utilities-terminal",
        },
        {
            "app_id": "pcmanfm",
            "name": "Menedżer plików",
            "cmd": "pcmanfm",
            "icon": "system-file-manager",
        },
        {
            "app_id": "okay-settings",
            "name": "Ustawienia OKay",
            "cmd": "okay-settings",
            "icon": "preferences-system",
        },
    ],
}

def xdg_config_home() -> str:
    val = os.environ.get("XDG_CONFIG_HOME", "").strip()
    return os.path.expanduser(val) if val else os.path.expanduser("~/.config")


def xdg_data_home() -> str:
    val = os.environ.get("XDG_DATA_HOME", "").strip()
    return os.path.expanduser(val) if val else os.path.expanduser("~/.local/share")


def xdg_bin_home() -> str:
    return os.path.expanduser("~/.local/bin")


def okay_config_dir() -> str:
    return os.path.join(xdg_config_home(), "okay-de")


def wayfire_config_file() -> str:
    return os.path.join(xdg_config_home(), "wayfire.ini")


def wayfire_template_file() -> str:
    candidates = [
        "/etc/okay-de/wayfire.ini",
        "/etc/xdg/okay-de/wayfire.ini",
        "/usr/share/okay-de/defaults/wayfire/wayfire.ini",
        "/usr/local/share/okay-de/defaults/wayfire/wayfire.ini",
        os.path.join(okay_data_dir(), "defaults", "wayfire", "wayfire.ini"),
        os.path.join(okay_data_dir(), "repo", "config", "wayfire", "wayfire.ini"),
    ]
    repo_root = _repo_root_from_script()
    if repo_root:
        candidates.append(os.path.join(repo_root, "config", "wayfire", "wayfire.ini"))
    return find_existing_file(*candidates)


def read_wayfire_value(section: str, key: str, default: str = "") -> str:
    """Odczytuje wartość klucza z wybranej sekcji ~/.config/wayfire.ini."""
    wf_ini = wayfire_config_file()
    if not os.path.isfile(wf_ini):
        wf_ini = wayfire_template_file()
    if not wf_ini or not os.path.isfile(wf_ini):
        return default
    target_sec = f"[{section}]"
    in_sec = False
    try:
        with open(wf_ini, "r", encoding="utf-8") as f:
            for raw in f:
                s = raw.strip()
                if not s or s.startswith("#") or s.startswith(";"):
                    continue
                if s.startswith("[") and s.endswith("]"):
                    in_sec = (s == target_sec)
                    continue
                if in_sec and "=" in s:
                    k, v = s.split("=", 1)
                    if k.strip() == key:
                        return v.strip()
    except Exception:
        pass
    return default


def update_wayfire_section(section: str, updates: dict, remove_keys: set = None) -> bool:
    """Atomowo aktualizuje, dopisuje lub usuwa klucze w sekcji [section] pliku ~/.config/wayfire.ini (usuwając duplikaty sekcji)."""
    wf_ini = wayfire_config_file()
    if not os.path.isfile(wf_ini):
        tmpl = wayfire_template_file()
        if tmpl and os.path.isfile(tmpl):
            try:
                atomic_copy_file(tmpl, wf_ini, mode=0o644)
            except Exception:
                pass
    text = ""
    if os.path.isfile(wf_ini):
        try:
            with open(wf_ini, "r", encoding="utf-8") as f:
                text = f.read()
        except Exception:
            text = ""

    lines = text.splitlines()
    out_lines = []
    target_sec = f"[{section}]"
    in_sec = False
    sec_found = False
    skip_dup_sec = False
    pending = {str(k): str(v) for k, v in updates.items()}
    drop_set = {str(k) for k in remove_keys} if remove_keys else set()

    def _flush_pending():
        for pk, pv in pending.items():
            out_lines.append(f"{pk} = {pv}")
        pending.clear()

    for line in lines:
        s = line.strip()
        if s.startswith("[") and s.endswith("]"):
            if in_sec and pending:
                _flush_pending()
            if s == target_sec:
                if not sec_found:
                    sec_found = True
                    in_sec = True
                    skip_dup_sec = False
                    out_lines.append(line)
                else:
                    in_sec = False
                    skip_dup_sec = True
            else:
                in_sec = False
                skip_dup_sec = False
                out_lines.append(line)
            continue
        if skip_dup_sec:
            continue
        if in_sec and "=" in s and not s.startswith("#") and not s.startswith(";"):
            k = s.split("=", 1)[0].strip()
            if k in drop_set and k not in pending:
                continue
            if k in pending:
                out_lines.append(f"{k} = {pending.pop(k)}")
                continue
        out_lines.append(line)

    if in_sec and pending:
        _flush_pending()
    elif not sec_found and pending:
        if out_lines and out_lines[-1].strip():
            out_lines.append("")
        out_lines.append(target_sec)
        _flush_pending()

    try:
        atomic_write_text(wf_ini, "\n".join(out_lines).rstrip() + "\n", mode=0o644)
        return True
    except Exception:
        return False


def okay_data_dir() -> str:
    return os.path.join(xdg_data_home(), "okay-de")


def _repo_root_from_script() -> str:
    here = os.path.dirname(os.path.realpath(__file__))
    candidate = os.path.abspath(os.path.join(here, "..", ".."))
    if os.path.isfile(os.path.join(candidate, "install.sh")):
        return candidate
    return ""


def find_existing_file(*candidates: str) -> str:
    for path in candidates:
        if path and os.path.isfile(path):
            return path
    return ""


def okay_template_dirs() -> tuple:
    dirs = [
        "/etc/okay-de",
        "/etc/xdg/okay-de",
        "/usr/share/okay-de/defaults/okay-de",
        "/usr/share/okay-de",
        "/usr/local/share/okay-de/defaults/okay-de",
        "/usr/local/share/okay-de",
        os.path.join(okay_data_dir(), "defaults", "okay-de"),
        os.path.join(okay_data_dir(), "repo", "config", "okay-de"),
    ]
    repo_root = _repo_root_from_script()
    if repo_root:
        dirs.append(os.path.join(repo_root, "config", "okay-de"))
    return tuple(dirs)


def okay_template_file(rel_path: str) -> str:
    return find_existing_file(*(os.path.join(d, rel_path) for d in okay_template_dirs()))


def wallpaper_template_file() -> str:
    candidates = [
        "/etc/okay-de/wallpaper.png",
        "/usr/share/okay-de/defaults/backgrounds/default.png",
        "/usr/share/okay-de/wallpaper.png",
        "/usr/share/backgrounds/okay-de/default.png",
        "/usr/local/share/backgrounds/okay-de/default.png",
        os.path.join(okay_data_dir(), "defaults", "backgrounds", "default.png"),
        os.path.join(okay_data_dir(), "repo", "backgrounds", "default.png"),
    ]
    repo_root = _repo_root_from_script()
    if repo_root:
        candidates.append(os.path.join(repo_root, "backgrounds", "default.png"))
    return find_existing_file(*candidates)


def mako_template_file() -> str:
    candidates = [
        "/etc/xdg/mako/config",
        os.path.join(okay_data_dir(), "defaults", "mako", "config"),
        os.path.join(okay_data_dir(), "repo", "config", "mako", "config"),
    ]
    repo_root = _repo_root_from_script()
    if repo_root:
        candidates.append(os.path.join(repo_root, "config", "mako", "config"))
    return find_existing_file(*candidates)


_DEFAULT_MAKO_CONFIG = """# OKay DE - Konfiguracja demona powiadomień mako
font=JetBrains Mono 10
width=360
height=120
margin=14,14,0,0
padding=12
border-size=2
border-radius=8
icons=1
max-icon-size=48
layer=overlay
anchor=top-right
max-visible=4
background-color=#1b2b30e6
text-color=#cdd6f4
border-color=#00e676
progress-color=over #313244
default-timeout=4500
ignore-timeout=1

[urgency=low]
border-color=#313244
default-timeout=3000

[urgency=normal]
border-color=#00e676
default-timeout=4500

[urgency=critical]
border-color=#f38ba8
text-color=#f38ba8
default-timeout=0
ignore-timeout=0
"""


def atomic_write_bytes(dest_path: str, content: bytes, mode: int = None) -> None:
    """Zapisuje bajty atomowo przez plik tymczasowy w tym samym katalogu i os.replace()."""
    abs_dest = os.path.abspath(dest_path)
    parent = os.path.dirname(abs_dest)
    os.makedirs(parent, exist_ok=True)
    fd, tmp_path = tempfile.mkstemp(prefix=".tmp-", dir=parent)
    try:
        with os.fdopen(fd, "wb") as f:
            f.write(content)
            f.flush()
            try:
                os.fsync(f.fileno())
            except OSError:
                pass
        if mode is not None:
            os.chmod(tmp_path, mode)
        os.replace(tmp_path, abs_dest)
    except Exception:
        try:
            if os.path.exists(tmp_path):
                os.unlink(tmp_path)
        except OSError:
            pass
        raise


def atomic_write_text(dest_path: str, text: str, mode: int = None) -> None:
    atomic_write_bytes(dest_path, text.encode("utf-8"), mode=mode)


def atomic_write_json(dest_path: str, data: dict) -> None:
    atomic_write_text(dest_path, json.dumps(data, indent=2, ensure_ascii=False) + "\n", mode=0o644)


def atomic_copy_file(src_path: str, dest_path: str, mode: int = None) -> None:
    with open(src_path, "rb") as f:
        data = f.read()
    if mode is None:
        try:
            mode = stat.S_IMODE(os.stat(src_path).st_mode)
        except OSError:
            mode = None
    atomic_write_bytes(dest_path, data, mode=mode)


def normalize_panel_config(cfg: dict) -> dict:
    """Waliduje typy, migruje schemat ze starszych wersji i zapewnia bezpieczny fallback ustawień."""
    data = copy.deepcopy(DEFAULT_PANEL_CONFIG)
    raw = cfg if isinstance(cfg, dict) else {}
    data.update(raw)

    theme_mode = str(data.get("theme_mode", "dark")).strip().lower()
    data["theme_mode"] = "light" if theme_mode == "light" else "dark"

    if "icon_only" in raw:
        icon_only = bool(raw["icon_only"])
    elif "show_labels" in raw:
        icon_only = not bool(raw["show_labels"])
    else:
        icon_only = bool(data.get("icon_only", True))
    data["icon_only"] = icon_only
    data["show_labels"] = not icon_only

    pos = str(data.get("position", "bottom")).lower()
    data["position"] = "top" if pos == "top" else "bottom"

    if "mode" in raw:
        mode = "dock" if str(raw["mode"]).lower() == "dock" else "panel"
        floating = (mode == "dock")
    elif "floating" in raw:
        floating = bool(raw["floating"])
        mode = "dock" if floating else "panel"
    else:
        mode = "panel"
        floating = False
    data["mode"] = mode
    data["floating"] = floating

    try:
        data["height"] = max(32, min(72, int(data.get("height", 44))))
    except Exception:
        data["height"] = 44

    try:
        data["opacity"] = max(0.20, min(1.00, round(float(data.get("opacity", 0.90)), 2)))
    except Exception:
        data["opacity"] = 0.90

    # Synchronizacja reduce_motion <-> animations
    if "reduce_motion" in raw and "animations" not in raw:
        reduce_motion = bool(raw["reduce_motion"])
        animations = not reduce_motion
    elif "animations" in raw and "reduce_motion" not in raw:
        animations = bool(raw["animations"])
        reduce_motion = not animations
    else:
        reduce_motion = bool(data.get("reduce_motion", False))
        animations = not reduce_motion
    data["reduce_motion"] = reduce_motion
    data["animations"] = animations

    try:
        speed = float(data.get("animation_speed", 1.0))
        data["animation_speed"] = max(0.5, min(2.0, round(speed, 2)))
    except Exception:
        data["animation_speed"] = 1.0

    open_eff = str(data.get("open_effect", "zoom")).strip().lower()
    data["open_effect"] = open_eff if open_eff in ("zoom", "fade", "fire", "none") else "zoom"

    min_eff = str(data.get("minimize_effect", "squeezimize")).strip().lower()
    data["minimize_effect"] = min_eff if min_eff in ("squeezimize", "zoom", "fade", "blinds", "none") else "squeezimize"

    data["blur_enabled"] = bool(data.get("blur_enabled", False))
    data["wobbly_enabled"] = bool(data.get("wobbly_enabled", False))

    try:
        w_op = float(data.get("window_opacity", 1.0))
        data["window_opacity"] = max(0.50, min(1.00, round(w_op, 2)))
    except Exception:
        data["window_opacity"] = 1.0

    data["focus_mode"] = bool(data.get("focus_mode", False))

    try:
        idle_s = int(data.get("idle_timeout", 600))
        data["idle_timeout"] = max(0, min(7200, idle_s))
    except Exception:
        data["idle_timeout"] = 600

    try:
        ws_cnt = int(data.get("workspace_count", 3))
        data["workspace_count"] = max(1, min(9, ws_cnt))
    except Exception:
        data["workspace_count"] = 3

    raw_accent = str(data.get("accent_color", DEFAULT_ACCENT_COLOR)).strip()
    data["accent_color"] = sanitize_hex_color(raw_accent, DEFAULT_ACCENT_COLOR)

    btn_style = str(data.get("window_buttons_style", "rectangles")).lower()
    data["window_buttons_style"] = (
        btn_style if btn_style in ("rectangles", "circles_left", "circles_right", "windows") else "rectangles"
    )

    try:
        if "window_corner_radius" in raw:
            radius = int(raw["window_corner_radius"])
        elif "window_rounding" in raw:
            radius = int(raw["window_rounding"])
        else:
            radius = int(data.get("window_corner_radius", 12))
        radius = max(0, min(32, radius))
    except Exception:
        radius = 12
    data["window_corner_radius"] = radius
    data["window_rounding"] = radius

    for flag_key in (
        "show_clock",
        "show_seconds",
        "show_volume",
        "show_network",
        "show_battery",
        "show_sysload",
        "show_workspaces",
    ):
        data[flag_key] = bool(data.get(flag_key, DEFAULT_PANEL_CONFIG.get(flag_key, True)))

    if not isinstance(data.get("pinned_apps"), list):
        data["pinned_apps"] = copy.deepcopy(DEFAULT_PANEL_CONFIG["pinned_apps"])
    return data


def _merge_missing_dict(target: dict, source: dict) -> bool:
    changed = False
    for k, v in source.items():
        if k not in target:
            target[k] = copy.deepcopy(v)
            changed = True
        elif isinstance(target[k], dict) and isinstance(v, dict):
            if _merge_missing_dict(target[k], v):
                changed = True
    return changed


def merge_missing_json_defaults(user_json_path: str, defaults: dict, template_path: str = "", normalizer=None) -> dict:
    """Zachowuje ustawienia użytkownika w pliku JSON i dopisuje wyłącznie brakujące klucze domyślne."""
    base = copy.deepcopy(defaults)
    if template_path and os.path.isfile(template_path):
        try:
            with open(template_path, "r", encoding="utf-8") as f:
                tmpl_data = json.load(f)
            if isinstance(tmpl_data, dict):
                base.update(tmpl_data)
        except Exception:
            pass

    def _write_fresh() -> dict:
        fresh = normalizer(base) if normalizer is not None else base
        atomic_write_json(user_json_path, fresh)
        return fresh

    if not os.path.exists(user_json_path):
        return _write_fresh()

    try:
        with open(user_json_path, "r", encoding="utf-8") as f:
            user_data = json.load(f)
    except Exception:
        try:
            atomic_copy_file(user_json_path, user_json_path + ".bak", mode=0o644)
        except Exception:
            pass
        return _write_fresh()

    if not isinstance(user_data, dict):
        return _write_fresh()

    if _merge_missing_dict(user_data, base):
        atomic_write_json(user_json_path, user_data)
    return user_data


def ensure_user_configs() -> None:
    """Inicjalizuje ~/.config/okay-de oraz ~/.config/wayfire.ini z szablonów systemowych przy pierwszym starcie."""
    okay_dir = okay_config_dir()
    os.makedirs(okay_dir, exist_ok=True)

    # 1. Konfiguracja panelu i animacji (~/.config/okay-de/panel.json)
    panel_json = os.path.join(okay_dir, "panel.json")
    if not os.path.exists(panel_json):
        try:
            merge_missing_json_defaults(
                panel_json,
                DEFAULT_PANEL_CONFIG,
                okay_template_file("panel.json"),
                normalizer=normalize_panel_config,
            )
        except Exception:
            pass

    # 2. Tapeta pulpitu (~/.config/okay-de/wallpaper.png)
    user_wp = os.path.join(okay_dir, "wallpaper.png")
    if not os.path.exists(user_wp):
        wp_tmpl = wallpaper_template_file()
        if wp_tmpl:
            try:
                atomic_copy_file(wp_tmpl, user_wp, mode=0o644)
            except Exception:
                pass

    # 3. Wersja lokalna (~/.config/okay-de/version)
    user_ver = os.path.join(okay_dir, "version")
    if not os.path.exists(user_ver):
        ver_tmpl = find_existing_file(
            "/etc/okay-de/version",
            "/usr/share/okay-de/version",
            os.path.join(okay_data_dir(), "defaults", "version"),
        )
        if ver_tmpl:
            try:
                atomic_copy_file(ver_tmpl, user_ver, mode=0o644)
            except Exception:
                pass

    # 4. Skrypt autostartu (~/.config/okay-de/autostart)
    user_autostart = os.path.join(okay_dir, "autostart")
    if not os.path.exists(user_autostart):
        src_as = okay_template_file("autostart")
        if src_as:
            try:
                atomic_copy_file(src_as, user_autostart, mode=0o755)
            except Exception:
                pass

    # 7. Konfiguracja mako (~/.config/mako/config)
    created_mako = False
    mako_dir = os.path.join(xdg_config_home(), "mako")
    mako_cfg = os.path.join(mako_dir, "config")
    if not os.path.exists(mako_cfg):
        os.makedirs(mako_dir, exist_ok=True)
        m_tmpl = mako_template_file()
        if m_tmpl and os.path.isfile(m_tmpl):
            try:
                atomic_copy_file(m_tmpl, mako_cfg, mode=0o644)
                created_mako = True
            except Exception:
                pass
        else:
            try:
                atomic_write_text(mako_cfg, _DEFAULT_MAKO_CONFIG, mode=0o644)
                created_mako = True
            except Exception:
                pass

    # 8b. Konfiguracja Wayfire (~/.config/wayfire.ini) oraz migracja starych kluczy/kolorów
    created_wf = False
    wf_ini = wayfire_config_file()
    if not os.path.exists(wf_ini):
        w_tmpl = wayfire_template_file()
        if w_tmpl and os.path.isfile(w_tmpl):
            try:
                atomic_copy_file(w_tmpl, wf_ini, mode=0o644)
                created_wf = True
            except Exception:
                pass
    if os.path.isfile(wf_ini):
        try:
            ensure_wayfire_decorator_plugin()
            dec_active = read_wayfire_value("decoration", "active_color", "")
            pix_active = read_wayfire_value("pixdecor", "active_color", "")
            pix_layout = read_wayfire_value("pixdecor", "button_layout", "")
            pix_fg = read_wayfire_value("pixdecor", "fg_color", "")
            anim_min = read_wayfire_value("animate", "minimize_animation", "")
            anim_open = read_wayfire_value("animate", "open_animation", "")

            need_decor_sync = (
                dec_active.startswith("#")
                or pix_fg.startswith("#")
                or bool(pix_active)
                or not pix_layout
            )
            if not need_decor_sync:
                for gtk_ver in ("gtk-3.0", "gtk-4.0"):
                    ini_p = os.path.join(xdg_config_home(), gtk_ver, "settings.ini")
                    css_p = os.path.join(xdg_config_home(), gtk_ver, "gtk.css")
                    if not os.path.isfile(ini_p) or not os.path.isfile(css_p):
                        need_decor_sync = True
                        break
                    try:
                        with open(ini_p, "r", encoding="utf-8") as f:
                            ini_txt = f.read()
                        with open(css_p, "r", encoding="utf-8") as f:
                            css_txt = f.read()
                        if "gtk-decoration-layout" not in ini_txt or "gtk-theme-name" not in ini_txt or "windowcontrols" not in css_txt:
                            need_decor_sync = True
                            break
                    except Exception:
                        need_decor_sync = True
                        break
            if not need_decor_sync:
                try:
                    with open(wf_ini, "r", encoding="utf-8") as f:
                        wf_txt = f.read()
                    if wf_txt.count("[decoration]") > 1 or wf_txt.count("[pixdecor]") > 1:
                        need_decor_sync = True
                except Exception:
                    pass
            if need_decor_sync:
                apply_window_decorations(_from_ensure=True)
            if anim_min == "fade" or (anim_open == "fade" and anim_min != "none"):
                panel_anim = True
                if os.path.isfile(panel_json):
                    try:
                        with open(panel_json, "r", encoding="utf-8") as f:
                            panel_anim = bool(json.load(f).get("animations", True))
                    except Exception:
                        pass
                sync_gtk_animations(panel_anim)
        except Exception:
            pass

    if created_mako or created_wf:
        cur_accent = get_current_accent_color()
        if cur_accent.lower() != DEFAULT_ACCENT_COLOR:
            try:
                update_system_accent_color(cur_accent)
            except Exception:
                pass


class UpdateManager:
    """Zarządza sprawdzaniem i bezrootową instalacją aktualizacji OKay DE w przestrzeni użytkownika."""

    GITHUB_REPO = "h3v3n/okay-de"
    GIT_CLONE_URL = "https://github.com/h3v3n/okay-de.git"
    API_COMMITS_URL = "https://api.github.com/repos/h3v3n/okay-de/commits/main"
    TARBALL_URL = "https://github.com/h3v3n/okay-de/archive/refs/heads/main.tar.gz"
    SYSTEM_VERSION_FILES = ("/etc/okay-de/version", "/usr/share/okay-de/version")

    @classmethod
    def user_repo_dir(cls) -> str:
        return os.path.join(okay_data_dir(), "repo")

    @classmethod
    def local_version_file(cls) -> str:
        return os.path.join(okay_config_dir(), "version")

    @classmethod
    def get_local_commit(cls) -> str:
        candidates = (cls.local_version_file(),) + cls.SYSTEM_VERSION_FILES
        for v_file in candidates:
            if os.path.exists(v_file):
                try:
                    with open(v_file, "r", encoding="utf-8") as f:
                        val = f.read().strip()
                    if val:
                        return val[:7]
                except Exception:
                    pass
        repo_dir = cls.user_repo_dir()
        if os.path.isdir(os.path.join(repo_dir, ".git")) and shutil.which("git"):
            rc, out, _ = run_cmd(["git", "-C", repo_dir, "rev-parse", "--short", "HEAD"], timeout=5)
            if rc == 0 and out.strip():
                return out.strip()[:7]
        return "nieznana"

    @classmethod
    def check_remote_commit(cls, local_source_dir: str = "") -> dict:
        if local_source_dir:
            src = os.path.abspath(local_source_dir)
            if not os.path.isfile(os.path.join(src, "install.sh")):
                raise RuntimeError(f"Brak skryptu install.sh w katalogu źródłowym: {src}")
            if os.path.isdir(os.path.join(src, ".git")) and shutil.which("git"):
                rc, out, _ = run_cmd(["git", "-C", src, "log", "-1", "--format=%h%n%s%n%cs"], timeout=6)
                if rc == 0:
                    lines = out.splitlines()
                    return {
                        "sha": (lines[0] if len(lines) > 0 else "local")[:7],
                        "message": lines[1] if len(lines) > 1 else "Lokalne źródło",
                        "date": lines[2] if len(lines) > 2 else "",
                        "source": src,
                    }
            return {"sha": "local", "message": f"Lokalny katalog ({src})", "date": "", "source": src}

        req = urllib.request.Request(
            cls.API_COMMITS_URL,
            headers={
                "User-Agent": "OKay-Settings-Updater/2.0",
                "Accept": "application/vnd.github.v3+json",
            },
        )
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read().decode("utf-8"))
        sha = data.get("sha", "")[:7]
        commit = data.get("commit", {})
        msg = (commit.get("message", "") or "").splitlines()[0]
        date = commit.get("committer", {}).get("date", "")[:10]
        return {"sha": sha, "message": msg, "date": date, "source": cls.GIT_CLONE_URL}

    @staticmethod
    def _safe_extract_tar(tar_path: str, dest_dir: str) -> None:
        real_dest = os.path.realpath(dest_dir)
        with tarfile.open(tar_path, "r:gz") as tf:
            if sys.version_info >= (3, 12):
                tf.extractall(dest_dir, filter="data")
                return
            for member in tf.getmembers():
                target = os.path.realpath(os.path.join(real_dest, member.name))
                if target != real_dest and not target.startswith(real_dest + os.sep):
                    raise RuntimeError(f"Niebezpieczna ścieżka w archiwum: {member.name}")
                if member.issym() or member.islnk():
                    raise RuntimeError(f"Niedozwolone dowiązanie w archiwum: {member.name}")
                tf.extract(member, dest_dir)

    @staticmethod
    def _rmtree_force(path: str) -> None:
        if not os.path.exists(path):
            return
        def _onerror(func, p, _exc_info):
            try:
                parent = os.path.dirname(p)
                if os.path.isdir(parent):
                    os.chmod(parent, os.stat(parent).st_mode | stat.S_IWUSR | stat.S_IXUSR)
                if os.path.exists(p):
                    os.chmod(p, os.stat(p).st_mode | stat.S_IWUSR | (stat.S_IXUSR if os.path.isdir(p) else 0))
                func(p)
            except OSError:
                pass
        shutil.rmtree(path, onerror=_onerror)

    @staticmethod
    def _make_tree_writable(root_dir: str) -> None:
        for cur, dirs, files in os.walk(root_dir):
            try:
                os.chmod(cur, os.stat(cur).st_mode | stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR)
            except OSError:
                pass
            for fn in files:
                fp = os.path.join(cur, fn)
                try:
                    os.chmod(fp, os.stat(fp).st_mode | stat.S_IRUSR | stat.S_IWUSR)
                except OSError:
                    pass

    @classmethod
    def _atomic_swap_dir(cls, staging_dir: str, target_dir: str) -> None:
        parent = os.path.dirname(os.path.abspath(target_dir))
        os.makedirs(parent, exist_ok=True)
        backup_dir = target_dir + ".bak"
        cls._rmtree_force(backup_dir)
        had_existing = os.path.exists(target_dir)
        if had_existing:
            os.replace(target_dir, backup_dir)
        try:
            os.replace(staging_dir, target_dir)
        except Exception:
            if had_existing and os.path.exists(backup_dir) and not os.path.exists(target_dir):
                try:
                    os.replace(backup_dir, target_dir)
                except OSError:
                    pass
            raise
        cls._rmtree_force(backup_dir)

    @classmethod
    def _git_head_sha(cls, repo_dir: str, fallback: str = "") -> str:
        if os.path.isdir(os.path.join(repo_dir, ".git")) and shutil.which("git"):
            rc, out, _ = run_cmd(["git", "-C", repo_dir, "rev-parse", "--short", "HEAD"], timeout=5)
            if rc == 0 and out.strip():
                return out.strip()[:7]
        return (fallback or "")[:7] or "local"

    @classmethod
    def _prepare_user_repo(cls, expected_sha: str = "", local_source_dir: str = "") -> tuple:
        data_dir = okay_data_dir()
        os.makedirs(data_dir, exist_ok=True)
        repo_dir = cls.user_repo_dir()

        if local_source_dir:
            src = os.path.abspath(local_source_dir)
            if not os.path.isfile(os.path.join(src, "install.sh")):
                raise RuntimeError(f"Brak skryptu install.sh w lokalnym źródle: {src}")
            if src != os.path.abspath(repo_dir):
                stage_parent = tempfile.mkdtemp(prefix=".repo-stage-", dir=data_dir)
                try:
                    staged = os.path.join(stage_parent, "repo")
                    shutil.copytree(
                        src,
                        staged,
                        symlinks=False,
                        ignore_dangling_symlinks=True,
                        ignore=shutil.ignore_patterns(".tmp-*", "*.tmp"),
                    )
                    cls._make_tree_writable(staged)
                    cls._atomic_swap_dir(staged, repo_dir)
                finally:
                    cls._rmtree_force(stage_parent)
            sha = cls._git_head_sha(repo_dir, fallback=expected_sha or "local")
            return repo_dir, sha

        # 1. Preferowana ścieżka git w ~/.local/share/okay-de/repo
        if shutil.which("git"):
            if os.path.isdir(os.path.join(repo_dir, ".git")):
                rc_f, _o, _e = run_cmd(["git", "-C", repo_dir, "fetch", "--depth=1", "origin", "main"], timeout=35)
                if rc_f == 0:
                    rc_r, _o2, _e2 = run_cmd(["git", "-C", repo_dir, "reset", "--hard", "origin/main"], timeout=15)
                    if rc_r == 0 and os.path.isfile(os.path.join(repo_dir, "install.sh")):
                        return repo_dir, cls._git_head_sha(repo_dir, fallback=expected_sha)

            stage_parent = tempfile.mkdtemp(prefix=".repo-git-", dir=data_dir)
            try:
                staged_git = os.path.join(stage_parent, "repo")
                rc_c, _o, _e = run_cmd(
                    ["git", "clone", "--depth=1", "--branch", "main", cls.GIT_CLONE_URL, staged_git],
                    timeout=45,
                )
                if rc_c == 0 and os.path.isfile(os.path.join(staged_git, "install.sh")):
                    cls._atomic_swap_dir(staged_git, repo_dir)
                    return repo_dir, cls._git_head_sha(repo_dir, fallback=expected_sha)
            finally:
                cls._rmtree_force(stage_parent)

        # 2. Fallback: pobranie archiwum tar.gz z GitHub bezpośrednio do przestrzeni użytkownika
        stage_parent = tempfile.mkdtemp(prefix=".repo-tar-", dir=data_dir)
        try:
            tar_path = os.path.join(stage_parent, "main.tar.gz")
            req = urllib.request.Request(cls.TARBALL_URL, headers={"User-Agent": "OKay-Settings-Updater/2.0"})
            with urllib.request.urlopen(req, timeout=35) as resp, open(tar_path, "wb") as out:
                shutil.copyfileobj(resp, out)

            extract_dir = os.path.join(stage_parent, "extracted")
            os.makedirs(extract_dir, exist_ok=True)
            cls._safe_extract_tar(tar_path, extract_dir)

            src_dir = None
            for entry in os.listdir(extract_dir):
                full = os.path.join(extract_dir, entry)
                if os.path.isdir(full) and os.path.isfile(os.path.join(full, "install.sh")):
                    src_dir = full
                    break
            if not src_dir:
                raise RuntimeError("Nie znaleziono skryptu install.sh w paczce aktualizacji.")

            cls._make_tree_writable(src_dir)
            cls._atomic_swap_dir(src_dir, repo_dir)
        finally:
            cls._rmtree_force(stage_parent)

        return repo_dir, (expected_sha[:7] if expected_sha else "main")

    @classmethod
    def _reload_user_session(cls) -> list:
        notes = []
        if os.environ.get("WAYLAND_DISPLAY"):
            notes.append("Wayfire automatycznie przeładowuje ~/.config/wayfire.ini.")
            for app_name in ("okay-panel", "okay-desktop"):
                app_bin = os.path.join(xdg_bin_home(), app_name)
                if not (os.path.isfile(app_bin) and os.access(app_bin, os.X_OK)):
                    app_bin = shutil.which(app_name) or ""
                if app_bin:
                    try:
                        subprocess.Popen(
                            [app_bin, "--restart"],
                            stdin=subprocess.DEVNULL,
                            stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL,
                            start_new_session=True,
                        )
                        notes.append(f"Zrestartowano {app_name}.")
                    except Exception as exc:
                        notes.append(f"Nie udało się uruchomić {app_name}: {exc}")
        return notes

    @classmethod
    def download_and_install(cls, expected_sha: str = "", local_source_dir: str = "") -> str:
        repo_dir, actual_sha = cls._prepare_user_repo(expected_sha=expected_sha, local_source_dir=local_source_dir)

        env = os.environ.copy()
        env["OKAY_DE_FROM_UPDATER"] = "1"
        env["OKAY_UPDATER_PID"] = str(os.getpid())
        env["OKAY_DE_SKIP_RELOAD"] = "1"
        if actual_sha:
            env["OKAY_DE_VERSION"] = actual_sha

        proc = subprocess.run(
            ["bash", "./install.sh", "--user"],
            cwd=repo_dir,
            env=env,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=90,
            check=False,
        )
        if proc.returncode != 0:
            raise RuntimeError(proc.stderr or proc.stdout or "Błąd instalatora install.sh --user")

        # Zachowaj plik użytkownika panel.json i dopisz tylko ewentualne nowe klucze domyślne
        user_panel = os.path.join(okay_config_dir(), "panel.json")
        repo_panel = os.path.join(repo_dir, "config", "okay-de", "panel.json")
        try:
            merge_missing_json_defaults(
                user_panel,
                DEFAULT_PANEL_CONFIG,
                repo_panel,
                normalizer=normalize_panel_config,
            )
        except Exception:
            pass
        ensure_user_configs()

        if actual_sha:
            atomic_write_text(cls.local_version_file(), actual_sha + "\n", mode=0o644)

        reload_notes = cls._reload_user_session()
        out = proc.stdout.strip()
        if reload_notes:
            out = (out + "\n" if out else "") + "\n".join(reload_notes)
        return out


def _ensure_window_button_svgs(buttons_style: str, accent_hex: str) -> dict:
    """Generuje ikony SVG przycisków okna dla wtyczki Wayfire pixdecor."""
    btn_dir = os.path.join(okay_data_dir(), "window-buttons", buttons_style)
    try:
        os.makedirs(btn_dir, exist_ok=True)
    except Exception:
        return {}

    contrast = compute_contrast_text(accent_hex)
    if buttons_style in ("circles_left", "circles_right"):
        specs = {
            "close.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><circle cx="9" cy="9" r="6" fill="#ff5f57"/></svg>',
            "close-hover.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><circle cx="9" cy="9" r="6.5" fill="#ff5f57"/><path d="M6.4 6.4 L11.6 11.6 M11.6 6.4 L6.4 11.6" stroke="#1e1e2e" stroke-width="1.5" stroke-linecap="round"/></svg>',
            "minimize.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><circle cx="9" cy="9" r="6" fill="#febc2e"/></svg>',
            "minimize-hover.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><circle cx="9" cy="9" r="6.5" fill="#febc2e"/><path d="M5.8 9 L12.2 9" stroke="#1e1e2e" stroke-width="1.6" stroke-linecap="round"/></svg>',
            "maximize.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><circle cx="9" cy="9" r="6" fill="#28c840"/></svg>',
            "maximize-hover.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><circle cx="9" cy="9" r="6.5" fill="#28c840"/><rect x="6.2" y="6.2" width="5.6" height="5.6" rx="0.8" fill="none" stroke="#1e1e2e" stroke-width="1.4"/></svg>',
            "restore.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><circle cx="9" cy="9" r="6" fill="#28c840"/></svg>',
            "restore-hover.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><circle cx="9" cy="9" r="6.5" fill="#28c840"/><path d="M6.8 6.8 H11.5 V11.5 M6 7.8 H10.2 V12 H6 Z" fill="none" stroke="#1e1e2e" stroke-width="1.2"/></svg>',
        }
    elif buttons_style == "windows":
        specs = {
            "close.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><path d="M5.5 5.5 L12.5 12.5 M12.5 5.5 L5.5 12.5" stroke="{contrast}" stroke-width="1.4" stroke-linecap="round"/></svg>',
            "close-hover.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><rect x="1" y="1" width="16" height="16" rx="3" fill="#e81123"/><path d="M5.5 5.5 L12.5 12.5 M12.5 5.5 L5.5 12.5" stroke="#ffffff" stroke-width="1.45" stroke-linecap="round"/></svg>',
            "minimize.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><path d="M5 10.5 H13" stroke="{contrast}" stroke-width="1.4" stroke-linecap="round"/></svg>',
            "minimize-hover.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><rect x="1" y="1" width="16" height="16" rx="3" fill="#313244"/><path d="M5 10.5 H13" stroke="#cdd6f4" stroke-width="1.45" stroke-linecap="round"/></svg>',
            "maximize.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><rect x="5" y="5" width="8" height="8" rx="0.8" fill="none" stroke="{contrast}" stroke-width="1.35"/></svg>',
            "maximize-hover.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><rect x="1" y="1" width="16" height="16" rx="3" fill="#313244"/><rect x="5" y="5" width="8" height="8" rx="0.8" fill="none" stroke="#cdd6f4" stroke-width="1.4"/></svg>',
            "restore.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><path d="M7 5 H13 V11 M5 7 H11 V13 H5 Z" fill="none" stroke="{contrast}" stroke-width="1.25"/></svg>',
            "restore-hover.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><rect x="1" y="1" width="16" height="16" rx="3" fill="#313244"/><path d="M7 5 H13 V11 M5 7 H11 V13 H5 Z" fill="none" stroke="#cdd6f4" stroke-width="1.3"/></svg>',
        }
    else:
        # Styl "rectangles" (domyślne zaokrąglone prostokąty z akcentem)
        specs = {
            "close.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><rect x="1.5" y="1.5" width="15" height="15" rx="3.5" fill="#313244" stroke="#45475a" stroke-width="1"/><path d="M6.2 6.2 L11.8 11.8 M11.8 6.2 L6.2 11.8" stroke="#f38ba8" stroke-width="1.5" stroke-linecap="round"/></svg>',
            "close-hover.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><rect x="1.5" y="1.5" width="15" height="15" rx="3.5" fill="#f38ba8"/><path d="M6.2 6.2 L11.8 11.8 M11.8 6.2 L6.2 11.8" stroke="#11111b" stroke-width="1.6" stroke-linecap="round"/></svg>',
            "minimize.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><rect x="1.5" y="1.5" width="15" height="15" rx="3.5" fill="#313244" stroke="#45475a" stroke-width="1"/><path d="M5.8 9.5 H12.2" stroke="#cdd6f4" stroke-width="1.5" stroke-linecap="round"/></svg>',
            "minimize-hover.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><rect x="1.5" y="1.5" width="15" height="15" rx="3.5" fill="{accent_hex}"/><path d="M5.8 9.5 H12.2" stroke="{contrast}" stroke-width="1.6" stroke-linecap="round"/></svg>',
            "maximize.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><rect x="1.5" y="1.5" width="15" height="15" rx="3.5" fill="#313244" stroke="#45475a" stroke-width="1"/><rect x="5.8" y="5.8" width="6.4" height="6.4" rx="1" fill="none" stroke="#cdd6f4" stroke-width="1.4"/></svg>',
            "maximize-hover.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><rect x="1.5" y="1.5" width="15" height="15" rx="3.5" fill="{accent_hex}"/><rect x="5.8" y="5.8" width="6.4" height="6.4" rx="1" fill="none" stroke="{contrast}" stroke-width="1.5"/></svg>',
            "restore.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><rect x="1.5" y="1.5" width="15" height="15" rx="3.5" fill="#313244" stroke="#45475a" stroke-width="1"/><path d="M7.2 5.4 H12.4 V10.6 M5.6 7 H10.8 V12.2 H5.6 Z" fill="none" stroke="#cdd6f4" stroke-width="1.2"/></svg>',
            "restore-hover.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><rect x="1.5" y="1.5" width="15" height="15" rx="3.5" fill="{accent_hex}"/><path d="M7.2 5.4 H12.4 V10.6 M5.6 7 H10.8 V12.2 H5.6 Z" fill="none" stroke="{contrast}" stroke-width="1.3"/></svg>',
        }

    key_map = {
        "close.svg": "button_close_image",
        "close-hover.svg": "button_close_hover_image",
        "minimize.svg": "button_minimize_image",
        "minimize-hover.svg": "button_minimize_hover_image",
        "maximize.svg": "button_maximize_image",
        "maximize-hover.svg": "button_maximize_hover_image",
        "restore.svg": "button_restore_image",
        "restore-hover.svg": "button_restore_hover_image",
    }
    out = {}
    for fname, svg_data in specs.items():
        fpath = os.path.join(btn_dir, fname)
        try:
            existing = ""
            if os.path.isfile(fpath):
                with open(fpath, "r", encoding="utf-8") as f:
                    existing = f.read()
            if existing != svg_data:
                atomic_write_text(fpath, svg_data, mode=0o644)
        except Exception:
            pass
        if fname in key_map:
            out[key_map[fname]] = fpath
    return out


_GTK_COMMON_DEFAULTS = {
    "gtk-theme-name": "Adwaita-dark",
    "gtk-icon-theme-name": "Papirus-Dark",
    "gtk-font-name": "JetBrains Mono 10",
    "gtk-cursor-theme-name": "Adwaita",
    "gtk-cursor-theme-size": "24",
    "gtk-application-prefer-dark-theme": "1",
    "gtk-enable-animations": "1",
    "gtk-decoration-layout": ":minimize,maximize,close",
}

_GTK3_EXTRA_DEFAULTS = {
    "gtk-toolbar-style": "GTK_TOOLBAR_BOTH_HORIZ",
    "gtk-toolbar-icon-size": "GTK_ICON_SIZE_LARGE_TOOLBAR",
    "gtk-button-images": "1",
    "gtk-menu-images": "1",
    "gtk-enable-event-sounds": "1",
    "gtk-enable-input-feedback-sounds": "0",
    "gtk-xft-antialias": "1",
    "gtk-xft-hinting": "1",
    "gtk-xft-hintstyle": "hintslight",
    "gtk-xft-rgba": "rgb",
}


def _get_button_layouts(buttons_style: str) -> tuple:
    """Zwraca (wayfire_decoration_order, pixdecor_and_gtk_layout) dla wybranego stylu przycisków."""
    if str(buttons_style).lower() == "circles_left":
        return "close minimize maximize", "close,maximize,minimize:"
    return "minimize maximize close", ":minimize,maximize,close"


def sync_gtk_settings(decoration_layout: str = None, animations: bool = None, theme_mode: str = None) -> None:
    """Aktualizuje ~/.config/gtk-3.0/settings.ini oraz gtk-4.0/settings.ini, zachowując spójność układu przycisków i motywu."""
    updates = {}
    if decoration_layout is not None:
        updates["gtk-decoration-layout"] = decoration_layout
    if animations is not None:
        updates["gtk-enable-animations"] = "1" if animations else "0"
    if theme_mode is not None:
        is_light = (str(theme_mode).lower() == "light")
        updates["gtk-application-prefer-dark-theme"] = "0" if is_light else "1"
        updates["gtk-theme-name"] = "Adwaita" if is_light else "Adwaita-dark"
        updates["gtk-icon-theme-name"] = "Papirus" if is_light else "Papirus-Dark"
        if shutil.which("gsettings"):
            color_scheme = "prefer-light" if is_light else "prefer-dark"
            gtk_theme = "Adwaita" if is_light else "Adwaita-dark"
            run_cmd(["gsettings", "set", "org.gnome.desktop.interface", "color-scheme", color_scheme], timeout=2)
            run_cmd(["gsettings", "set", "org.gnome.desktop.interface", "gtk-theme", gtk_theme], timeout=2)

    for gtk_ver in ("gtk-3.0", "gtk-4.0"):
        ini_path = os.path.join(xdg_config_home(), gtk_ver, "settings.ini")
        defaults = dict(_GTK_COMMON_DEFAULTS)
        if gtk_ver == "gtk-3.0":
            defaults.update(_GTK3_EXTRA_DEFAULTS)
        try:
            raw = ""
            if os.path.isfile(ini_path):
                with open(ini_path, "r", encoding="utf-8") as f:
                    raw = f.read()

            existing = {}
            other_lines = []
            in_settings = False
            for line in raw.splitlines():
                s = line.strip()
                if s.startswith("[") and s.endswith("]"):
                    in_settings = (s == "[Settings]")
                    if not in_settings:
                        other_lines.append(line)
                    continue
                if in_settings and "=" in s and not s.startswith("#") and not s.startswith(";"):
                    k, v = s.split("=", 1)
                    existing[k.strip()] = v.strip()
                elif not in_settings and s:
                    other_lines.append(line)

            merged = dict(defaults)
            merged.update(existing)
            merged.update(updates)

            out_lines = ["[Settings]"]
            for k, v in merged.items():
                out_lines.append(f"{k} = {v}")
            if other_lines:
                out_lines.append("")
                out_lines.extend(other_lines)
            atomic_write_text(ini_path, "\n".join(out_lines).rstrip() + "\n", mode=0o644)
        except Exception:
            pass


def sync_gnome_wm_button_layout(gtk_layout: str) -> None:
    """Synchronizuje układ przycisków w org.gnome.desktop.wm.preferences (dla GTK4, przeglądarek, Flatpak i portalu)."""
    if not shutil.which("gsettings"):
        return
    run_cmd(["gsettings", "set", "org.gnome.desktop.wm.preferences", "button-layout", gtk_layout], timeout=2)


def sync_flatpak_environment(theme_mode: str = "dark") -> None:
    """Synchronizuje w tle uprawnienia i zmienne sandboxa Flatpaka z motywem oraz arkuszami CSS OKay DE."""
    if not shutil.which("flatpak"):
        return
    gtk_theme = "Adwaita" if str(theme_mode).lower() == "light" else "Adwaita-dark"

    def _worker():
        run_cmd([
            "flatpak", "override", "--user",
            "--filesystem=xdg-config/gtk-3.0:ro",
            "--filesystem=xdg-config/gtk-4.0:ro",
            "--filesystem=xdg-config/okay-de:ro",
            "--env=GTK_CSD=0",
            f"--env=GTK_THEME={gtk_theme}",
            "--env=XCURSOR_THEME=Adwaita",
            "--env=XCURSOR_SIZE=24",
        ], timeout=6)

    threading.Thread(target=_worker, daemon=True).start()


def apply_window_decorations(
    buttons_style: str = None,
    corner_radius: int = None,
    accent_hex: str = None,
    theme_mode: str = None,
    _from_ensure: bool = False,
) -> dict:
    """
    Zapisuje styl przycisków, zaokrąglenie ramek okien, motyw (dark/light) i kolor akcentu
    w Wayfire ([decoration], [pixdecor]), GTK3/GTK4/libadwaita CSS, settings.ini, GSettings oraz Flatpaku.
    """
    if not _from_ensure:
        ensure_user_configs()
    panel_file = os.path.join(okay_config_dir(), "panel.json")
    cfg = {}
    if os.path.isfile(panel_file):
        try:
            with open(panel_file, "r", encoding="utf-8") as f:
                cfg = json.load(f)
        except Exception:
            cfg = {}
    if not isinstance(cfg, dict):
        cfg = copy.deepcopy(DEFAULT_PANEL_CONFIG)

    if buttons_style is None:
        buttons_style = cfg.get("window_buttons_style", "rectangles")
    buttons_style = str(buttons_style).lower()
    if buttons_style not in ("rectangles", "circles_left", "circles_right", "windows"):
        buttons_style = "rectangles"

    if corner_radius is None:
        corner_radius = cfg.get("window_corner_radius", cfg.get("window_rounding", 12))
    try:
        corner_radius = max(0, min(32, int(corner_radius)))
    except Exception:
        corner_radius = 12

    if accent_hex is None:
        accent_hex = cfg.get("accent_color", DEFAULT_ACCENT_COLOR)
    accent_hex = sanitize_hex_color(accent_hex)

    if theme_mode is None:
        theme_mode = cfg.get("theme_mode", "dark")
    theme_mode = "light" if str(theme_mode).lower() == "light" else "dark"

    cfg["window_buttons_style"] = buttons_style
    cfg["window_corner_radius"] = corner_radius
    cfg["window_rounding"] = corner_radius
    cfg["accent_color"] = accent_hex
    cfg["theme_mode"] = theme_mode
    atomic_write_json(panel_file, normalize_panel_config(cfg))

    wf_layout, pix_layout = _get_button_layouts(buttons_style)
    contrast_text = compute_contrast_text(accent_hex)
    svg_opts = _ensure_window_button_svgs(buttons_style, accent_hex)

    # Synchronizacja układu przycisków i motywu w GTK3/GTK4 settings.ini, GSettings oraz Flatpaku
    sync_gtk_settings(decoration_layout=pix_layout, theme_mode=theme_mode)
    sync_gnome_wm_button_layout(pix_layout)
    sync_flatpak_environment(theme_mode=theme_mode)

    is_light = (theme_mode == "light")
    win_bg = "#f4f5f8" if is_light else "#181825"
    win_fg = "#181825" if is_light else "#f2f2f7"
    hdr_bg = blend_hex_color("#ebedf2" if is_light else "#1e1e2e", accent_hex, 0.08)
    hdr_backdrop = "#e2e4ec" if is_light else "#14141d"
    card_bg = "#ffffff" if is_light else "#1e1e2e"
    btn_bg = "#dce0ec" if is_light else "#313244"
    btn_border = "#bcc2d6" if is_light else "#45475a"

    # Zmienne kolorystyczne libadwaita / GTK4 + GTK3 dla pełnej spójności aplikacji CSD i Flatpak
    adw_color_defs = (
        f"@define-color accent_color {accent_hex};\n"
        f"@define-color accent_bg_color {accent_hex};\n"
        f"@define-color accent_fg_color {contrast_text};\n"
        f"@define-color window_bg_color {win_bg};\n"
        f"@define-color window_fg_color {win_fg};\n"
        f"@define-color view_bg_color {card_bg};\n"
        f"@define-color view_fg_color {win_fg};\n"
        f"@define-color headerbar_bg_color {hdr_bg};\n"
        f"@define-color headerbar_fg_color {win_fg};\n"
        f"@define-color headerbar_border_color {accent_hex};\n"
        f"@define-color headerbar_backdrop_color {hdr_backdrop};\n"
        f"@define-color card_bg_color {card_bg};\n"
        f"@define-color card_fg_color {win_fg};\n"
        f"@define-color popover_bg_color {card_bg};\n"
        f"@define-color popover_fg_color {win_fg};\n"
        f"@define-color dialog_bg_color {win_bg};\n"
        f"@define-color dialog_fg_color {win_fg};\n"
    )

    # Stylizacja przycisków okna GTK3/GTK4/libadwaita CSD (kompatybilna z rectangles / circles_* / windows)
    btn_sel = "headerbar button.titlebutton, .titlebar button.titlebutton, headerbar windowcontrols > button, windowcontrols > button"
    img_sel = "headerbar button.titlebutton image, .titlebar button.titlebutton image, headerbar windowcontrols > button image, windowcontrols > button image"
    if buttons_style in ("circles_left", "circles_right"):
        gtk_btn_css = (
            f"{btn_sel} {{\n"
            f"    border-radius: 999px;\n"
            f"    min-width: 14px; min-height: 14px;\n"
            f"    padding: 2px; margin: 0 3px;\n"
            f"    background-image: none;\n"
            f"    border: none;\n"
            f"    box-shadow: none;\n"
            f"}}\n"
            f"{img_sel} {{ opacity: 0; }}\n"
            f"headerbar button.titlebutton:hover image, .titlebar button.titlebutton:hover image,\n"
            f"headerbar windowcontrols > button:hover image, windowcontrols > button:hover image {{ opacity: 1; }}\n"
            f"headerbar button.titlebutton.close, .titlebar button.titlebutton.close,\n"
            f"headerbar windowcontrols > button.close, windowcontrols > button.close {{ background-color: #ff5f57; color: #1e1e2e; }}\n"
            f"headerbar button.titlebutton.minimize, .titlebar button.titlebutton.minimize,\n"
            f"headerbar windowcontrols > button.minimize, windowcontrols > button.minimize {{ background-color: #febc2e; color: #1e1e2e; }}\n"
            f"headerbar button.titlebutton.maximize, .titlebar button.titlebutton.maximize,\n"
            f"headerbar windowcontrols > button.maximize, windowcontrols > button.maximize {{ background-color: #28c840; color: #1e1e2e; }}\n"
        )
    elif buttons_style == "windows":
        gtk_btn_css = (
            f"{btn_sel} {{\n"
            f"    border-radius: 4px;\n"
            f"    min-width: 28px; min-height: 24px;\n"
            f"    background-image: none;\n"
            f"    background-color: transparent;\n"
            f"    border: none;\n"
            f"    box-shadow: none;\n"
            f"    color: {win_fg};\n"
            f"}}\n"
            f"headerbar button.titlebutton.close:hover, .titlebar button.titlebutton.close:hover,\n"
            f"headerbar windowcontrols > button.close:hover, windowcontrols > button.close:hover {{ background-color: #e81123; color: #ffffff; }}\n"
            f"headerbar button.titlebutton:not(.close):hover, .titlebar button.titlebutton:not(.close):hover,\n"
            f"headerbar windowcontrols > button:not(.close):hover, windowcontrols > button:not(.close):hover {{ background-color: alpha({accent_hex}, 0.28); }}\n"
        )
    else:
        gtk_btn_css = (
            f"{btn_sel} {{\n"
            f"    border-radius: 5px;\n"
            f"    background-image: none;\n"
            f"    background-color: {btn_bg};\n"
            f"    border: 1px solid {btn_border};\n"
            f"    box-shadow: none;\n"
            f"    color: {win_fg};\n"
            f"    min-width: 22px; min-height: 20px;\n"
            f"    padding: 2px; margin: 0 2px;\n"
            f"}}\n"
            f"headerbar button.titlebutton.close, .titlebar button.titlebutton.close,\n"
            f"headerbar windowcontrols > button.close, windowcontrols > button.close {{ color: #f38ba8; }}\n"
            f"headerbar button.titlebutton.close:hover, .titlebar button.titlebutton.close:hover,\n"
            f"headerbar windowcontrols > button.close:hover, windowcontrols > button.close:hover {{ background-color: #f38ba8; border-color: #f38ba8; color: #11111b; }}\n"
            f"headerbar button.titlebutton:not(.close):hover, .titlebar button.titlebutton:not(.close):hover,\n"
            f"headerbar windowcontrols > button:not(.close):hover, windowcontrols > button:not(.close):hover {{ background-color: {accent_hex}; border-color: {accent_hex}; color: {contrast_text}; }}\n"
        )

    # Wymuszenie zaokrąglenia narożników, palety barw i koloru ramki w aplikacjach GTK3 / GTK4 / libadwaita
    marker_start = "/* OKAY-DE-CORNERS-START */"
    marker_end = "/* OKAY-DE-CORNERS-END */"
    gtk3_block = (
        f"{marker_start}\n"
        f"{adw_color_defs}"
        f"window, window.background, window.csd, window.ssd, window.solid-csd,\n"
        f"decoration, messagedialog, dialog, popover > contents, menu {{\n"
        f"    border-radius: {corner_radius}px;\n"
        f"}}\n"
        f"window.csd decoration {{\n"
        f"    border: 2px solid {accent_hex};\n"
        f"}}\n"
        f"window.csd decoration:backdrop {{\n"
        f"    border-color: {btn_border};\n"
        f"}}\n"
        f"headerbar, .titlebar {{\n"
        f"    background-color: {hdr_bg};\n"
        f"    background-image: none;\n"
        f"    color: {win_fg};\n"
        f"    border-top-left-radius: {corner_radius}px;\n"
        f"    border-top-right-radius: {corner_radius}px;\n"
        f"}}\n"
        f"headerbar:backdrop, .titlebar:backdrop {{\n"
        f"    background-color: {hdr_backdrop};\n"
        f"}}\n"
        f"headerbar:not(.backdrop), .titlebar:not(.backdrop) {{\n"
        f"    border-bottom: 2px solid {accent_hex};\n"
        f"}}\n"
        f"window.background > box, window.background > stack, window.background > paned {{\n"
        f"    border-bottom-left-radius: {corner_radius}px;\n"
        f"    border-bottom-right-radius: {corner_radius}px;\n"
        f"}}\n"
        f"{gtk_btn_css}"
        f"{marker_end}\n"
    )
    gtk4_block = (
        f"{marker_start}\n"
        f"{adw_color_defs}"
        f"window, window.background, window.csd, window.ssd, window.solid-csd,\n"
        f"decoration, messagedialog, dialog, popover > contents {{\n"
        f"    border-radius: {corner_radius}px;\n"
        f"    overflow: hidden;\n"
        f"}}\n"
        f"window.csd decoration {{\n"
        f"    border: 2px solid {accent_hex};\n"
        f"}}\n"
        f"window.csd decoration:backdrop {{\n"
        f"    border-color: {btn_border};\n"
        f"}}\n"
        f"headerbar, .titlebar {{\n"
        f"    background-color: {hdr_bg};\n"
        f"    background-image: none;\n"
        f"    color: {win_fg};\n"
        f"}}\n"
        f"headerbar:backdrop, .titlebar:backdrop {{\n"
        f"    background-color: {hdr_backdrop};\n"
        f"}}\n"
        f"headerbar:not(.backdrop), .titlebar:not(.backdrop) {{\n"
        f"    border-bottom: 2px solid {accent_hex};\n"
        f"}}\n"
        f"{gtk_btn_css}"
        f"{marker_end}\n"
    )
    for gtk_ver, gtk_block in (("gtk-3.0", gtk3_block), ("gtk-4.0", gtk4_block)):
        gtk_css = os.path.join(xdg_config_home(), gtk_ver, "gtk.css")
        try:
            existing_css = ""
            if os.path.isfile(gtk_css):
                with open(gtk_css, "r", encoding="utf-8") as f:
                    existing_css = f.read()
            if marker_start in existing_css and marker_end in existing_css:
                new_css = re.sub(
                    re.escape(marker_start) + r".*?" + re.escape(marker_end) + r"\n?",
                    gtk_block,
                    existing_css,
                    flags=re.DOTALL,
                )
            else:
                new_css = (existing_css.rstrip() + "\n\n" if existing_css.strip() else "") + gtk_block
            atomic_write_text(gtk_css, new_css, mode=0o644)
        except Exception:
            pass

    # Synchronizacja zaokrąglenia w mako (~/.config/mako/config)
    mako_cfg = os.path.join(xdg_config_home(), "mako", "config")
    if os.path.isfile(mako_cfg):
        try:
            with open(mako_cfg, "r", encoding="utf-8") as f:
                m_txt = f.read()
            if re.search(r"^border-radius\s*=.*$", m_txt, flags=re.MULTILINE):
                m_txt = re.sub(r"^(border-radius\s*=\s*).*$", rf"\g<1>{corner_radius}", m_txt, flags=re.MULTILINE)
            else:
                m_txt = m_txt.rstrip() + f"\nborder-radius={corner_radius}\n"
            atomic_write_text(mako_cfg, m_txt, mode=0o644)
        except Exception:
            pass

    # Aktualizacja Wayfire (~/.config/wayfire.ini: format float RGBA wymagany przez wf-config)
    ensure_wayfire_decorator_plugin()
    inactive_hex = "#d0d4e0" if is_light else "#313244"
    pix_bg_hex = "#ebedf2" if is_light else "#1e1e2e"
    update_wayfire_section("decoration", {
        "button_order": wf_layout,
        "active_color": hex_to_wf_rgba(accent_hex, 1.0),
        "inactive_color": hex_to_wf_rgba(inactive_hex, 1.0),
        "border_size": "2",
        "ignore_views": "none",
    })
    pix_updates = {
        "titlebar": "true",
        "csd_titlebar_height": "28",
        "border_size": "2",
        "rounded_corner_radius": str(corner_radius),
        "overlay_engine": "rounded_corners" if corner_radius > 0 else "none",
        "always_decorate": "all",
        "button_layout": pix_layout,
        "fg_color": hex_to_wf_rgba(accent_hex, 1.0),
        "bg_color": hex_to_wf_rgba(pix_bg_hex, 1.0),
        "fg_text_color": hex_to_wf_rgba(contrast_text, 1.0),
        "bg_text_color": "0.2500 0.2700 0.3500 1.0000" if is_light else "0.6510 0.6784 0.7843 1.0000",
        "effect_color": hex_to_wf_rgba(accent_hex, 1.0),
        "maximized_borders": "true",
        "maximized_shadows": "true",
    }
    pix_updates.update(svg_opts)
    update_wayfire_section(
        "pixdecor",
        pix_updates,
        remove_keys={"rounded_corners", "active_color", "inactive_color", "button_order"},
    )

    if shutil.which("makoctl"):
        run_cmd(["makoctl", "reload"], timeout=2)

    return {
        "status": "ok",
        "buttons_style": buttons_style,
        "corner_radius": corner_radius,
        "theme_mode": theme_mode,
        "layout": wf_layout,
        "pixdecor_active": is_pixdecor_installed(),
    }


def apply_wayfire_animations_and_effects(cfg: dict = None) -> dict:
    """
    Synchronizuje na żywo silnik animacji Wayfire ([animate], [vswitch], [expo], [grid], [switcher]),
    efekty kompozytora ([blur], [wobbly], [window-rules]) oraz liczbę obszarów roboczych i blokadę ekranu ([idle]).
    """
    if cfg is None:
        panel_file = os.path.join(okay_config_dir(), "panel.json")
        raw_cfg = {}
        if os.path.isfile(panel_file):
            try:
                with open(panel_file, "r", encoding="utf-8") as f:
                    raw_cfg = json.load(f)
            except Exception:
                raw_cfg = {}
        cfg = normalize_panel_config(raw_cfg)
    else:
        cfg = normalize_panel_config(cfg)

    reduce_motion = bool(cfg.get("reduce_motion", False))
    speed = max(0.5, min(2.0, float(cfg.get("animation_speed", 1.0))))
    open_eff = str(cfg.get("open_effect", "zoom"))
    min_eff = str(cfg.get("minimize_effect", "squeezimize"))
    blur_enabled = bool(cfg.get("blur_enabled", False))
    wobbly_enabled = bool(cfg.get("wobbly_enabled", False))
    win_opacity = max(0.50, min(1.00, float(cfg.get("window_opacity", 1.0))))
    ws_count = max(1, min(9, int(cfg.get("workspace_count", 3))))
    idle_timeout = max(0, min(7200, int(cfg.get("idle_timeout", 600))))
    accent_hex = sanitize_hex_color(cfg.get("accent_color", DEFAULT_ACCENT_COLOR))

    # Synchronizacja natywnego kontrolera animacji w samym procesie Pythona
    try:
        for p in [os.path.abspath(os.path.join(os.path.dirname(__file__), "..")), xdg_bin_home()]:
            if os.path.isdir(os.path.join(p, "panel")) and p not in sys.path:
                sys.path.insert(0, p)
        from panel.animations import configure_animations
        configure_animations(reduce_motion=reduce_motion, speed_multiplier=speed)
    except Exception:
        pass

    sync_gtk_settings(animations=(not reduce_motion))

    def _scaled_ms(base_ms: int, curve: str = "circle") -> str:
        if reduce_motion:
            return f"1ms {curve}"
        ms = max(30, int(round(float(base_ms) / speed)))
        return f"{ms}ms {curve}"

    if reduce_motion or open_eff == "none":
        update_wayfire_section("animate", {
            "open_animation": "none",
            "close_animation": "none",
            "minimize_animation": "none",
            "fade_enabled_for": "none",
        })
    else:
        update_wayfire_section("animate", {
            "open_animation": open_eff,
            "close_animation": open_eff,
            "minimize_animation": min_eff,
            "duration": _scaled_ms(180, "circle"),
            "startup_duration": _scaled_ms(260, "circle"),
            "fade_duration": _scaled_ms(150, "circle"),
            "zoom_duration": _scaled_ms(180, "circle"),
            "fire_duration": _scaled_ms(220, "linear"),
            "squeezimize_duration": _scaled_ms(160, "linear"),
            "fire_color": hex_to_wf_rgba(accent_hex, 1.0),
            "fade_enabled_for": '(type is "overlay" | (type is "x-or" & focusable is true))',
        })

    update_wayfire_section("vswitch", {"duration": _scaled_ms(180, "circle")})
    update_wayfire_section("expo", {"duration": _scaled_ms(200, "circle")})
    update_wayfire_section("grid", {"duration": _scaled_ms(160, "circle")})
    update_wayfire_section("switcher", {"speed": _scaled_ms(180, "circle")})

    # Aktualizacja [core]: vwidth, preferred_decoration_mode oraz dynamiczne wtyczki (blur, wobbly)
    plugins_str = read_wayfire_value(
        "core",
        "plugins",
        "alpha animate autostart blur command decoration expo fast-switcher foreign-toplevel grid gtk-shell idle move place resize switcher vswitch window-rules wm-actions xdg-activation",
    )
    tokens = [t for t in plugins_str.split() if t]
    if wobbly_enabled and not reduce_motion:
        if "wobbly" not in tokens:
            tokens.append("wobbly")
    else:
        tokens = [t for t in tokens if t != "wobbly"]

    if blur_enabled:
        if "blur" not in tokens:
            tokens.append("blur")
    update_wayfire_section("core", {
        "plugins": " ".join(tokens),
        "vwidth": str(ws_count),
        "preferred_decoration_mode": "server",
    })

    update_wayfire_section("blur", {
        "mode": "normal" if blur_enabled else "none",
        "method": "kawase",
        "kawase_offset": "2.0",
        "kawase_degrade": "2",
        "kawase_iterations": "2",
    })

    # Reguły przezroczystości okien ([window-rules])
    wr_updates = {
        "rule_panel_ignore": 'on created if app_id is "okay-panel" then set alpha 1.0',
    }
    if win_opacity < 0.995:
        wr_updates["rule_global_opacity"] = (
            f'on created if !(app_id is "okay-panel") then set alpha {win_opacity:.2f}'
        )
    update_wayfire_section(
        "window-rules",
        wr_updates,
        remove_keys={"rule_global_opacity"} if win_opacity >= 0.995 else None,
    )

    update_wayfire_section("idle", {
        "screensaver_timeout": str(idle_timeout) if idle_timeout > 0 else "-1",
        "dpms_timeout": str(idle_timeout + 300) if idle_timeout > 0 else "-1",
    })

    return {
        "reduce_motion": reduce_motion,
        "animation_speed": speed,
        "open_effect": open_eff,
        "minimize_effect": min_eff,
        "blur_enabled": blur_enabled,
        "wobbly_enabled": wobbly_enabled,
        "window_opacity": win_opacity,
    }


def apply_wayfire_outputs(outputs_config: list) -> bool:
    """
    Zapisuje konfigurację monitorów w sekcjach `[output:<name>]` w `~/.config/wayfire.ini`,
    zapewniając trwałość układu wielomonitorowego, rozdzielczości, odświeżania Hz oraz skalowania ułamkowego.
    """
    if not isinstance(outputs_config, list):
        return False
    ok_all = True
    for out in outputs_config:
        if not isinstance(out, dict):
            continue
        name = str(out.get("name", "")).strip()
        if not name:
            continue
        sec_name = f"output:{name}"
        enabled = bool(out.get("enabled", True))
        if not enabled:
            ok_all = update_wayfire_section(sec_name, {"mode": "off"}) and ok_all
            continue
        w = int(out.get("width", 1920))
        h = int(out.get("height", 1080))
        hz = float(out.get("refresh", 60.0))
        hz_int = int(round(hz * 1000)) if hz < 1000 else int(round(hz))
        scale = float(out.get("scale", 1.0))
        pos_x = int(out.get("x", 0))
        pos_y = int(out.get("y", 0))
        transform = str(out.get("transform", "normal")).strip() or "normal"
        ok_all = update_wayfire_section(sec_name, {
            "mode": f"{w}x{h}@{hz_int}",
            "scale": f"{scale:.2f}",
            "position": f"{pos_x},{pos_y}",
            "transform": transform,
        }) and ok_all
    return ok_all


def set_focus_mode(enabled: bool) -> bool:
    """Włącza lub wyłącza Tryb Skupienia (Do Not Disturb) w demonie powiadomień mako i zapisuje stan w panel.json."""
    focus = bool(enabled)
    if shutil.which("makoctl"):
        if focus:
            run_cmd(["makoctl", "mode", "-a", "do-not-disturb"], timeout=2)
        else:
            run_cmd(["makoctl", "mode", "-r", "do-not-disturb"], timeout=2)
    try:
        get_state_manager().update({"focus_mode": focus}, sync_wayfire=False)
    except Exception:
        pass
    return focus


def sync_gtk_animations(enabled: bool) -> None:
    """Synchronizuje animacje GTK (settings.ini) oraz kompozytora Wayfire (~/.config/wayfire.ini)."""
    panel_file = os.path.join(okay_config_dir(), "panel.json")
    cfg = {}
    if os.path.isfile(panel_file):
        try:
            with open(panel_file, "r", encoding="utf-8") as f:
                cfg = json.load(f)
        except Exception:
            cfg = {}
    if not isinstance(cfg, dict):
        cfg = copy.deepcopy(DEFAULT_PANEL_CONFIG)
    cfg["animations"] = bool(enabled)
    cfg["reduce_motion"] = not bool(enabled)
    norm = normalize_panel_config(cfg)
    try:
        atomic_write_json(panel_file, norm)
    except Exception:
        pass
    apply_wayfire_animations_and_effects(norm)


def sync_startup_environment() -> None:
    """Wykonywane przy starcie sesji OKay DE: synchronizuje dekoracje, animacje, motyw i Flatpaka."""
    try:
        ensure_user_configs()
        state = get_state_manager()
        cfg = state.get_config()
        apply_window_decorations(
            buttons_style=cfg.get("window_buttons_style", "rectangles"),
            corner_radius=cfg.get("window_corner_radius", 12),
            accent_hex=cfg.get("accent_color", DEFAULT_ACCENT_COLOR),
            theme_mode=cfg.get("theme_mode", "dark"),
            _from_ensure=True,
        )
        apply_wayfire_animations_and_effects(cfg)
        if cfg.get("focus_mode", False):
            set_focus_mode(True)
    except Exception:
        pass


def update_system_accent_color(new_accent: str) -> dict:
    """Aktualizuje kolor akcentu w całej przestrzeni użytkownika OKay DE:
    - ~/.config/okay-de/panel.json (klucz accent_color)
    - ~/.config/wayfire.ini (sekcje [decoration] i [pixdecor] w formacie float RGBA)
    - ~/.config/mako/config (border-color, background-color)
    oraz przeładowuje powiadomienia (makoctl reload).
    """
    clean_hex = sanitize_hex_color(new_accent)
    mako_bg = blend_hex_color("#1e1e2e", clean_hex, 0.08)

    notes = []

    # 1. ~/.config/okay-de/panel.json
    ensure_user_configs()
    panel_file = os.path.join(okay_config_dir(), "panel.json")
    cur_btn_style = "rectangles"
    cur_radius = 12
    cur_theme = "dark"
    try:
        cfg = {}
        if os.path.isfile(panel_file):
            with open(panel_file, "r", encoding="utf-8") as f:
                cfg = json.load(f)
        if not isinstance(cfg, dict):
            cfg = copy.deepcopy(DEFAULT_PANEL_CONFIG)
        cfg["accent_color"] = clean_hex
        cur_btn_style = cfg.get("window_buttons_style", "rectangles")
        cur_radius = cfg.get("window_corner_radius", 12)
        cur_theme = cfg.get("theme_mode", "dark")
        atomic_write_json(panel_file, normalize_panel_config(cfg))
        notes.append("panel.json")
    except Exception:
        pass

    # 2. ~/.config/wayfire.ini ([decoration], [pixdecor], SVG przycisków, GTK CSS)
    try:
        apply_window_decorations(
            buttons_style=cur_btn_style,
            corner_radius=cur_radius,
            accent_hex=clean_hex,
            theme_mode=cur_theme,
        )
        notes.append("wayfire.ini")
        notes.append("decorations")
    except Exception:
        pass

    # 3. ~/.config/mako/config
    mako_dir = os.path.join(xdg_config_home(), "mako")
    mako_file = os.path.join(mako_dir, "config")
    try:
        os.makedirs(mako_dir, exist_ok=True)
        if os.path.isfile(mako_file):
            with open(mako_file, "r", encoding="utf-8") as f:
                m_raw = f.read()
        else:
            m_tmpl = mako_template_file()
            if m_tmpl and os.path.isfile(m_tmpl):
                with open(m_tmpl, "r", encoding="utf-8") as f:
                    m_raw = f.read()
            else:
                m_raw = _DEFAULT_MAKO_CONFIG
        m_updated = re.sub(r"^(background-color=)[0-9a-fA-F#]+([0-9a-fA-F]{2})$", rf"\g<1>{mako_bg}\g<2>", m_raw, flags=re.MULTILINE)
        lines = []
        cur_sec = "main"
        for line in m_updated.splitlines():
            stripped = line.strip()
            if stripped.startswith("[") and stripped.endswith("]"):
                cur_sec = stripped[1:-1].strip().lower()
                lines.append(line)
                continue
            if stripped.startswith("border-color="):
                if cur_sec in ("main", "urgency=normal"):
                    lines.append(f"border-color={clean_hex}")
                    continue
            lines.append(line)
        atomic_write_text(mako_file, "\n".join(lines) + "\n", mode=0o644)
        notes.append("mako")
        if shutil.which("makoctl"):
            run_cmd(["makoctl", "reload"], timeout=3)
    except Exception:
        pass

    return {
        "status": "ok",
        "accent": clean_hex,
        "notes": notes,
        "contrast": compute_contrast_text(clean_hex),
        "hover": compute_hover_hex(clean_hex),
    }


class StateManager:
    """
    Centralny, reaktywny menedżer stanu i konfiguracji OKay DE.
    Zapewnia:
    - Atomowy zapis do ~/.config/okay-de/panel.json z walidacją typów i migracją schematu,
    - Nasłuchiwanie zmian na żywo przez Gio.FileMonitor (inotify) z debouncingiem,
    - Reaktywne powiadamianie obserwatorów UI (subscribe / unsubscribe),
    - Automatyczną synchronizację z silnikiem animacji oraz kompozytorem Wayfire.
    """

    def __init__(self):
        self._config_path = os.path.join(okay_config_dir(), "panel.json")
        self._observers = []
        self._monitor = None
        self._debounce_id = 0
        self._data = copy.deepcopy(DEFAULT_PANEL_CONFIG)
        self.reload(notify=False)
        self._setup_file_monitor()

    def _setup_file_monitor(self):
        try:
            os.makedirs(os.path.dirname(self._config_path), exist_ok=True)
            gfile = Gio.File.new_for_path(self._config_path)
            self._monitor = gfile.monitor_file(Gio.FileMonitorFlags.NONE, None)
            self._monitor.connect("changed", self._on_file_changed)
        except Exception:
            self._monitor = None

    def _on_file_changed(self, _monitor, _file, _other_file, event_type):
        if event_type in (
            Gio.FileMonitorEvent.CHANGED,
            Gio.FileMonitorEvent.CREATED,
            Gio.FileMonitorEvent.CHANGES_DONE_HINT,
            Gio.FileMonitorEvent.ATTRIBUTE_CHANGED,
        ):
            if self._debounce_id:
                GLib.source_remove(self._debounce_id)
            self._debounce_id = GLib.timeout_add(80, self._debounced_reload)

    def _debounced_reload(self):
        self._debounce_id = 0
        self.reload(notify=True)
        return False

    def reload(self, notify: bool = True) -> dict:
        raw = {}
        if os.path.isfile(self._config_path):
            try:
                with open(self._config_path, "r", encoding="utf-8") as f:
                    raw = json.load(f)
            except Exception:
                raw = {}
        self._data = normalize_panel_config(raw)
        try:
            for p in [os.path.abspath(os.path.join(os.path.dirname(__file__), "..")), xdg_bin_home()]:
                if os.path.isdir(os.path.join(p, "panel")) and p not in sys.path:
                    sys.path.insert(0, p)
            from panel.animations import configure_animations
            configure_animations(
                reduce_motion=self._data.get("reduce_motion", False),
                speed_multiplier=self._data.get("animation_speed", 1.0),
            )
        except Exception:
            pass
        if notify:
            self._notify_observers()
        return copy.deepcopy(self._data)

    def get_config(self) -> dict:
        return copy.deepcopy(self._data)

    def get(self, key: str, default=None):
        return copy.deepcopy(self._data.get(key, default))

    def update(self, updates: dict, sync_wayfire: bool = True) -> dict:
        if not isinstance(updates, dict):
            return self.get_config()
        merged = copy.deepcopy(self._data)
        merged.update(updates)
        self._data = normalize_panel_config(merged)
        try:
            atomic_write_json(self._config_path, self._data)
        except Exception:
            pass

        if sync_wayfire:
            anim_keys = {
                "reduce_motion", "animations", "animation_speed", "open_effect",
                "minimize_effect", "blur_enabled", "wobbly_enabled", "window_opacity",
                "workspace_count", "idle_timeout",
            }
            decor_keys = {"window_buttons_style", "window_corner_radius", "window_rounding", "accent_color", "theme_mode"}
            if anim_keys.intersection(updates.keys()):
                apply_wayfire_animations_and_effects(self._data)
            if decor_keys.intersection(updates.keys()):
                apply_window_decorations(
                    buttons_style=self._data["window_buttons_style"],
                    corner_radius=self._data["window_corner_radius"],
                    accent_hex=self._data["accent_color"],
                    theme_mode=self._data["theme_mode"],
                )

        self._notify_observers()
        return self.get_config()

    def subscribe(self, callback):
        if callable(callback) and callback not in self._observers:
            self._observers.append(callback)

    def unsubscribe(self, callback):
        if callback in self._observers:
            self._observers.remove(callback)

    def _notify_observers(self):
        snapshot = copy.deepcopy(self._data)
        for cb in list(self._observers):
            try:
                cb(snapshot)
            except Exception:
                pass


_STATE_MANAGER_INSTANCE = None


def get_state_manager() -> StateManager:
    global _STATE_MANAGER_INSTANCE
    if _STATE_MANAGER_INSTANCE is None:
        _STATE_MANAGER_INSTANCE = StateManager()
    return _STATE_MANAGER_INSTANCE
