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
from gi.repository import Gtk, Gdk, GLib, Pango


DEFAULT_ACCENT_COLOR = "#00e676"

COLOR_PRESETS = [
    ("#00e676", "OKsavi Neon"),
    ("#89b4fa", "Catppuccin Blue"),
    ("#cba6f7", "Mauve Fiolet"),
    ("#94e2d5", "Teal Morski"),
    ("#fab387", "Peach Brzoskwinia"),
    ("#f5c2e7", "Flamingo Róż"),
    ("#f38ba8", "Red Czerwień"),
    ("#f9e2af", "Sun Słoneczny"),
    ("#b4befe", "Lavender Lawenda"),
    ("#2ecc71", "Emerald Szmaragd"),
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


SETTINGS_CSS_TEMPLATE = """
window#okay-settings {
    background-color: __BG_WIN__;
    color: #f5f5f5;
    font-family: "Inter", "JetBrains Mono", system-ui, sans-serif;
    font-size: 13px;
}

.settings-sidebar {
    background-color: __BG_SIDEBAR__;
    border-right: 1px solid #282828;
    padding: 8px 6px;
}

.settings-sidebar list {
    background-color: __BG_SIDEBAR__;
}

.settings-sidebar row {
    color: #cdd6f4;
    border-radius: 6px;
    margin: 2px 4px;
    padding: 10px 14px;
    font-size: 13.5px;
    font-weight: 500;
}

.settings-sidebar row:hover {
    background-color: #222222;
    color: #ffffff;
}

.settings-sidebar row:selected {
    background-color: rgba(__ACCENT_RGB__, 0.16);
    color: __ACCENT__;
    font-weight: 700;
    border-left: 3px solid __ACCENT__;
}

.settings-brand {
    background-color: __BG_SIDEBAR__;
    border-right: 1px solid #282828;
    border-bottom: 1px solid #282828;
    padding: 14px 16px;
}

.brand-title {
    color: __ACCENT__;
    font-size: 15.5px;
    font-weight: 800;
    letter-spacing: 0.5px;
}

.brand-subtitle {
    color: #bac2de;
    font-size: 12px;
}

.page-title {
    color: #ffffff;
    font-size: 20px;
    font-weight: 700;
}

.page-subtitle {
    color: #bac2de;
    font-size: 13px;
}

.card-frame {
    background-color: __BG_CARD__;
    border: 1px solid rgba(__ACCENT_RGB__, 0.18);
    border-radius: 8px;
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
    color: #ffffff;
    font-size: 13.5px;
    font-weight: 600;
}

.row-subtitle {
    color: #bac2de;
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
    border-radius: 4px;
    padding: 3px 8px;
    font-size: 12px;
    font-weight: 700;
}

list.settings-list {
    background-color: #161616;
    border: 1px solid #282828;
    border-radius: 6px;
}

list.settings-list row {
    padding: 9px 12px;
    border-bottom: 1px solid #222222;
    color: #f0f0f0;
    font-size: 13px;
}

list.settings-list row:last-child {
    border-bottom: none;
}

list.settings-list row:hover {
    background-color: #222222;
}

list.settings-list row:selected {
    background-color: rgba(__ACCENT_RGB__, 0.14);
    color: #ffffff;
}

button.accent-btn {
    background-color: __ACCENT__;
    background-image: none;
    color: __ACCENT_CONTRAST__;
    border: none;
    border-radius: 5px;
    padding: 7px 15px;
    font-weight: 700;
    font-size: 12.5px;
}

button.accent-btn:hover {
    background-color: __ACCENT_HOVER__;
}

button.accent-btn:disabled {
    background-color: #2c2c2c;
    color: #666666;
}

button.secondary-btn {
    background-color: #242424;
    background-image: none;
    color: #f0f0f0;
    border: 1px solid #383838;
    border-radius: 5px;
    padding: 7px 13px;
    font-weight: 600;
    font-size: 12.5px;
}

button.secondary-btn:hover {
    background-color: #2e2e2e;
    border-color: __ACCENT__;
}

button.danger-btn {
    background-color: #2b181c;
    background-image: none;
    color: #f38ba8;
    border: 1px solid #592632;
    border-radius: 5px;
    padding: 7px 13px;
    font-weight: 600;
    font-size: 12.5px;
}

button.danger-btn:hover {
    background-color: #3a1f25;
}

button.scale-pill {
    background-color: #222222;
    background-image: none;
    color: #dddddd;
    border: 1px solid #383838;
    border-radius: 5px;
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
    background-color: #202020;
    background-image: none;
    color: #f0f0f0;
    border: 1px solid #353535;
    border-radius: 6px;
    padding: 6px 10px;
    font-weight: 600;
    font-size: 12.5px;
}

button.color-preset-btn:hover {
    background-color: #2a2a2a;
    border-color: __ACCENT__;
}

button.color-preset-btn:checked {
    background-color: rgba(__ACCENT_RGB__, 0.16);
    color: #ffffff;
    border: 1px solid __ACCENT__;
}

switch {
    background-color: #2d2d2d;
    border: 1px solid #3f3f3f;
    border-radius: 12px;
}

switch:checked {
    background-color: __ACCENT__;
    border-color: __ACCENT__;
}

switch slider {
    background-color: #f5f5f5;
    border-radius: 10px;
}

scale highlight {
    background-color: __ACCENT__;
    border-radius: 3px;
}

scale trough {
    background-color: #2c2c2c;
    border-radius: 3px;
    min-height: 6px;
}

scale slider {
    background-color: __ACCENT__;
    border: 2px solid #111111;
    border-radius: 10px;
    min-width: 16px;
    min-height: 16px;
}

entry, combobox button {
    background-color: #121212;
    background-image: none;
    color: #ffffff;
    border: 1px solid #383838;
    border-radius: 5px;
    padding: 6px 10px;
    font-size: 12.5px;
}

entry:focus {
    border-color: __ACCENT__;
}

combobox menuitem {
    font-size: 12.5px;
    color: #cdd6f4;
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


def get_settings_css(accent_hex: str = DEFAULT_ACCENT_COLOR) -> str:
    clean = sanitize_hex_color(accent_hex)
    r, g, b = parse_hex_rgb(clean)
    hover = compute_hover_hex(clean)
    contrast = compute_contrast_text(clean)
    bg_win = blend_hex_color("#111111", clean, 0.035)
    bg_sidebar = blend_hex_color("#151515", clean, 0.045)
    bg_card = blend_hex_color("#1a1a1a", clean, 0.045)
    return (
        SETTINGS_CSS_TEMPLATE
        .replace("__ACCENT__", clean)
        .replace("__ACCENT_RGB__", f"{r}, {g}, {b}")
        .replace("__ACCENT_HOVER__", hover)
        .replace("__ACCENT_CONTRAST__", contrast)
        .replace("__BG_WIN__", bg_win)
        .replace("__BG_SIDEBAR__", bg_sidebar)
        .replace("__BG_CARD__", bg_card)
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


def apply_settings_css(accent_hex: str = None):
    """Rejestruje lub przeładowuje dynamiczny styl CSS OKsavi Linux dla okna OKay Settings."""
    global _SETTINGS_CSS_PROVIDER
    target_hex = sanitize_hex_color(accent_hex) if accent_hex else get_current_accent_color()
    css_data = get_settings_css(target_hex).encode("utf-8")
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
    "icon_only": True,
    "show_labels": False,
    "position": "bottom",
    "mode": "panel",
    "animations": True,
    "accent_color": DEFAULT_ACCENT_COLOR,
    "window_buttons_style": "rectangles",
    "window_corner_radius": 0,
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

_LABWC_CONFIG_FILES = (
    "rc.xml",
    "autostart",
    "environment",
    "menu.xml",
    "themerc",
    "themerc-override",
)

_LABWC_ASSET_EXTS = (".xbm", ".svg")


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


def labwc_config_dir() -> str:
    return os.path.join(xdg_config_home(), "labwc")


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


def labwc_template_dirs() -> tuple:
    dirs = [
        "/etc/okay-de/labwc",
        "/etc/xdg/labwc",
        "/usr/share/okay-de/defaults/labwc",
        "/usr/share/okay-de/labwc",
        "/usr/local/share/okay-de/defaults/labwc",
        "/usr/local/share/okay-de/labwc",
        os.path.join(okay_data_dir(), "defaults", "labwc"),
        os.path.join(okay_data_dir(), "repo", "config", "labwc"),
    ]
    repo_root = _repo_root_from_script()
    if repo_root:
        dirs.append(os.path.join(repo_root, "config", "labwc"))
    return tuple(dirs)


def okay_template_file(rel_path: str) -> str:
    return find_existing_file(*(os.path.join(d, rel_path) for d in okay_template_dirs()))


def labwc_template_file(rel_path: str) -> str:
    return find_existing_file(*(os.path.join(d, rel_path) for d in labwc_template_dirs()))


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


def fuzzel_template_file() -> str:
    candidates = [
        "/etc/xdg/fuzzel/fuzzel.ini",
        os.path.join(okay_data_dir(), "defaults", "fuzzel", "fuzzel.ini"),
        os.path.join(okay_data_dir(), "repo", "config", "fuzzel", "fuzzel.ini"),
    ]
    repo_root = _repo_root_from_script()
    if repo_root:
        candidates.append(os.path.join(repo_root, "config", "fuzzel", "fuzzel.ini"))
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


_DEFAULT_FUZZEL_INI = """[main]
font=JetBrains Mono:size=11
prompt="❯ "
icon-theme=Papirus-Dark
icons-enabled=yes
terminal=foot -e
layer=overlay
width=32
lines=10
horizontal-pad=18
vertical-pad=12
inner-pad=8

[colors]
background=1b2b30ff
text=cdd6f4ff
prompt=00e676ff
match=00e676ff
selection=313244ff
selection-text=cdd6f4ff
selection-match=00e676ff
border=00e676ff

[border]
width=2
radius=8
"""

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
    data = copy.deepcopy(DEFAULT_PANEL_CONFIG)
    if isinstance(cfg, dict):
        data.update(cfg)
    if "icon_only" in data:
        icon_only = bool(data["icon_only"])
    elif "show_labels" in data:
        icon_only = not bool(data["show_labels"])
    else:
        icon_only = True
    data["icon_only"] = icon_only
    data["show_labels"] = not icon_only
    pos = str(data.get("position", "bottom")).lower()
    data["position"] = "top" if pos == "top" else "bottom"
    mode = str(data.get("mode", "panel")).lower()
    data["mode"] = "dock" if mode == "dock" else "panel"
    data["animations"] = bool(data.get("animations", True))
    raw_accent = str(data.get("accent_color", DEFAULT_ACCENT_COLOR)).strip()
    data["accent_color"] = sanitize_hex_color(raw_accent, DEFAULT_ACCENT_COLOR)
    btn_style = str(data.get("window_buttons_style", "rectangles")).lower()
    data["window_buttons_style"] = btn_style if btn_style in ("rectangles", "circles_left", "circles_right", "windows") else "rectangles"
    try:
        radius = int(data.get("window_corner_radius", 0))
        data["window_corner_radius"] = radius if 0 <= radius <= 32 else 0
    except Exception:
        data["window_corner_radius"] = 0
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
    """Inicjalizuje ~/.config/okay-de oraz ~/.config/labwc z szablonów systemowych (read-only) przy pierwszym starcie."""
    okay_dir = okay_config_dir()
    labwc_dir = labwc_config_dir()
    os.makedirs(okay_dir, exist_ok=True)
    os.makedirs(labwc_dir, exist_ok=True)

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

    # 4. Pliki labwc (~/.config/labwc/{rc.xml, autostart, environment, menu.xml, themerc, ...})
    for fname in _LABWC_CONFIG_FILES:
        dst = os.path.join(labwc_dir, fname)
        if not os.path.exists(dst):
            src = labwc_template_file(fname)
            if src:
                try:
                    atomic_copy_file(src, dst, mode=0o755 if fname == "autostart" else 0o644)
                except Exception:
                    pass

    # 5. Maski przycisków okna labwc (*.xbm, *.svg)
    seen_assets = set()
    for tdir in labwc_template_dirs():
        if not os.path.isdir(tdir):
            continue
        try:
            for entry in os.listdir(tdir):
                if entry.endswith(_LABWC_ASSET_EXTS) and entry not in seen_assets:
                    seen_assets.add(entry)
                    dst = os.path.join(labwc_dir, entry)
                    if not os.path.exists(dst):
                        try:
                            atomic_copy_file(os.path.join(tdir, entry), dst, mode=0o644)
                        except Exception:
                            pass
        except Exception:
            pass

    # 6. Motyw użytkownika (~/.local/share/themes/OKay) z szablonu systemowego (read-only)
    user_theme_dir = os.path.join(xdg_data_home(), "themes", "OKay")
    if not os.path.exists(user_theme_dir):
        for sys_theme in (
            "/usr/share/themes/OKay",
            "/usr/local/share/themes/OKay",
            "/usr/share/okay-de/themes/OKay",
            os.path.join(okay_data_dir(), "repo", "themes", "OKay"),
        ):
            if os.path.isdir(sys_theme):
                try:
                    shutil.copytree(sys_theme, user_theme_dir)
                except Exception:
                    pass
                break

    # 7. Konfiguracja fuzzel (~/.config/fuzzel/fuzzel.ini)
    created_fuzzel = False
    fuzzel_dir = os.path.join(xdg_config_home(), "fuzzel")
    fuzzel_ini = os.path.join(fuzzel_dir, "fuzzel.ini")
    if not os.path.exists(fuzzel_ini):
        os.makedirs(fuzzel_dir, exist_ok=True)
        f_tmpl = fuzzel_template_file()
        if f_tmpl and os.path.isfile(f_tmpl):
            try:
                atomic_copy_file(f_tmpl, fuzzel_ini, mode=0o644)
                created_fuzzel = True
            except Exception:
                pass
        else:
            try:
                atomic_write_text(fuzzel_ini, _DEFAULT_FUZZEL_INI, mode=0o644)
                created_fuzzel = True
            except Exception:
                pass

    # 8. Konfiguracja mako (~/.config/mako/config)
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

    if created_fuzzel or created_mako:
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
            if shutil.which("labwc"):
                rc_l, _o, err_l = run_cmd(["labwc", "--reconfigure"], timeout=5)
                if rc_l == 0:
                    notes.append("Przeładowano konfigurację kompozytora (labwc --reconfigure).")
                else:
                    notes.append(f"Ostrzeżenie labwc --reconfigure: {err_l.strip() or rc_l}")
            panel_bin = os.path.join(xdg_bin_home(), "okay-panel")
            if not (os.path.isfile(panel_bin) and os.access(panel_bin, os.X_OK)):
                panel_bin = shutil.which("okay-panel") or ""
            if panel_bin:
                try:
                    subprocess.Popen(
                        [panel_bin, "--restart"],
                        stdin=subprocess.DEVNULL,
                        stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL,
                        start_new_session=True,
                    )
                    notes.append("Zrestartowano pasek zadań (okay-panel).")
                except Exception as exc:
                    notes.append(f"Nie udało się uruchomić okay-panel: {exc}")
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


def generate_window_button_svgs(style: str, accent_hex: str) -> dict:
    """Generuje kod SVG dla przycisków okna (close, max, max_toggled, iconify) w zadanym stylu."""
    accent = sanitize_hex_color(accent_hex)
    hover_accent = compute_hover_hex(accent)
    style = style.lower() if style in ("rectangles", "circles_left", "circles_right", "windows") else "rectangles"

    if style == "rectangles":
        return {
            "close.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="48" height="10" viewBox="0 0 48 10"><rect width="48" height="10" rx="0" fill="#f38ba8" /></svg>\n',
            "close_hover.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="48" height="10" viewBox="0 0 48 10"><rect width="48" height="10" rx="0" fill="#eba0ac" /></svg>\n',
            "max.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="48" height="10" viewBox="0 0 48 10"><rect width="48" height="10" rx="0" fill="{accent}" /></svg>\n',
            "max_hover.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="48" height="10" viewBox="0 0 48 10"><rect width="48" height="10" rx="0" fill="{hover_accent}" /></svg>\n',
            "max_toggled.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="48" height="10" viewBox="0 0 48 10"><rect width="48" height="10" rx="0" fill="{accent}" /></svg>\n',
            "iconify.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="48" height="10" viewBox="0 0 48 10"><rect width="48" height="10" rx="0" fill="#585b70" /></svg>\n',
            "iconify_hover.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="48" height="10" viewBox="0 0 48 10"><rect width="48" height="10" rx="0" fill="#6c7086" /></svg>\n',
        }
    elif style in ("circles_left", "circles_right"):
        return {
            "close.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 14 14"><circle cx="7" cy="7" r="6" fill="#f38ba8" /></svg>\n',
            "close_hover.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 14 14"><circle cx="7" cy="7" r="6" fill="#ed6a5e" /><path d="M4.5 4.5 L9.5 9.5 M9.5 4.5 L4.5 9.5" stroke="#4c0000" stroke-width="1.2" stroke-linecap="round" /></svg>\n',
            "max.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 14 14"><circle cx="7" cy="7" r="6" fill="{accent}" /></svg>\n',
            "max_hover.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 14 14"><circle cx="7" cy="7" r="6" fill="{hover_accent}" /><path d="M4.5 4.5 L7.5 4.5 L4.5 7.5 Z M9.5 9.5 L6.5 9.5 L9.5 6.5 Z" fill="#11111b" /></svg>\n',
            "max_toggled.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 14 14"><circle cx="7" cy="7" r="6" fill="{accent}" /></svg>\n',
            "iconify.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 14 14"><circle cx="7" cy="7" r="6" fill="#f9e2af" /></svg>\n',
            "iconify_hover.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 14 14"><circle cx="7" cy="7" r="6" fill="#f5c359" /><line x1="4.5" y1="7" x2="9.5" y2="7" stroke="#4a3a00" stroke-width="1.2" stroke-linecap="round" /></svg>\n',
        }
    else:  # windows
        return {
            "close.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="26" height="20" viewBox="0 0 26 20"><path d="M8 5 L18 15 M18 5 L8 15" stroke="#cdd6f4" stroke-width="1.3" stroke-linecap="round" /></svg>\n',
            "close_hover.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="26" height="20" viewBox="0 0 26 20"><rect width="26" height="20" rx="3" fill="#e81123" /><path d="M8 5 L18 15 M18 5 L8 15" stroke="#ffffff" stroke-width="1.3" stroke-linecap="round" /></svg>\n',
            "max.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="26" height="20" viewBox="0 0 26 20"><rect x="8" y="5" width="10" height="10" rx="1" fill="none" stroke="#cdd6f4" stroke-width="1.3" /></svg>\n',
            "max_hover.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="26" height="20" viewBox="0 0 26 20"><rect width="26" height="20" rx="3" fill="#313244" /><rect x="8" y="5" width="10" height="10" rx="1" fill="none" stroke="{accent}" stroke-width="1.3" /></svg>\n',
            "max_toggled.svg": f'<svg xmlns="http://www.w3.org/2000/svg" width="26" height="20" viewBox="0 0 26 20"><rect x="9.5" y="4" width="8" height="8" rx="1" fill="none" stroke="#cdd6f4" stroke-width="1.1" /><rect x="7" y="6.5" width="8" height="8" rx="1" fill="#181825" stroke="#cdd6f4" stroke-width="1.1" /></svg>\n',
            "iconify.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="26" height="20" viewBox="0 0 26 20"><line x1="8" y1="14" x2="18" y2="14" stroke="#cdd6f4" stroke-width="1.3" stroke-linecap="round" /></svg>\n',
            "iconify_hover.svg": '<svg xmlns="http://www.w3.org/2000/svg" width="26" height="20" viewBox="0 0 26 20"><rect width="26" height="20" rx="3" fill="#313244" /><line x1="8" y1="14" x2="18" y2="14" stroke="#ffffff" stroke-width="1.3" stroke-linecap="round" /></svg>\n',
        }


def apply_window_decorations(buttons_style: str = None, corner_radius: int = None, accent_hex: str = None) -> dict:
    """Zapisuje styl przycisków, zaokrąglenie ramek okien labwc oraz przeładowuje kompozytor."""
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
        corner_radius = cfg.get("window_corner_radius", 0)
    try:
        corner_radius = max(0, min(32, int(corner_radius)))
    except Exception:
        corner_radius = 0

    if accent_hex is None:
        accent_hex = cfg.get("accent_color", DEFAULT_ACCENT_COLOR)
    accent_hex = sanitize_hex_color(accent_hex)

    cfg["window_buttons_style"] = buttons_style
    cfg["window_corner_radius"] = corner_radius
    cfg["accent_color"] = accent_hex
    atomic_write_json(panel_file, normalize_panel_config(cfg))

    labwc_dir = labwc_config_dir()
    os.makedirs(labwc_dir, exist_ok=True)

    # 1. Zapis wektorowych plików SVG dla przycisków w ~/.config/labwc
    svg_map = generate_window_button_svgs(buttons_style, accent_hex)
    for fname, svg_content in svg_map.items():
        atomic_write_text(os.path.join(labwc_dir, fname), svg_content, mode=0o644)

    # 2. Zarządzanie plikami XBM:
    # W stylu innym niż "rectangles" usuwamy stare pliki .xbm w ~/.config/labwc,
    # aby labwc nie używał ich jako maski bitmapowej zamiast nowych SVG.
    # W stylu "rectangles" przywracamy domyślne XBM jeśli brakuje.
    xbm_names = (
        "close.xbm", "close_hover.xbm",
        "max.xbm", "max_hover.xbm", "max_toggled.xbm",
        "iconify.xbm", "iconify_hover.xbm"
    )
    if buttons_style != "rectangles":
        for xbm in xbm_names:
            p = os.path.join(labwc_dir, xbm)
            if os.path.isfile(p):
                try:
                    os.unlink(p)
                except Exception:
                    pass
    else:
        for xbm in xbm_names:
            dst = os.path.join(labwc_dir, xbm)
            if not os.path.isfile(dst):
                src = labwc_template_file(xbm)
                if src and os.path.isfile(src):
                    try:
                        atomic_copy_file(src, dst, mode=0o644)
                    except Exception:
                        pass

    # 3. Parametry geometrii i układu
    if buttons_style == "rectangles":
        layout = ":iconify,max,close"
        btn_w, btn_h, btn_sp = 48, 10, 8
    elif buttons_style == "circles_left":
        layout = "close,iconify,max:"
        btn_w, btn_h, btn_sp = 14, 14, 8
    elif buttons_style == "circles_right":
        layout = ":iconify,max,close"
        btn_w, btn_h, btn_sp = 14, 14, 8
    else:  # windows
        layout = ":iconify,max,close"
        btn_w, btn_h, btn_sp = 26, 20, 2

    # 4. Aktualizacja themerc-override i themerc
    override_path = os.path.join(labwc_dir, "themerc-override")
    def _patch_decorations(raw_text: str) -> str:
        text = raw_text
        reps = [
            (r"^(window\.cornerRadius:\s*).*$", rf"\g<1>{corner_radius}"),
            (r"^(window\.titlebar\.layout:\s*).*$", rf"\g<1>{layout}"),
            (r"^(window\.button\.width:\s*).*$", rf"\g<1>{btn_w}"),
            (r"^(window\.button\.height:\s*).*$", rf"\g<1>{btn_h}"),
            (r"^(window\.button\.spacing:\s*).*$", rf"\g<1>{btn_sp}"),
        ]
        for pattern, repl in reps:
            if re.search(pattern, text, flags=re.MULTILINE):
                text = re.sub(pattern, repl, text, flags=re.MULTILINE)
            else:
                key = pattern.split(r"\s*")[0].lstrip("^").rstrip(":")
                val = repl.rsplit(r">", 1)[-1]
                text += f"\n{key}: {val}"
        return text

    if os.path.isfile(override_path):
        try:
            with open(override_path, "r", encoding="utf-8") as f:
                cur = f.read()
            atomic_write_text(override_path, _patch_decorations(cur), mode=0o644)
        except Exception:
            pass
    user_themerc = os.path.join(labwc_dir, "themerc")
    if os.path.isfile(user_themerc):
        try:
            with open(user_themerc, "r", encoding="utf-8") as f:
                cur = f.read()
            atomic_write_text(user_themerc, _patch_decorations(cur), mode=0o644)
        except Exception:
            pass

    # 5. Aktualizacja rc.xml
    rc_file = os.path.join(labwc_dir, "rc.xml")
    if os.path.isfile(rc_file):
        try:
            with open(rc_file, "r", encoding="utf-8") as f:
                rc_content = f.read()
            rc_content = re.sub(r"(<cornerRadius>).*?(</cornerRadius>)", rf"\g<1>{corner_radius}\g<2>", rc_content)
            rc_content = re.sub(r"(<layout>).*?(</layout>)", rf"\g<1>{layout}\g<2>", rc_content)
            atomic_write_text(rc_file, rc_content, mode=0o644)
        except Exception:
            pass

    # 6. Przeładowanie labwc
    if os.environ.get("WAYLAND_DISPLAY") and shutil.which("labwc"):
        run_cmd(["labwc", "--reconfigure"], timeout=4)

    return {
        "status": "ok",
        "buttons_style": buttons_style,
        "corner_radius": corner_radius,
        "layout": layout,
    }


def update_system_accent_color(new_accent: str) -> dict:
    """Aktualizuje kolor akcentu w całej przestrzeni użytkownika OKay DE:
    - ~/.config/okay-de/panel.json (klucz accent_color)
    - ~/.config/labwc/themerc-override (oraz themerc)
    - ~/.config/mako/config (border-color, background-color)
    - ~/.config/fuzzel/fuzzel.ini (prompt, match, selection-match, border, background)
    oraz asynchronicznie przeładowuje kompozytor (labwc --reconfigure) i powiadomienia (makoctl reload).
    """
    clean_hex = sanitize_hex_color(new_accent)
    hover_hex = compute_hover_hex(clean_hex)
    contrast_text = compute_contrast_text(clean_hex)
    title_bg = blend_hex_color("#181825", clean_hex, 0.12)
    mako_bg = blend_hex_color("#1e1e2e", clean_hex, 0.08)

    notes = []

    # 1. ~/.config/okay-de/panel.json
    ensure_user_configs()
    panel_file = os.path.join(okay_config_dir(), "panel.json")
    cur_btn_style = "rectangles"
    cur_radius = 0
    try:
        cfg = {}
        if os.path.isfile(panel_file):
            with open(panel_file, "r", encoding="utf-8") as f:
                cfg = json.load(f)
        if not isinstance(cfg, dict):
            cfg = copy.deepcopy(DEFAULT_PANEL_CONFIG)
        cfg["accent_color"] = clean_hex
        cur_btn_style = cfg.get("window_buttons_style", "rectangles")
        cur_radius = cfg.get("window_corner_radius", 0)
        atomic_write_json(panel_file, normalize_panel_config(cfg))
        notes.append("panel.json")
    except Exception:
        pass

    # 2. ~/.config/labwc/themerc-override (oraz themerc)
    labwc_dir = labwc_config_dir()
    os.makedirs(labwc_dir, exist_ok=True)
    override_path = os.path.join(labwc_dir, "themerc-override")

    def _patch_themerc_content(raw_text: str) -> str:
        replacements = [
            (r"^(window\.active\.border\.color:\s*).*$", rf"\g<1>{clean_hex}"),
            (r"^(window\.active\.title\.bg\.color:\s*).*$", rf"\g<1>{title_bg}"),
            (r"^(window\.active\.button\.max\.unpressed\.image\.color:\s*).*$", rf"\g<1>{clean_hex}"),
            (r"^(window\.active\.button\.max\.hover\.image\.color:\s*).*$", rf"\g<1>{hover_hex}"),
            (r"^(window\.inactive\.button\.max\.hover\.image\.color:\s*).*$", rf"\g<1>{clean_hex}"),
            (r"^(menu\.border\.color:\s*).*$", rf"\g<1>{clean_hex}"),
            (r"^(menu\.items\.bg\.color:\s*).*$", rf"\g<1>{title_bg}"),
            (r"^(menu\.items\.active\.bg\.color:\s*).*$", rf"\g<1>{clean_hex}"),
            (r"^(menu\.items\.active\.text\.color:\s*).*$", rf"\g<1>{contrast_text}"),
        ]
        text = raw_text
        for pattern, repl in replacements:
            if re.search(pattern, text, flags=re.MULTILINE):
                text = re.sub(pattern, repl, text, flags=re.MULTILINE)
            else:
                key = pattern.split(r"\s*")[0].lstrip("^").rstrip(":")
                val = repl.rsplit(r">", 1)[-1]
                text += f"\n{key}: {val}"
        return text

    existing_themerc = ""
    if os.path.isfile(override_path):
        try:
            with open(override_path, "r", encoding="utf-8") as f:
                existing_themerc = f.read()
        except Exception:
            existing_themerc = ""
    if not existing_themerc:
        src = labwc_template_file("themerc-override") or labwc_template_file("themerc")
        if src and os.path.isfile(src):
            try:
                with open(src, "r", encoding="utf-8") as f:
                    existing_themerc = f.read()
            except Exception:
                existing_themerc = ""

    if existing_themerc:
        updated_themerc = _patch_themerc_content(existing_themerc)
    else:
        updated_themerc = (
            "# OKay DE - Labwc Theme Override\n"
            "window.cornerRadius: 0\n"
            "window.titlebar.layout: :iconify,max,close\n"
            "window.button.width: 48\n"
            "window.button.height: 10\n"
            "window.button.spacing: 8\n"
            "border.width: 2\n"
            f"window.active.border.color: {clean_hex}\n"
            "window.inactive.border.color: #313244\n"
            "titlebar.height: 30\n"
            "window.titlebar.padding: 4\n"
            "window.titlebar.padding.width: 10\n"
            "window.titlebar.padding.height: 10\n"
            f"window.active.title.bg.color: {title_bg}\n"
            "window.inactive.title.bg.color: #181825\n"
            "window.active.label.text.color: transparent\n"
            "window.inactive.label.text.color: transparent\n"
            "window.active.button.close.unpressed.image.color: #f38ba8\n"
            "window.active.button.close.hover.image.color: #eba0ac\n"
            "window.inactive.button.close.unpressed.image.color: #45475a\n"
            "window.inactive.button.close.hover.image.color: #f38ba8\n"
            f"window.active.button.max.unpressed.image.color: {clean_hex}\n"
            f"window.active.button.max.hover.image.color: {hover_hex}\n"
            "window.inactive.button.max.unpressed.image.color: #45475a\n"
            f"window.inactive.button.max.hover.image.color: {clean_hex}\n"
            "window.active.button.iconify.unpressed.image.color: #585b70\n"
            "window.active.button.iconify.hover.image.color: #6c7086\n"
            "window.inactive.button.iconify.unpressed.image.color: #313244\n"
            "window.inactive.button.iconify.hover.image.color: #585b70\n"
            "menu.border.width: 1\n"
            f"menu.border.color: {clean_hex}\n"
            f"menu.items.bg.color: {title_bg}\n"
            "menu.items.text.color: #cdd6f4\n"
            f"menu.items.active.bg.color: {clean_hex}\n"
            f"menu.items.active.text.color: {contrast_text}\n"
        )
    try:
        atomic_write_text(override_path, updated_themerc, mode=0o644)
        notes.append("themerc-override")
    except Exception:
        pass

    user_themerc = os.path.join(labwc_dir, "themerc")
    if os.path.isfile(user_themerc):
        try:
            with open(user_themerc, "r", encoding="utf-8") as f:
                t_raw = f.read()
            atomic_write_text(user_themerc, _patch_themerc_content(t_raw), mode=0o644)
        except Exception:
            pass

    for theme_sub in ("labwc", "openbox-3"):
        theme_rc = os.path.join(xdg_data_home(), "themes", "OKay", theme_sub, "themerc")
        if os.path.isfile(theme_rc):
            try:
                with open(theme_rc, "r", encoding="utf-8") as f:
                    tr_raw = f.read()
                atomic_write_text(theme_rc, _patch_themerc_content(tr_raw), mode=0o644)
            except Exception:
                pass

    # Synchronizacja wektorowych przycisków okna labwc z nowym kolorem akcentu
    try:
        apply_window_decorations(buttons_style=cur_btn_style, corner_radius=cur_radius, accent_hex=clean_hex)
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

    # 4. ~/.config/fuzzel/fuzzel.ini
    fuzzel_dir = os.path.join(xdg_config_home(), "fuzzel")
    fuzzel_file = os.path.join(fuzzel_dir, "fuzzel.ini")
    try:
        os.makedirs(fuzzel_dir, exist_ok=True)
        if os.path.isfile(fuzzel_file):
            with open(fuzzel_file, "r", encoding="utf-8") as f:
                f_raw = f.read()
        else:
            f_tmpl = fuzzel_template_file()
            if f_tmpl and os.path.isfile(f_tmpl):
                with open(f_tmpl, "r", encoding="utf-8") as f:
                    f_raw = f.read()
            else:
                f_raw = _DEFAULT_FUZZEL_INI
        hex_no_hash = clean_hex.lstrip("#").lower()
        f_bg_hex = blend_hex_color("#1b2b30", clean_hex, 0.08).lstrip("#").lower()
        target_colors = {
            "prompt": f"{hex_no_hash}ff",
            "match": f"{hex_no_hash}ff",
            "selection-match": f"{hex_no_hash}ff",
            "border": f"{hex_no_hash}ff",
            "background": f"{f_bg_hex}ff",
        }
        f_lines = []
        cur_sec = ""
        seen_keys = set()
        for raw_line in f_raw.splitlines():
            s = raw_line.strip()
            if s.startswith("[") and s.endswith("]"):
                if cur_sec == "colors":
                    for k, v in target_colors.items():
                        if k not in seen_keys:
                            f_lines.append(f"{k}={v}")
                cur_sec = s[1:-1].strip().lower()
                f_lines.append(raw_line)
                continue
            if cur_sec == "colors" and "=" in s and not s.startswith("#") and not s.startswith(";"):
                k = s.split("=", 1)[0].strip().lower()
                if k in target_colors:
                    f_lines.append(f"{k}={target_colors[k]}")
                    seen_keys.add(k)
                    continue
            f_lines.append(raw_line)
        if cur_sec == "colors":
            for k, v in target_colors.items():
                if k not in seen_keys:
                    f_lines.append(f"{k}={v}")
        elif not any(l.strip().lower() == "[colors]" for l in f_lines):
            f_lines.append("")
            f_lines.append("[colors]")
            for k, v in target_colors.items():
                f_lines.append(f"{k}={v}")
        atomic_write_text(fuzzel_file, "\n".join(f_lines) + "\n", mode=0o644)
        notes.append("fuzzel")
    except Exception:
        pass

    return {
        "status": "ok",
        "accent": clean_hex,
        "notes": notes,
        "contrast": contrast_text,
        "hover": hover_hex,
    }
