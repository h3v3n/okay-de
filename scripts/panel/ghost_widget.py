# -*- coding: utf-8 -*-

"""
Widżet oksavi-ghost-runner (cgroups v2 freezer) dla okay-panel.
Wyświetla liczbę aktualnie zamrożonych aplikacji/slice'ów i umożliwia ich odmrożenie.
"""

import os
import re
import json
import glob
import signal
import shutil
import threading
import subprocess
import gi

gi.require_version("Gtk", "3.0")
gi.require_version("GtkLayerShell", "0.1")
from gi.repository import Gtk, Gdk, GLib, GtkLayerShell


STATE_FILE = os.path.join(os.environ.get("XDG_RUNTIME_DIR") or "/tmp", "oksavi-ghost-runner-state.json")


def _find_user_app_slice() -> str:
    uid = os.getuid() if hasattr(os, "getuid") else 1000
    try:
        with open("/proc/self/cgroup", "r", encoding="utf-8") as f:
            for line in f:
                parts = line.strip().split(":", 2)
                if len(parts) == 3 and parts[0] == "0":
                    marker = f"/user@{uid}.service"
                    idx = parts[2].find(marker)
                    if idx != -1:
                        base = parts[2][: idx + len(marker)] + "/app.slice"
                        cg = "/sys/fs/cgroup" + base
                        if os.path.isdir(cg):
                            return cg
    except Exception:
        pass
    fb = f"/sys/fs/cgroup/user.slice/user-{uid}.slice/user@{uid}.service/app.slice"
    return fb if os.path.isdir(fb) else ""


def _friendly_scope_name(raw_name: str) -> str:
    name = raw_name
    for prefix in ("app-gnome-", "app-flatpak-", "app-"):
        if name.startswith(prefix):
            name = name[len(prefix):]
            break
    name = re.sub(r"-\d+\.scope$", "", name)
    name = re.sub(r"\.(scope|service)$", "", name)
    name = name.replace("\\x2d", "-")
    if "." in name and len(name) > 22:
        name = name.split(".")[-1]
    return name or raw_name


def scan_frozen_apps() -> list:
    """Skanuje zamrożone cgroups v2 (cgroup.freeze == 1) oraz plik stanu oksavi-ghost-runner."""
    found = {}

    app_slice = _find_user_app_slice()
    if app_slice:
        patterns = (
            os.path.join(app_slice, "*", "cgroup.freeze"),
            os.path.join(app_slice, "*", "*", "cgroup.freeze"),
        )
        for pat in patterns:
            for fpath in glob.iglob(pat):
                try:
                    with open(fpath, "r", encoding="utf-8") as f:
                        if f.read().strip() != "1":
                            continue
                    cdir = os.path.dirname(fpath)
                    pids = 0
                    procs_file = os.path.join(cdir, "cgroup.procs")
                    if os.path.isfile(procs_file):
                        with open(procs_file, "r", encoding="utf-8") as pf:
                            pids = sum(1 for ln in pf if ln.strip())
                    raw = os.path.basename(cdir)
                    found[cdir] = {
                        "name": _friendly_scope_name(raw),
                        "raw": raw,
                        "cgroup": cdir,
                        "pids": pids,
                    }
                except Exception:
                    continue

    # Scal wpisy z pliku stanu daemona (jeśli jeszcze aktywne i zamrożone)
    if os.path.isfile(STATE_FILE):
        try:
            with open(STATE_FILE, "r", encoding="utf-8") as f:
                data = json.load(f)
            for item in data.get("frozen", []):
                cdir = item.get("cgroup", "")
                if not cdir or cdir in found:
                    continue
                freeze_file = os.path.join(cdir, "cgroup.freeze")
                if os.path.isfile(freeze_file):
                    try:
                        with open(freeze_file, "r", encoding="utf-8") as ff:
                            if ff.read().strip() != "1":
                                continue
                    except Exception:
                        continue
                found[cdir] = {
                    "name": _friendly_scope_name(item.get("name") or os.path.basename(cdir)),
                    "raw": item.get("name") or os.path.basename(cdir),
                    "cgroup": cdir,
                    "pids": int(item.get("pids", 0) or 0),
                }
        except Exception:
            pass

    return sorted(found.values(), key=lambda x: x["name"].lower())


def _thaw_single_cgroup(cgroup_path: str):
    runner = shutil.which("oksavi-ghost-runner")
    if runner:
        subprocess.run([runner, "thaw_app", cgroup_path], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    freeze_file = os.path.join(cgroup_path, "cgroup.freeze")
    if os.path.isfile(freeze_file):
        try:
            with open(freeze_file, "w", encoding="utf-8") as f:
                f.write("0")
        except Exception:
            pass
    procs_file = os.path.join(cgroup_path, "cgroup.procs")
    if os.path.isfile(procs_file):
        try:
            with open(procs_file, "r", encoding="utf-8") as f:
                for ln in f:
                    ln = ln.strip()
                    if ln.isdigit():
                        try:
                            os.kill(int(ln), signal.SIGCONT)
                        except Exception:
                            pass
        except Exception:
            pass


def _thaw_all_cgroups():
    runner = shutil.which("oksavi-ghost-runner")
    if runner:
        subprocess.run([runner, "thaw_all"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for item in scan_frozen_apps():
        _thaw_single_cgroup(item["cgroup"])


class GhostRunnerPopover(Gtk.Window):
    """Okno listy zamrożonych aplikacji oksavi-ghost-runner (Layer-Shell TOP)."""

    def __init__(self, get_panel_pos_cb, on_changed_cb=None):
        super().__init__(type=Gtk.WindowType.TOPLEVEL)
        self.set_name("okay-ghost-popover")
        self.set_title("okay-ghost-popover")
        self._get_panel_pos = get_panel_pos_cb
        self._on_changed = on_changed_cb

        GtkLayerShell.init_for_window(self)
        GtkLayerShell.set_layer(self, GtkLayerShell.Layer.TOP)
        GtkLayerShell.set_namespace(self, "okay-ghost-popover")
        GtkLayerShell.set_keyboard_mode(self, GtkLayerShell.KeyboardMode.ON_DEMAND)
        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.RIGHT, True)
        GtkLayerShell.set_margin(self, GtkLayerShell.Edge.RIGHT, 8)
        self._sync_anchor()

        self.set_decorated(False)
        self.set_resizable(False)
        self.set_skip_taskbar_hint(True)
        self.set_skip_pager_hint(True)
        self.set_default_size(320, -1)

        self.connect("key-press-event", self._on_key_press)
        self.connect("focus-out-event", self._on_focus_out)

        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        root.get_style_context().add_class("popover-card")
        self.add(root)

        hdr = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        lbl = Gtk.Label(label="OKsavi Ghost Runner", xalign=0.0)
        lbl.get_style_context().add_class("popover-title")
        hdr.pack_start(lbl, True, True, 0)

        self.lbl_count = Gtk.Label(label="0 zamrożonych")
        self.lbl_count.get_style_context().add_class("popover-status")
        hdr.pack_end(self.lbl_count, False, False, 0)
        root.pack_start(hdr, False, False, 0)

        sw = Gtk.ScrolledWindow()
        sw.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        sw.set_min_content_height(100)
        sw.set_max_content_height(210)

        self.listbox = Gtk.ListBox()
        self.listbox.set_selection_mode(Gtk.SelectionMode.NONE)
        self.listbox.get_style_context().add_class("qs-list")
        sw.add(self.listbox)
        root.pack_start(sw, True, True, 0)

        footer = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.btn_thaw_all = Gtk.Button(label="Odmroź wszystko")
        self.btn_thaw_all.get_style_context().add_class("qs-accent-btn")
        self.btn_thaw_all.connect("clicked", self._on_thaw_all_clicked)
        footer.pack_start(self.btn_thaw_all, True, True, 0)
        root.pack_start(footer, False, False, 0)

    def _sync_anchor(self):
        pos = self._get_panel_pos()
        is_top = (pos == "top")
        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.TOP, is_top)
        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.BOTTOM, not is_top)
        GtkLayerShell.set_margin(self, GtkLayerShell.Edge.TOP, 6 if is_top else 0)
        GtkLayerShell.set_margin(self, GtkLayerShell.Edge.BOTTOM, 0 if is_top else 6)

    def toggle(self):
        if self.get_visible():
            self.hide()
        else:
            self._sync_anchor()
            self.refresh()
            self.show_all()
            self.present()

    def _on_key_press(self, _w, event):
        if event.keyval == Gdk.KEY_Escape:
            self.hide()
            return True
        return False

    def _on_focus_out(self, _w, _ev):
        GLib.idle_add(lambda: (self.hide() if not self.is_active() else None, False)[1])
        return False

    def refresh(self):
        items = scan_frozen_apps()
        for ch in self.listbox.get_children():
            self.listbox.remove(ch)
            ch.destroy()

        self.lbl_count.set_text(f"Zamrożonych: {len(items)}")
        self.btn_thaw_all.set_sensitive(len(items) > 0)

        if not items:
            row = Gtk.ListBoxRow()
            lbl = Gtk.Label(label="Brak aktualnie zamrożonych procesów / slice'ów.")
            lbl.set_line_wrap(True)
            lbl.set_margin_top(12)
            lbl.set_margin_bottom(12)
            lbl.get_style_context().add_class("popover-status")
            row.add(lbl)
            self.listbox.add(row)
        else:
            for item in items:
                row = Gtk.ListBoxRow()
                hb = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
                lbl_n = Gtk.Label(label=f"{item['name']} ({item['pids']} PID)", xalign=0.0)
                lbl_n.get_style_context().add_class("qs-ssid")
                hb.pack_start(lbl_n, True, True, 0)

                btn = Gtk.Button(label="Odmroź")
                btn.get_style_context().add_class("qs-mini-btn")
                btn.connect("clicked", lambda _b, cg=item["cgroup"]: self._on_thaw_one(cg))
                hb.pack_end(btn, False, False, 0)
                row.add(hb)
                self.listbox.add(row)

        self.listbox.show_all()

    def _on_thaw_one(self, cgroup_path: str):
        def _w():
            _thaw_single_cgroup(cgroup_path)
            GLib.idle_add(lambda: (self.refresh(), self._on_changed() if self._on_changed else None, False)[2])

        threading.Thread(target=_w, daemon=True).start()

    def _on_thaw_all_clicked(self, _btn):
        def _w():
            _thaw_all_cgroups()
            GLib.idle_add(lambda: (self.refresh(), self._on_changed() if self._on_changed else None, False)[2])

        threading.Thread(target=_w, daemon=True).start()


class GhostRunnerIndicator(Gtk.Button):
    """Kompaktowy wskaźnik stanu zamrożonych aplikacji z oksavi-ghost-runner w pasku panelu."""

    def __init__(self, get_panel_pos_cb, on_open_cb=None):
        super().__init__()
        self.get_style_context().add_class("status-item")
        self.get_style_context().add_class("ghost-pill")
        self._last_count = -1
        self._on_open = on_open_cb
        self._popover = GhostRunnerPopover(get_panel_pos_cb, on_changed_cb=self.update_state)

        box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
        self.lbl_icon = Gtk.Label(label="❄")
        self.lbl_count = Gtk.Label(label="0")
        box.pack_start(self.lbl_icon, False, False, 0)
        box.pack_start(self.lbl_count, False, False, 0)
        self.add(box)

        self.connect("clicked", self._on_indicator_clicked)
        self.update_state()
        GLib.timeout_add_seconds(8, self.update_state)

    def _on_indicator_clicked(self, _w):
        if self._on_open:
            self._on_open()
        self._popover.toggle()

    def update_state(self) -> bool:
        items = scan_frozen_apps()
        cnt = len(items)
        if cnt != self._last_count:
            self._last_count = cnt
            self.lbl_count.set_text(str(cnt))
            ctx = self.get_style_context()
            if cnt > 0:
                ctx.add_class("ghost-active")
                self.set_tooltip_text(f"Ghost Runner: {cnt} zamrożonych aplikacji (kliknij, aby odmrozić)")
            else:
                ctx.remove_class("ghost-active")
                self.set_tooltip_text("Ghost Runner: 0 zamrożonych aplikacji")
        return True
