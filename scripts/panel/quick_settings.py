# -*- coding: utf-8 -*-

"""
Quick Settings Popover (okno warstwy GtkLayerShell.Layer.TOP) dla okay-panel.
Styl: Catppuccin Mocha (#1e1e2e) + akcent OKsavi (#00e676).
"""

import re
import os
import time
import shutil
import threading
import subprocess
import gi

gi.require_version("Gtk", "3.0")
gi.require_version("GtkLayerShell", "0.1")
from gi.repository import Gtk, Gdk, GLib, GtkLayerShell

from .animations import apply_animation_class, prepare_fade_in, start_fade_in, stop_fade


def _split_terse(line: str) -> list:
    fields, cur, esc = [], [], False
    for ch in line:
        if esc:
            cur.append(ch)
            esc = False
        elif ch == "\\":
            esc = True
        elif ch == ":":
            fields.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
    fields.append("".join(cur))
    return fields


def _run_cmd(cmd: list, timeout: float = 6.0) -> tuple:
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, check=False)
        return proc.returncode, proc.stdout, proc.stderr
    except Exception as exc:
        return 1, "", str(exc)


def _async(fn, cb=None):
    def _worker():
        res = None
        try:
            res = fn()
        except Exception:
            res = None
        if cb is not None:
            GLib.idle_add(lambda r=res: (cb(r), False)[1])

    threading.Thread(target=_worker, daemon=True).start()


def _sig_icon(signal: int) -> str:
    if signal >= 75:
        return "network-wireless-signal-excellent-symbolic"
    if signal >= 50:
        return "network-wireless-signal-good-symbolic"
    if signal >= 25:
        return "network-wireless-signal-ok-symbolic"
    if signal > 0:
        return "network-wireless-signal-weak-symbolic"
    return "network-wireless-signal-none-symbolic"


class QuickSettingsPopover(Gtk.Window):
    """Lekkie okno szybkich ustawień na warstwie GtkLayerShell.Layer.TOP."""

    def __init__(self, get_panel_pos_cb, on_volume_changed_cb=None, animations_enabled_cb=None):
        super().__init__(type=Gtk.WindowType.TOPLEVEL)
        self.set_name("okay-quick-settings")
        self.set_title("okay-quick-settings")
        self._get_panel_pos = get_panel_pos_cb
        self._on_volume_changed = on_volume_changed_cb
        self._animations_enabled = animations_enabled_cb or (lambda: True)

        self._updating_ui = False
        self._vol_debounce = 0
        self._bri_debounce = 0
        self._pending_pwd_ssid = ""

        GtkLayerShell.init_for_window(self)
        GtkLayerShell.set_layer(self, GtkLayerShell.Layer.TOP)
        GtkLayerShell.set_namespace(self, "okay-panel-popover")
        GtkLayerShell.set_keyboard_mode(self, GtkLayerShell.KeyboardMode.ON_DEMAND)
        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.RIGHT, True)
        GtkLayerShell.set_margin(self, GtkLayerShell.Edge.RIGHT, 8)
        self._sync_anchor()

        self.set_decorated(False)
        self.set_resizable(False)
        self.set_skip_taskbar_hint(True)
        self.set_skip_pager_hint(True)
        self.set_default_size(340, -1)

        screen = self.get_screen()
        visual = screen.get_rgba_visual() if screen is not None else None
        if visual is not None:
            self.set_visual(visual)
            self.set_app_paintable(True)

        self.connect("key-press-event", self._on_key_press)
        self.connect("focus-out-event", self._on_focus_out)
        self.connect("destroy", self._on_destroy)

        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        root.get_style_context().add_class("popover-card")
        self.add(root)

        # Nagłówek
        hdr = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        lbl_hdr = Gtk.Label(label="Szybkie ustawienia", xalign=0.0)
        lbl_hdr.get_style_context().add_class("popover-title")
        hdr.pack_start(lbl_hdr, True, True, 0)

        self.btn_refresh = Gtk.Button()
        self.btn_refresh.get_style_context().add_class("qs-icon-btn")
        self.btn_refresh.set_tooltip_text("Odśwież")
        self.btn_refresh.add(Gtk.Image.new_from_icon_name("view-refresh-symbolic", Gtk.IconSize.MENU))
        self.btn_refresh.connect("clicked", lambda _w: self.refresh_all(rescan_wifi=True))
        hdr.pack_end(self.btn_refresh, False, False, 0)
        root.pack_start(hdr, False, False, 0)

        # Suwak głośności (wpctl)
        vol_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.btn_mute = Gtk.Button()
        self.btn_mute.get_style_context().add_class("qs-icon-btn")
        self.img_mute = Gtk.Image.new_from_icon_name("audio-volume-high-symbolic", Gtk.IconSize.MENU)
        self.btn_mute.add(self.img_mute)
        self.btn_mute.set_tooltip_text("Wycisz / Odcisz")
        self.btn_mute.connect("clicked", self._on_mute_clicked)
        vol_row.pack_start(self.btn_mute, False, False, 0)

        self.vol_scale = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0, 100, 1)
        self.vol_scale.set_digits(0)
        self.vol_scale.set_value_pos(Gtk.PositionType.RIGHT)
        self.vol_scale.connect("value-changed", self._on_vol_scale)
        vol_row.pack_start(self.vol_scale, True, True, 0)
        root.pack_start(vol_row, False, False, 0)

        # Suwak jasności (brightnessctl)
        bri_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        img_bri = Gtk.Image.new_from_icon_name("display-brightness-symbolic", Gtk.IconSize.MENU)
        img_bri.set_margin_start(6)
        img_bri.set_margin_end(6)
        bri_row.pack_start(img_bri, False, False, 0)

        self.bri_scale = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 5, 100, 1)
        self.bri_scale.set_digits(0)
        self.bri_scale.set_value_pos(Gtk.PositionType.RIGHT)
        self.bri_scale.connect("value-changed", self._on_bri_scale)
        bri_row.pack_start(self.bri_scale, True, True, 0)
        root.pack_start(bri_row, False, False, 0)

        # Sekcja Wi-Fi (przełącznik + lista z nmcli)
        wifi_sep = Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL)
        root.pack_start(wifi_sep, False, False, 0)

        wifi_hdr = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        lbl_wifi = Gtk.Label(label="Wi-Fi", xalign=0.0)
        lbl_wifi.get_style_context().add_class("popover-subtitle")
        wifi_hdr.pack_start(lbl_wifi, True, True, 0)

        self.wifi_spinner = Gtk.Spinner()
        self.wifi_spinner.set_no_show_all(True)
        wifi_hdr.pack_start(self.wifi_spinner, False, False, 0)

        self.wifi_switch = Gtk.Switch()
        self.wifi_switch.connect("notify::active", self._on_wifi_toggled)
        wifi_hdr.pack_end(self.wifi_switch, False, False, 0)
        root.pack_start(wifi_hdr, False, False, 0)

        self.wifi_scroll = Gtk.ScrolledWindow()
        self.wifi_scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        self.wifi_scroll.set_min_content_height(140)
        self.wifi_scroll.set_max_content_height(210)

        self.wifi_list = Gtk.ListBox()
        self.wifi_list.set_selection_mode(Gtk.SelectionMode.NONE)
        self.wifi_list.get_style_context().add_class("qs-list")
        self.wifi_scroll.add(self.wifi_list)
        root.pack_start(self.wifi_scroll, True, True, 0)

        # Wbudowany pasek hasła do zabezpieczonych sieci (bez walki o fokus modali na layer-shell)
        self.pwd_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        self.pwd_box.set_no_show_all(True)
        self.pwd_entry = Gtk.Entry()
        self.pwd_entry.set_visibility(False)
        self.pwd_entry.set_placeholder_text("Hasło Wi-Fi...")
        self.pwd_entry.connect("activate", self._on_pwd_submit)
        self.pwd_entry.show()
        self.pwd_box.pack_start(self.pwd_entry, True, True, 0)

        btn_pwd_ok = Gtk.Button(label="Połącz")
        btn_pwd_ok.get_style_context().add_class("qs-accent-btn")
        btn_pwd_ok.connect("clicked", self._on_pwd_submit)
        btn_pwd_ok.show()
        self.pwd_box.pack_end(btn_pwd_ok, False, False, 0)
        root.pack_start(self.pwd_box, False, False, 0)

        self.lbl_qs_status = Gtk.Label(label="", xalign=0.0)
        self.lbl_qs_status.set_line_wrap(True)
        self.lbl_qs_status.get_style_context().add_class("popover-status")
        root.pack_start(self.lbl_qs_status, False, False, 0)

        # Stopka: przycisk otwarcia pełnego okay-settings
        footer = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        btn_settings = Gtk.Button(label="Pełne ustawienia (okay-settings)")
        btn_settings.get_style_context().add_class("qs-accent-btn")
        btn_settings.connect("clicked", self._on_open_settings)
        footer.pack_start(btn_settings, True, True, 0)
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
            self.hide_popover()
        else:
            anim = bool(self._animations_enabled())
            apply_animation_class(self, anim)
            self._sync_anchor()
            self.pwd_box.hide()
            prepare_fade_in(self, anim)
            self.show_all()
            self.present()
            start_fade_in(self, anim)
            self.refresh_all(rescan_wifi=False)

    def hide_popover(self):
        stop_fade(self)
        self.pwd_box.hide()
        self.hide()

    def _on_key_press(self, _w, event):
        if event.keyval == Gdk.KEY_Escape:
            self.hide_popover()
            return True
        return False

    def _on_focus_out(self, _w, _ev):
        GLib.idle_add(lambda: (self.hide_popover() if not self.is_active() else None, False)[1])
        return False

    def _on_destroy(self, _w):
        stop_fade(self, reset_opacity=False)
        for attr in ("_vol_debounce", "_bri_debounce"):
            tid = getattr(self, attr, 0)
            if tid:
                try:
                    GLib.source_remove(tid)
                except Exception:
                    pass
                setattr(self, attr, 0)

    def _set_wifi_busy(self, busy: bool):
        if busy:
            self.wifi_spinner.show()
            self.wifi_spinner.start()
        else:
            self.wifi_spinner.stop()
            self.wifi_spinner.hide()

    def refresh_all(self, rescan_wifi: bool = False):
        self._refresh_audio_and_brightness()
        self._refresh_wifi(rescan=rescan_wifi)

    def _refresh_audio_and_brightness(self):
        def _worker():
            vol_pct, muted, has_audio = 50, False, False
            if shutil.which("wpctl"):
                rc, out, _ = _run_cmd(["wpctl", "get-volume", "@DEFAULT_AUDIO_SINK@"], timeout=2.0)
                if rc == 0:
                    has_audio = True
                    muted = "MUTED" in out.upper()
                    m = re.search(r"Volume:\s*([0-9.]+)", out)
                    if m:
                        vol_pct = max(0, min(100, int(round(float(m.group(1)) * 100))))

            bri_pct, has_bri = 100, False
            if shutil.which("brightnessctl"):
                rc_c, out_c, _ = _run_cmd(["brightnessctl", "get"], timeout=2.0)
                rc_m, out_m, _ = _run_cmd(["brightnessctl", "max"], timeout=2.0)
                if rc_c == 0 and rc_m == 0:
                    try:
                        cur, mx = int(out_c.strip()), int(out_m.strip())
                        if mx > 0:
                            bri_pct = max(5, min(100, int(round(cur * 100 / mx))))
                            has_bri = True
                    except ValueError:
                        pass
            return (has_audio, vol_pct, muted, has_bri, bri_pct)

        def _done(res):
            if not res:
                return
            has_audio, vol_pct, muted, has_bri, bri_pct = res
            self._updating_ui = True
            self.vol_scale.set_sensitive(has_audio)
            self.btn_mute.set_sensitive(has_audio)
            if has_audio:
                self.vol_scale.set_value(vol_pct)
                self.img_mute.set_from_icon_name(
                    "audio-volume-muted-symbolic" if muted else "audio-volume-high-symbolic",
                    Gtk.IconSize.MENU
                )
            self.bri_scale.set_sensitive(has_bri)
            if has_bri:
                self.bri_scale.set_value(bri_pct)
                self.bri_scale.set_tooltip_text("Jasność ekranu (%)")
            else:
                self.bri_scale.set_tooltip_text("Brak obsługiwanego podświetlenia")
            self._updating_ui = False

        _async(_worker, _done)

    def _refresh_wifi(self, rescan: bool = False):
        if not shutil.which("nmcli"):
            self.wifi_switch.set_sensitive(False)
            self._populate_wifi_rows(False, [], "Brak pakietu NetworkManager (nmcli).")
            return

        self._set_wifi_busy(True)

        def _worker():
            rc_r, out_r, _ = _run_cmd(["nmcli", "radio", "wifi"], timeout=4.0)
            enabled = (rc_r == 0 and out_r.strip().lower() == "enabled")

            saved_ssids = {}
            rc_s, out_s, _ = _run_cmd(["nmcli", "-t", "-f", "NAME,TYPE", "con", "show"], timeout=4.0)
            if rc_s == 0:
                for ln in out_s.splitlines():
                    p = _split_terse(ln)
                    if len(p) >= 2 and "wireless" in p[1] and p[0]:
                        saved_ssids[p[0]] = p[0]

            nets = []
            err_msg = ""
            if enabled:
                flag = "yes" if rescan else "auto"
                rc_w, out_w, err_w = _run_cmd(
                    ["nmcli", "-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "dev", "wifi", "list", "--rescan", flag],
                    timeout=10.0
                )
                if rc_w == 0:
                    by_ssid = {}
                    for ln in out_w.splitlines():
                        p = _split_terse(ln)
                        if len(p) < 4:
                            continue
                        in_use = (p[0].strip() == "*")
                        ssid = p[1].strip()
                        if not ssid:
                            continue
                        try:
                            sig = int(p[2].strip() or "0")
                        except ValueError:
                            sig = 0
                        sec = p[3].strip()
                        secured = sec not in ("", "--")
                        prev = by_ssid.get(ssid)
                        if prev is None or in_use or (not prev["in_use"] and sig > prev["signal"]):
                            by_ssid[ssid] = {
                                "ssid": ssid,
                                "in_use": in_use,
                                "signal": sig,
                                "secured": secured,
                                "saved": ssid in saved_ssids,
                            }
                    nets = sorted(by_ssid.values(), key=lambda x: (not x["in_use"], -x["signal"]))[:12]
                else:
                    err_msg = err_w.strip()
            return (rc_r == 0, enabled, nets, err_msg)

        def _done(res):
            self._set_wifi_busy(False)
            if not res:
                return
            ok, enabled, nets, err_msg = res
            self._updating_ui = True
            self.wifi_switch.set_sensitive(ok)
            self.wifi_switch.set_active(enabled)
            self._updating_ui = False
            self._populate_wifi_rows(enabled, nets, err_msg)

        _async(_worker, _done)

    def _populate_wifi_rows(self, enabled: bool, nets: list, msg: str = ""):
        for ch in self.wifi_list.get_children():
            self.wifi_list.remove(ch)
            ch.destroy()

        if not enabled:
            row = Gtk.ListBoxRow()
            lbl = Gtk.Label(label=msg or "Wi-Fi wyłączone")
            lbl.set_margin_top(12)
            lbl.set_margin_bottom(12)
            lbl.get_style_context().add_class("popover-status")
            row.add(lbl)
            self.wifi_list.add(row)
            self.wifi_list.show_all()
            return

        if not nets:
            row = Gtk.ListBoxRow()
            lbl = Gtk.Label(label=msg or "Brak wykrytych sieci Wi-Fi")
            lbl.set_margin_top(12)
            lbl.set_margin_bottom(12)
            lbl.get_style_context().add_class("popover-status")
            row.add(lbl)
            self.wifi_list.add(row)
            self.wifi_list.show_all()
            return

        for n in nets:
            row = Gtk.ListBoxRow()
            hb = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            hb.pack_start(Gtk.Image.new_from_icon_name(_sig_icon(n["signal"]), Gtk.IconSize.MENU), False, False, 0)

            lbl = Gtk.Label(label=n["ssid"], xalign=0.0)
            lbl.get_style_context().add_class("qs-ssid-active" if n["in_use"] else "qs-ssid")
            hb.pack_start(lbl, True, True, 0)

            if n["secured"]:
                hb.pack_start(
                    Gtk.Image.new_from_icon_name("network-wireless-encrypted-symbolic", Gtk.IconSize.MENU),
                    False, False, 0
                )

            btn = Gtk.Button(label="Rozłącz" if n["in_use"] else "Połącz")
            btn.get_style_context().add_class("qs-mini-btn-active" if n["in_use"] else "qs-mini-btn")
            btn.connect("clicked", lambda _b, net=n: self._on_wifi_action(net))
            hb.pack_end(btn, False, False, 0)

            row.add(hb)
            self.wifi_list.add(row)

        self.wifi_list.show_all()

    def _on_wifi_action(self, net: dict):
        ssid = net["ssid"]
        if net["in_use"]:
            self.lbl_qs_status.set_text(f"Rozłączanie {ssid}...")
            _async(
                lambda: _run_cmd(["nmcli", "con", "down", "id", ssid], timeout=8.0),
                lambda _r: (self.lbl_qs_status.set_text(""), self._refresh_wifi(False))
            )
            return

        if not net["secured"] or net["saved"]:
            self.pwd_box.hide()
            self.lbl_qs_status.set_text(f"Łączenie z {ssid}...")

            def _connect_saved():
                if net["saved"]:
                    rc, out, err = _run_cmd(["nmcli", "con", "up", "id", ssid], timeout=15.0)
                    if rc == 0:
                        return rc, out, err
                return _run_cmd(["nmcli", "dev", "wifi", "connect", ssid], timeout=15.0)

            def _done(res):
                rc, _o, err = res or (1, "", "błąd")
                if rc == 0:
                    self.lbl_qs_status.set_text(f"Połączono z {ssid}.")
                elif net["secured"]:
                    self._show_pwd_prompt(ssid)
                else:
                    self.lbl_qs_status.set_text(f"Błąd: {err.strip()}")
                self._refresh_wifi(False)

            _async(_connect_saved, _done)
        else:
            self._show_pwd_prompt(ssid)

    def _show_pwd_prompt(self, ssid: str):
        self._pending_pwd_ssid = ssid
        self.pwd_entry.set_text("")
        self.pwd_entry.set_placeholder_text(f"Hasło dla {ssid}...")
        self.pwd_box.show()
        self.pwd_entry.grab_focus()
        self.lbl_qs_status.set_text(f"Podaj hasło dla zabezpieczonej sieci {ssid}.")

    def _on_pwd_submit(self, _w):
        ssid = self._pending_pwd_ssid
        pwd = self.pwd_entry.get_text()
        if not ssid or not pwd:
            return
        self.pwd_box.hide()
        self.lbl_qs_status.set_text(f"Łączenie z {ssid}...")

        def _done(res):
            rc, _o, err = res or (1, "", "błąd")
            self.lbl_qs_status.set_text(f"Połączono z {ssid}." if rc == 0 else f"Błąd: {err.strip()}")
            self._refresh_wifi(False)

        _async(lambda: _run_cmd(["nmcli", "dev", "wifi", "connect", ssid, "password", pwd], timeout=18.0), _done)

    def _on_wifi_toggled(self, switch, _pspec):
        if self._updating_ui or not shutil.which("nmcli"):
            return
        on = switch.get_active()
        self._set_wifi_busy(True)

        def _w():
            _run_cmd(["nmcli", "radio", "wifi", "on" if on else "off"], timeout=5.0)
            if on:
                time.sleep(0.8)

        _async(_w, lambda _r: self._refresh_wifi(rescan=on))

    def _on_vol_scale(self, scale):
        if self._updating_ui:
            return
        pct = int(round(scale.get_value()))
        if self._vol_debounce:
            GLib.source_remove(self._vol_debounce)
        self._vol_debounce = GLib.timeout_add(50, self._apply_vol, pct)

    def _apply_vol(self, pct: int) -> bool:
        self._vol_debounce = 0
        def _w():
            _run_cmd(["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", f"{pct}%"], timeout=2.0)
            if self._on_volume_changed:
                self._on_volume_changed()
        _async(_w)
        return False

    def _on_mute_clicked(self, _btn):
        def _w():
            _run_cmd(["wpctl", "set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"], timeout=2.0)
            if self._on_volume_changed:
                self._on_volume_changed()
        _async(_w, lambda _r: self._refresh_audio_and_brightness())

    def _on_bri_scale(self, scale):
        if self._updating_ui:
            return
        pct = int(round(scale.get_value()))
        if self._bri_debounce:
            GLib.source_remove(self._bri_debounce)
        self._bri_debounce = GLib.timeout_add(50, self._apply_bri, pct)

    def _apply_bri(self, pct: int) -> bool:
        self._bri_debounce = 0
        _async(lambda: _run_cmd(["brightnessctl", "set", f"{pct}%", "-q"], timeout=2.0))
        return False

    def _on_open_settings(self, _btn):
        self.hide_popover()
        try:
            local_bin = os.path.expanduser("~/.local/bin/okay-settings")
            exe = local_bin if os.path.isfile(local_bin) and os.access(local_bin, os.X_OK) else "okay-settings"
            subprocess.Popen([exe], start_new_session=True)
        except Exception:
            pass
