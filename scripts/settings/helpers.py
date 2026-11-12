# -*- coding: utf-8 -*-

"""
Wspólne narzędzia GUI (GTK3), stylistyka OKsavi oraz asynchroniczne wykonywanie poleceń
dla modułów centrum sterowania OKay Settings.
"""

import os
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


SETTINGS_CSS = """
window#okay-settings {
    background-color: #111111;
    color: #f0f0f0;
    font-family: "Inter", "JetBrains Mono", sans-serif;
}

.settings-sidebar {
    background-color: #151515;
    border-right: 1px solid #262626;
    padding: 8px 6px;
}

.settings-sidebar list {
    background-color: #151515;
}

.settings-sidebar row {
    color: #b8c2cc;
    border-radius: 4px;
    margin: 2px 4px;
    padding: 10px 14px;
    font-size: 13px;
    font-weight: 500;
}

.settings-sidebar row:hover {
    background-color: #1f1f1f;
    color: #ffffff;
}

.settings-sidebar row:selected {
    background-color: rgba(0, 230, 118, 0.14);
    color: #00e676;
    font-weight: 700;
    border-left: 3px solid #00e676;
}

.settings-brand {
    background-color: #151515;
    border-right: 1px solid #262626;
    border-bottom: 1px solid #262626;
    padding: 14px 16px;
}

.brand-title {
    color: #00e676;
    font-size: 15px;
    font-weight: 800;
    letter-spacing: 0.4px;
}

.brand-subtitle {
    color: #8b949e;
    font-size: 11px;
}

.page-title {
    color: #ffffff;
    font-size: 20px;
    font-weight: 700;
}

.page-subtitle {
    color: #8b949e;
    font-size: 12px;
}

.card-frame {
    background-color: #1a1a1a;
    border: 1px solid #2a2a2a;
    border-radius: 6px;
    padding: 14px 16px;
}

.card-title {
    color: #00e676;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.5px;
    margin-bottom: 6px;
}

.row-title {
    color: #f0f0f0;
    font-size: 13px;
    font-weight: 600;
}

.row-subtitle {
    color: #8b949e;
    font-size: 11px;
}

.status-ok {
    color: #00e676;
    font-weight: 600;
    font-size: 12px;
}

.status-warn {
    color: #f9e2af;
    font-weight: 600;
    font-size: 12px;
}

.status-err {
    color: #f38ba8;
    font-weight: 600;
    font-size: 12px;
}

.badge-connected {
    background-color: rgba(0, 230, 118, 0.16);
    color: #00e676;
    border: 1px solid rgba(0, 230, 118, 0.4);
    border-radius: 4px;
    padding: 2px 8px;
    font-size: 11px;
    font-weight: 700;
}

list.settings-list {
    background-color: #161616;
    border: 1px solid #262626;
    border-radius: 5px;
}

list.settings-list row {
    padding: 8px 12px;
    border-bottom: 1px solid #202020;
    color: #eaeaea;
}

list.settings-list row:last-child {
    border-bottom: none;
}

list.settings-list row:hover {
    background-color: #202020;
}

list.settings-list row:selected {
    background-color: rgba(0, 230, 118, 0.12);
    color: #ffffff;
}

button.accent-btn {
    background-color: #00e676;
    background-image: none;
    color: #0b0b0b;
    border: none;
    border-radius: 4px;
    padding: 6px 14px;
    font-weight: 700;
    font-size: 12px;
}

button.accent-btn:hover {
    background-color: #2ecc71;
}

button.accent-btn:disabled {
    background-color: #2c2c2c;
    color: #666666;
}

button.secondary-btn {
    background-color: #242424;
    background-image: none;
    color: #e6e6e6;
    border: 1px solid #353535;
    border-radius: 4px;
    padding: 6px 12px;
    font-weight: 600;
    font-size: 12px;
}

button.secondary-btn:hover {
    background-color: #2e2e2e;
    border-color: #00e676;
}

button.danger-btn {
    background-color: #2b181c;
    background-image: none;
    color: #f38ba8;
    border: 1px solid #592632;
    border-radius: 4px;
    padding: 6px 12px;
    font-weight: 600;
    font-size: 12px;
}

button.danger-btn:hover {
    background-color: #3a1f25;
}

button.scale-pill {
    background-color: #222222;
    background-image: none;
    color: #cccccc;
    border: 1px solid #333333;
    border-radius: 4px;
    padding: 5px 12px;
    font-weight: 600;
    font-size: 12px;
}

button.scale-pill:checked {
    background-color: rgba(0, 230, 118, 0.18);
    color: #00e676;
    border-color: #00e676;
}

switch {
    background-color: #2d2d2d;
    border: 1px solid #3f3f3f;
    border-radius: 12px;
}

switch:checked {
    background-color: #00e676;
    border-color: #00e676;
}

switch slider {
    background-color: #f5f5f5;
    border-radius: 10px;
}

scale highlight {
    background-color: #00e676;
    border-radius: 3px;
}

scale trough {
    background-color: #2c2c2c;
    border-radius: 3px;
    min-height: 6px;
}

scale slider {
    background-color: #00e676;
    border: 2px solid #111111;
    border-radius: 10px;
    min-width: 16px;
    min-height: 16px;
}

entry, combobox button {
    background-color: #121212;
    background-image: none;
    color: #f0f0f0;
    border: 1px solid #333333;
    border-radius: 4px;
    padding: 5px 8px;
}

entry:focus {
    border-color: #00e676;
}

.settings-sidebar row,
list.settings-list row,
button.accent-btn,
button.secondary-btn,
button.danger-btn,
button.scale-pill {
    transition: all 140ms ease-out;
}

window.no-animations .settings-sidebar row,
window.no-animations list.settings-list row,
window.no-animations button.accent-btn,
window.no-animations button.secondary-btn,
window.no-animations button.danger-btn,
window.no-animations button.scale-pill,
window.no-animations * {
    transition: none;
}
"""


def apply_settings_css():
    """Rejestruje styl CSS OKsavi Linux dla okna OKay Settings."""
    provider = Gtk.CssProvider()
    provider.load_from_data(SETTINGS_CSS.encode("utf-8"))
    screen = Gdk.Screen.get_default()
    if screen is not None:
        Gtk.StyleContext.add_provider_for_screen(
            screen,
            provider,
            Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
        )


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
    "animations": True,
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
    data["animations"] = bool(data.get("animations", True))
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
    def _atomic_swap_dir(staging_dir: str, target_dir: str) -> None:
        parent = os.path.dirname(os.path.abspath(target_dir))
        os.makedirs(parent, exist_ok=True)
        backup_dir = target_dir + ".bak"
        if os.path.exists(backup_dir):
            shutil.rmtree(backup_dir, ignore_errors=True)
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
        if os.path.exists(backup_dir):
            shutil.rmtree(backup_dir, ignore_errors=True)

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
                    shutil.copytree(src, staged, symlinks=False, ignore=shutil.ignore_patterns(".tmp-*", "*.tmp"))
                    cls._atomic_swap_dir(staged, repo_dir)
                finally:
                    shutil.rmtree(stage_parent, ignore_errors=True)
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
                shutil.rmtree(stage_parent, ignore_errors=True)

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

            cls._atomic_swap_dir(src_dir, repo_dir)
        finally:
            shutil.rmtree(stage_parent, ignore_errors=True)

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
