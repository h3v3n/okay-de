# -*- coding: utf-8 -*-

"""
Wspólne narzędzia GUI (GTK3), stylistyka OKsavi oraz asynchroniczne wykonywanie poleceń
dla modułów centrum sterowania OKay Settings.
"""

import os
import sys
import glob
import json
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
                        if "model name" in line or "Hardware" in line:
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

    @staticmethod
    def get_zram_status() -> str:
        zram_disksize = "/sys/block/zram0/disksize"
        if os.path.exists(zram_disksize):
            try:
                with open(zram_disksize, "r", encoding="utf-8") as f:
                    size_bytes = int(f.read().strip())
                if size_bytes > 0:
                    return f"Aktywny ({size_bytes / (1024 * 1024):.0f} MiB)"
            except Exception:
                pass
        return "Brak aktywnego zram0"


class UpdateManager:
    """Zarządza sprawdzaniem i instalacją aktualizacji OKay DE z GitHub."""

    GITHUB_REPO = "h3v3n/okay-de"
    API_COMMITS_URL = "https://api.github.com/repos/h3v3n/okay-de/commits/main"
    TARBALL_URL = "https://github.com/h3v3n/okay-de/archive/refs/heads/main.tar.gz"
    LOCAL_VERSION_FILE = os.path.expanduser("~/.config/okay-de/version")
    SYSTEM_VERSION_FILE = "/etc/okay-de/version"

    @classmethod
    def get_local_commit(cls) -> str:
        for v_file in (cls.LOCAL_VERSION_FILE, cls.SYSTEM_VERSION_FILE):
            if os.path.exists(v_file):
                try:
                    with open(v_file, "r", encoding="utf-8") as f:
                        val = f.read().strip()
                        if val:
                            return val[:7]
                except Exception:
                    pass
        return "nieznana"

    @classmethod
    def check_remote_commit(cls) -> dict:
        req = urllib.request.Request(
            cls.API_COMMITS_URL,
            headers={
                "User-Agent": "OKay-Settings-Updater/1.0",
                "Accept": "application/vnd.github.v3+json"
            }
        )
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read().decode("utf-8"))
        sha = data.get("sha", "")[:7]
        commit = data.get("commit", {})
        msg = (commit.get("message", "") or "").splitlines()[0]
        date = commit.get("committer", {}).get("date", "")[:10]
        return {"sha": sha, "message": msg, "date": date}

    @classmethod
    def download_and_install(cls, expected_sha: str = "") -> str:
        with tempfile.TemporaryDirectory(prefix="okay-de-upd-") as tmpdir:
            tar_path = os.path.join(tmpdir, "main.tar.gz")
            req = urllib.request.Request(cls.TARBALL_URL, headers={"User-Agent": "OKay-Settings-Updater/1.0"})
            with urllib.request.urlopen(req, timeout=30) as resp, open(tar_path, "wb") as out:
                shutil.copyfileobj(resp, out)

            with tarfile.open(tar_path, "r:gz") as tf:
                tf.extractall(tmpdir)

            src_dir = None
            for entry in os.listdir(tmpdir):
                full = os.path.join(tmpdir, entry)
                if os.path.isdir(full) and os.path.exists(os.path.join(full, "install.sh")):
                    src_dir = full
                    break

            if not src_dir:
                raise RuntimeError("Nie znaleziono skryptu install.sh w paczce aktualizacji.")

            proc = subprocess.run(
                ["bash", "install.sh", "--user"],
                cwd=src_dir,
                capture_output=True,
                text=True,
                timeout=60,
                check=False
            )
            if proc.returncode != 0:
                raise RuntimeError(proc.stderr or proc.stdout or "Błąd instalatora install.sh")

            if expected_sha:
                os.makedirs(os.path.dirname(cls.LOCAL_VERSION_FILE), exist_ok=True)
                with open(cls.LOCAL_VERSION_FILE, "w", encoding="utf-8") as f:
                    f.write(expected_sha[:7] + "\n")

            return proc.stdout
