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

menu,
combobox menu,
combobox window menu {
    background-color: #181825;
    background-image: none;
    color: #cdd6f4;
    border: 1px solid rgba(__ACCENT_RGB__, 0.45);
    border-radius: 6px;
    padding: 4px 0;
}

menu menuitem,
combobox menuitem {
    background-color: transparent;
    background-image: none;
    color: #cdd6f4;
    padding: 6px 12px;
    font-size: 12.5px;
}

menu menuitem label,
combobox menuitem label,
combobox cellview {
    color: #cdd6f4;
}

menu menuitem:hover,
combobox menuitem:hover {
    background-color: rgba(__ACCENT_RGB__, 0.22);
    color: #ffffff;
}

menu menuitem:hover label,
combobox menuitem:hover label {
    color: #ffffff;
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


def update_wayfire_section(section: str, updates: dict) -> bool:
    """Atomowo aktualizuje lub dopisuje klucze w sekcji [section] pliku ~/.config/wayfire.ini."""
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
    pending = {str(k): str(v) for k, v in updates.items()}

    def _flush_pending():
        for pk, pv in pending.items():
            out_lines.append(f"{pk} = {pv}")
        pending.clear()

    for line in lines:
        s = line.strip()
        if s.startswith("[") and s.endswith("]"):
            if in_sec and pending:
                _flush_pending()
            in_sec = (s == target_sec)
            if in_sec:
                sec_found = True
            out_lines.append(line)
            continue
        if in_sec and "=" in s and not s.startswith("#") and not s.startswith(";"):
            k = s.split("=", 1)[0].strip()
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

    # 8b. Konfiguracja Wayfire (~/.config/wayfire.ini)
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
    if os.path.isfile(wf_ini) and (
        os.path.isfile("/usr/lib/wayfire/libpixdecor.so")
        or os.path.isfile("/usr/local/lib/wayfire/libpixdecor.so")
    ):
        plugins_line = read_wayfire_value("core", "plugins", "")
        if plugins_line and "pixdecor" not in plugins_line.split() and "decoration" in plugins_line.split():
            new_plugins = re.sub(r"\bdecoration\b", "pixdecor", plugins_line)
            update_wayfire_section("core", {"plugins": new_plugins})

    if created_fuzzel or created_mako or created_wf:
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


def apply_window_decorations(buttons_style: str = None, corner_radius: int = None, accent_hex: str = None) -> dict:
    """Zapisuje styl przycisków, zaokrąglenie ramek okien w Wayfire / GTK oraz przeładowuje powiadomienia."""
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

    layout = "close minimize maximize" if buttons_style == "circles_left" else "minimize maximize close"

    # Wymuszenie zaokrąglenia wszystkich 4 narożników w aplikacjach GTK3 / GTK4
    marker_start = "/* OKAY-DE-CORNERS-START */"
    marker_end = "/* OKAY-DE-CORNERS-END */"
    gtk3_block = (
        f"{marker_start}\n"
        f"window, window.background, window.csd, window.ssd, window.solid-csd,\n"
        f"decoration, messagedialog, dialog, popover > contents, menu {{\n"
        f"    border-radius: {corner_radius}px;\n"
        f"}}\n"
        f"window.background > box, window.background > stack, window.background > paned {{\n"
        f"    border-bottom-left-radius: {corner_radius}px;\n"
        f"    border-bottom-right-radius: {corner_radius}px;\n"
        f"}}\n"
        f"{marker_end}\n"
    )
    gtk4_block = (
        f"{marker_start}\n"
        f"window, window.background, window.csd, window.ssd, window.solid-csd,\n"
        f"decoration, messagedialog, dialog, popover > contents {{\n"
        f"    border-radius: {corner_radius}px;\n"
        f"    overflow: hidden;\n"
        f"}}\n"
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

    # Synchronizacja zaokrąglenia w fuzzel (~/.config/fuzzel/fuzzel.ini) oraz mako (~/.config/mako/config)
    fuzzel_ini = os.path.join(xdg_config_home(), "fuzzel", "fuzzel.ini")
    if os.path.isfile(fuzzel_ini):
        try:
            with open(fuzzel_ini, "r", encoding="utf-8") as f:
                f_txt = f.read()
            if re.search(r"^radius\s*=.*$", f_txt, flags=re.MULTILINE):
                f_txt = re.sub(r"^(radius\s*=\s*).*$", rf"\g<1>{corner_radius}", f_txt, flags=re.MULTILINE)
            elif re.search(r"^\[border\]", f_txt, flags=re.MULTILINE):
                f_txt = re.sub(r"^(\[border\]\s*\n)", rf"\g<1>radius={corner_radius}\n", f_txt, count=1, flags=re.MULTILINE)
            atomic_write_text(fuzzel_ini, f_txt, mode=0o644)
        except Exception:
            pass

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

    # Aktualizacja Wayfire (~/.config/wayfire.ini przeładowuje się automatycznie) oraz mako
    update_wayfire_section("decoration", {
        "button_order": layout,
        "active_color": f"{accent_hex}ff",
    })
    update_wayfire_section("pixdecor", {
        "button_order": layout,
        "rounded_corner_radius": str(corner_radius),
        "rounded_corners": "true" if corner_radius > 0 else "false",
        "active_color": f"{accent_hex}ff",
    })

    if shutil.which("makoctl"):
        run_cmd(["makoctl", "reload"], timeout=2)

    return {
        "status": "ok",
        "buttons_style": buttons_style,
        "corner_radius": corner_radius,
        "layout": layout,
    }


def sync_gtk_animations(enabled: bool) -> None:
    """Synchronizuje animacje GTK (settings.ini) oraz kompozytora Wayfire (~/.config/wayfire.ini)."""
    val = "1" if enabled else "0"
    for gtk_ver in ("gtk-3.0", "gtk-4.0"):
        ini_path = os.path.join(xdg_config_home(), gtk_ver, "settings.ini")
        try:
            content = "[Settings]\n"
            if os.path.isfile(ini_path):
                with open(ini_path, "r", encoding="utf-8") as f:
                    content = f.read()
            if re.search(r"^gtk-enable-animations\s*=.*$", content, flags=re.MULTILINE):
                content = re.sub(
                    r"^(gtk-enable-animations\s*=\s*).*$",
                    rf"\g<1>{val}",
                    content,
                    flags=re.MULTILINE,
                )
            else:
                if "[Settings]" not in content:
                    content = "[Settings]\n" + content
                content = content.rstrip() + f"\ngtk-enable-animations = {val}\n"
            atomic_write_text(ini_path, content, mode=0o644)
        except Exception:
            pass

    anim_mode = "fade" if enabled else "none"
    update_wayfire_section("animate", {
        "open_animation": anim_mode,
        "close_animation": anim_mode,
        "minimize_animation": anim_mode,
    })


def update_system_accent_color(new_accent: str) -> dict:
    """Aktualizuje kolor akcentu w całej przestrzeni użytkownika OKay DE:
    - ~/.config/okay-de/panel.json (klucz accent_color)
    - ~/.config/wayfire.ini (sekcje [decoration] i [pixdecor])
    - ~/.config/mako/config (border-color, background-color)
    - ~/.config/fuzzel/fuzzel.ini (prompt, match, selection-match, border, background)
    oraz przeładowuje powiadomienia (makoctl reload).
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

    # 2. ~/.config/wayfire.ini
    if update_wayfire_section("decoration", {"active_color": f"{clean_hex}ff"}) and update_wayfire_section("pixdecor", {"active_color": f"{clean_hex}ff", "fg_color": f"{title_bg}ff"}):
        notes.append("wayfire.ini")

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
