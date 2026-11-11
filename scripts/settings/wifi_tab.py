# -*- coding: utf-8 -*-

"""
Zakładka „Wi-Fi i Sieć” (NetworkManager / nmcli) w GTK3.
Wszystkie operacje skanowania i łączenia wykonuje asynchronicznie w wątku roboczym.
"""

import time
import shutil
from gi.repository import Gtk

from .helpers import (
    run_async,
    run_cmd,
    make_scrolled_page,
    make_page_header,
    make_card,
    make_setting_row,
)


def _split_nmcli_terse(line: str) -> list:
    """Dzieli wiersz wyjścia `nmcli -t` z zachowaniem eskejpowanych dwukropków (\\:)."""
    fields = []
    cur = []
    escaped = False
    for ch in line:
        if escaped:
            cur.append(ch)
            escaped = False
        elif ch == "\\":
            escaped = True
        elif ch == ":":
            fields.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
    fields.append("".join(cur))
    return fields


def _signal_icon_name(signal: int) -> str:
    if signal >= 75:
        return "network-wireless-signal-excellent-symbolic"
    if signal >= 50:
        return "network-wireless-signal-good-symbolic"
    if signal >= 25:
        return "network-wireless-signal-ok-symbolic"
    if signal > 0:
        return "network-wireless-signal-weak-symbolic"
    return "network-wireless-signal-none-symbolic"


class WifiTab(Gtk.Box):
    """Widok zarządzania Wi-Fi i połączeniami sieciowymi przez NetworkManager (nmcli)."""

    def __init__(self, parent_window=None):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.parent_window = parent_window
        self._updating_ui = False
        self._busy = False
        self._networks = []
        self._saved_profiles = {}

        sw, self.content = make_scrolled_page()
        self.pack_start(sw, True, True, 0)

        # Nagłówek + spinner + przycisk skanowania
        right_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.spinner = Gtk.Spinner()
        self.spinner.set_no_show_all(True)
        right_box.pack_start(self.spinner, False, False, 0)

        self.btn_scan = Gtk.Button(label="Skanuj sieci")
        self.btn_scan.get_style_context().add_class("secondary-btn")
        self.btn_scan.connect("clicked", lambda _w: self.refresh_state(rescan=True))
        right_box.pack_start(self.btn_scan, False, False, 0)

        self.content.pack_start(
            make_page_header(
                "Wi-Fi i Sieć",
                "Zarządzanie kartą bezprzewodową oraz połączeniami NetworkManager.",
                right_box
            ),
            False, False, 0
        )

        # Karta stanu radia Wi-Fi i połączeń przewodowych
        card_status, box_status = make_card("Adapter sieciowy")
        self.wifi_switch = Gtk.Switch()
        self.wifi_switch.connect("notify::active", self._on_wifi_switch_toggled)
        box_status.pack_start(
            make_setting_row(
                "Sieć bezprzewodowa Wi-Fi",
                "Włącza lub wyłącza moduł radiowy Wi-Fi",
                self.wifi_switch
            ),
            False, False, 0
        )

        self.lbl_active_net = Gtk.Label(label="Sprawdzanie połączeń...", xalign=0.0)
        self.lbl_active_net.get_style_context().add_class("row-subtitle")
        box_status.pack_start(self.lbl_active_net, False, False, 0)
        self.content.pack_start(card_status, False, False, 0)

        # Karta listy dostępnych sieci
        card_list, box_list = make_card("Dostępne sieci Wi-Fi")

        self.listbox = Gtk.ListBox()
        self.listbox.set_selection_mode(Gtk.SelectionMode.SINGLE)
        self.listbox.get_style_context().add_class("settings-list")
        self.listbox.connect("row-selected", self._on_row_selected)
        self.listbox.connect("row-activated", self._on_row_activated)
        box_list.pack_start(self.listbox, True, True, 0)

        # Pasek akcji na dole listy („Połącz / Rozłącz”)
        action_bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.lbl_status = Gtk.Label(label="", xalign=0.0)
        self.lbl_status.set_line_wrap(True)
        self.lbl_status.get_style_context().add_class("row-subtitle")
        action_bar.pack_start(self.lbl_status, True, True, 0)

        self.btn_connect_toggle = Gtk.Button(label="Połącz")
        self.btn_connect_toggle.get_style_context().add_class("accent-btn")
        self.btn_connect_toggle.set_sensitive(False)
        self.btn_connect_toggle.connect("clicked", self._on_connect_toggle_clicked)
        action_bar.pack_end(self.btn_connect_toggle, False, False, 0)

        box_list.pack_start(action_bar, False, False, 0)
        self.content.pack_start(card_list, True, True, 0)

        self.refresh_state(rescan=False)

    def _set_busy(self, busy: bool, message: str = ""):
        self._busy = busy
        self.btn_scan.set_sensitive(not busy)
        if busy:
            self.spinner.show()
            self.spinner.start()
        else:
            self.spinner.stop()
            self.spinner.hide()
        if message:
            self.lbl_status.set_text(message)

    def refresh_state(self, rescan: bool = False):
        """Pobiera asynchronicznie stan radia Wi-Fi, aktywne połączenia oraz listę sieci."""
        if not shutil.which("nmcli"):
            self.lbl_active_net.set_text("Brak pakietu NetworkManager (nmcli) w systemie.")
            self.wifi_switch.set_sensitive(False)
            self.btn_scan.set_sensitive(False)
            return

        self._set_busy(True, "Skanowanie sieci Wi-Fi..." if rescan else "Odświeżanie listy sieci...")

        def _worker():
            rc_radio, out_radio, err_radio = run_cmd(["nmcli", "radio", "wifi"], timeout=5)
            wifi_enabled = (rc_radio == 0 and out_radio.strip().lower() == "enabled")

            rc_act, out_act, _ = run_cmd(["nmcli", "-t", "-f", "NAME,TYPE,DEVICE", "con", "show", "--active"], timeout=5)
            active_conns = []
            if rc_act == 0:
                for ln in out_act.splitlines():
                    parts = _split_nmcli_terse(ln)
                    if len(parts) >= 3 and parts[0]:
                        active_conns.append((parts[0], parts[1], parts[2]))

            rc_saved, out_saved, _ = run_cmd(["nmcli", "-t", "-f", "NAME,TYPE", "con", "show"], timeout=5)
            saved_wifi = {}  # ssid -> profile_name
            if rc_saved == 0:
                for ln in out_saved.splitlines():
                    parts = _split_nmcli_terse(ln)
                    if len(parts) >= 2 and "wireless" in parts[1]:
                        prof_name = parts[0]
                        saved_wifi[prof_name] = prof_name
                        rc_ssid, out_ssid, _ = run_cmd(
                            ["nmcli", "-t", "-g", "802-11-wireless.ssid", "con", "show", "id", prof_name],
                            timeout=3
                        )
                        if rc_ssid == 0 and out_ssid.strip():
                            saved_wifi[out_ssid.strip()] = prof_name

            networks = []
            scan_err = ""
            if wifi_enabled:
                rescan_flag = "yes" if rescan else "auto"
                rc_wifi, out_wifi, err_wifi = run_cmd([
                    "nmcli", "-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY,BARS",
                    "dev", "wifi", "list", "--rescan", rescan_flag
                ], timeout=12)
                if rc_wifi == 0:
                    by_ssid = {}
                    for ln in out_wifi.splitlines():
                        parts = _split_nmcli_terse(ln)
                        if len(parts) < 4:
                            continue
                        in_use = (parts[0].strip() == "*")
                        ssid = parts[1].strip()
                        if not ssid:
                            continue
                        try:
                            signal = int(parts[2].strip() or "0")
                        except ValueError:
                            signal = 0
                        sec = parts[3].strip()
                        if sec in ("--", ""):
                            sec = "Otwarta"

                        prev = by_ssid.get(ssid)
                        if prev is None or in_use or (not prev["in_use"] and signal > prev["signal"]):
                            by_ssid[ssid] = {
                                "in_use": in_use,
                                "ssid": ssid,
                                "signal": signal,
                                "security": sec,
                                "secured": (sec != "Otwarta"),
                            }
                    networks = sorted(
                        by_ssid.values(),
                        key=lambda n: (not n["in_use"], -n["signal"], n["ssid"].lower())
                    )
                else:
                    scan_err = err_wifi.strip() or err_radio.strip()

            return {
                "rc_radio": rc_radio,
                "err_radio": err_radio.strip(),
                "wifi_enabled": wifi_enabled,
                "active_conns": active_conns,
                "saved_wifi": saved_wifi,
                "networks": networks,
                "scan_err": scan_err,
            }

        def _on_done(res: dict):
            self._set_busy(False)
            self._saved_profiles = res["saved_wifi"]
            self._networks = res["networks"]

            self._updating_ui = True
            self.wifi_switch.set_sensitive(res["rc_radio"] == 0)
            self.wifi_switch.set_active(res["wifi_enabled"])
            self._updating_ui = False

            if res["active_conns"]:
                summary = ", ".join(f"{name} ({dev})" for name, _t, dev in res["active_conns"] if dev)
                self.lbl_active_net.set_text(f"Aktywne połączenia: {summary or 'połączono'}")
            else:
                self.lbl_active_net.set_text("Brak aktywnych połączeń sieciowych.")

            self._populate_networks(res["wifi_enabled"], res["scan_err"] or res["err_radio"])

        def _on_err(exc):
            self._set_busy(False, f"Błąd NetworkManager: {exc}")

        run_async(_worker, _on_done, _on_err)

    def _populate_networks(self, wifi_enabled: bool, err_msg: str = ""):
        for child in self.listbox.get_children():
            self.listbox.remove(child)

        if not wifi_enabled:
            row = Gtk.ListBoxRow()
            row.set_selectable(False)
            lbl = Gtk.Label(label="Wi-Fi jest wyłączone. Użyj przełącznika powyżej, aby włączyć kartę sieciową.")
            lbl.set_margin_top(14)
            lbl.set_margin_bottom(14)
            lbl.get_style_context().add_class("row-subtitle")
            row.add(lbl)
            self.listbox.add(row)
            self.listbox.show_all()
            self.btn_connect_toggle.set_sensitive(False)
            self.lbl_status.set_text(err_msg or "Moduł Wi-Fi jest wyłączony.")
            return

        if not self._networks:
            row = Gtk.ListBoxRow()
            row.set_selectable(False)
            lbl = Gtk.Label(label=err_msg or "Nie znaleziono sieci Wi-Fi w zasięgu.")
            lbl.set_margin_top(14)
            lbl.set_margin_bottom(14)
            lbl.get_style_context().add_class("row-subtitle")
            row.add(lbl)
            self.listbox.add(row)
            self.listbox.show_all()
            self.btn_connect_toggle.set_sensitive(False)
            self.lbl_status.set_text(err_msg or "Brak wykrytych sieci.")
            return

        for net in self._networks:
            row = Gtk.ListBoxRow()
            row._wifi_net = net

            hbox = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)

            sig_icon = Gtk.Image.new_from_icon_name(_signal_icon_name(net["signal"]), Gtk.IconSize.MENU)
            hbox.pack_start(sig_icon, False, False, 0)

            info_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=1)
            lbl_ssid = Gtk.Label(label=net["ssid"], xalign=0.0)
            lbl_ssid.get_style_context().add_class("row-title")
            lbl_meta = Gtk.Label(label=f"{net['security']} • Zasięg: {net['signal']}%", xalign=0.0)
            lbl_meta.get_style_context().add_class("row-subtitle")
            info_box.pack_start(lbl_ssid, False, False, 0)
            info_box.pack_start(lbl_meta, False, False, 0)
            hbox.pack_start(info_box, True, True, 0)

            if net["secured"]:
                lock_icon = Gtk.Image.new_from_icon_name("network-wireless-encrypted-symbolic", Gtk.IconSize.MENU)
                lock_icon.set_tooltip_text(f"Sieć zabezpieczona ({net['security']})")
                hbox.pack_start(lock_icon, False, False, 2)

            if net["in_use"]:
                badge = Gtk.Label(label="Połączono")
                badge.get_style_context().add_class("badge-connected")
                hbox.pack_start(badge, False, False, 2)

            btn_row = Gtk.Button(label="Rozłącz" if net["in_use"] else "Połącz")
            btn_row.get_style_context().add_class("danger-btn" if net["in_use"] else "secondary-btn")
            btn_row.connect("clicked", lambda _b, n=net: self._handle_network_action(n))
            hbox.pack_end(btn_row, False, False, 0)

            row.add(hbox)
            self.listbox.add(row)

        self.listbox.show_all()
        self.lbl_status.set_text(f"Znaleziono sieci: {len(self._networks)}")
        self._update_bottom_action_button()

    def _on_row_selected(self, _lb, _row):
        self._update_bottom_action_button()

    def _on_row_activated(self, _lb, row):
        net = getattr(row, "_wifi_net", None)
        if net:
            self._handle_network_action(net)

    def _update_bottom_action_button(self):
        row = self.listbox.get_selected_row()
        net = getattr(row, "_wifi_net", None) if row else None
        if not net:
            self.btn_connect_toggle.set_sensitive(False)
            self.btn_connect_toggle.set_label("Połącz")
            return
        self.btn_connect_toggle.set_sensitive(not self._busy)
        self.btn_connect_toggle.set_label("Rozłącz" if net["in_use"] else "Połącz")

    def _on_connect_toggle_clicked(self, _btn):
        row = self.listbox.get_selected_row()
        net = getattr(row, "_wifi_net", None) if row else None
        if net:
            self._handle_network_action(net)

    def _on_wifi_switch_toggled(self, switch, _pspec):
        if self._updating_ui or not shutil.which("nmcli"):
            return
        enabled = switch.get_active()
        self._set_busy(True, "Włączanie Wi-Fi..." if enabled else "Wyłączanie Wi-Fi...")

        def _worker():
            res = run_cmd(["nmcli", "radio", "wifi", "on" if enabled else "off"], timeout=6)
            if enabled:
                time.sleep(0.8)
            return res

        def _done(_res):
            self.refresh_state(rescan=enabled)

        run_async(_worker, _done, lambda e: self._set_busy(False, f"Błąd przełączania Wi-Fi: {e}"))

    def _handle_network_action(self, net: dict):
        if self._busy:
            return
        ssid = net["ssid"]
        if net["in_use"]:
            self._disconnect_ssid(ssid)
            return

        # Jeśli sieć ma zapisany profil, spróbuj połączyć bezpośrednio; w przeciwnym razie zapytaj o hasło
        if not net["secured"] or ssid in self._saved_profiles:
            self._connect_ssid(ssid, password=None, retry_prompt_if_auth_fail=net["secured"])
        else:
            pwd = self._ask_password(ssid, net["security"])
            if pwd is not None:
                self._connect_ssid(ssid, password=pwd, retry_prompt_if_auth_fail=False)

    def _disconnect_ssid(self, ssid: str):
        self._set_busy(True, f"Rozłączanie sieci {ssid}...")

        def _worker():
            rc, out, err = run_cmd(["nmcli", "con", "down", "id", ssid], timeout=8)
            if rc != 0:
                # Fallback po aktywnym interfejsie wifi
                rc2, devs, _ = run_cmd(["nmcli", "-t", "-f", "DEVICE,TYPE,STATE", "dev"], timeout=5)
                if rc2 == 0:
                    for ln in devs.splitlines():
                        p = _split_nmcli_terse(ln)
                        if len(p) >= 3 and p[1] == "wifi" and p[2] == "connected":
                            return run_cmd(["nmcli", "dev", "disconnect", p[0]], timeout=8)
            return rc, out, err

        def _done(res):
            rc, _out, err = res
            msg = f"Rozłączono: {ssid}" if rc == 0 else f"Nie udało się rozłączyć ({err.strip()})"
            self.lbl_status.set_text(msg)
            self.refresh_state(rescan=False)

        run_async(_worker, _done, lambda e: self._set_busy(False, f"Błąd rozłączania: {e}"))

    def _connect_ssid(self, ssid: str, password=None, retry_prompt_if_auth_fail: bool = False):
        self._set_busy(True, f"Łączenie z siecią {ssid}...")

        def _worker():
            if password is None and ssid in self._saved_profiles:
                profile_id = self._saved_profiles[ssid]
                return run_cmd(["nmcli", "con", "up", "id", profile_id], timeout=20)
            cmd = ["nmcli", "dev", "wifi", "connect", ssid]
            if password:
                cmd += ["password", password]
            return run_cmd(cmd, timeout=20)

        def _done(res):
            rc, _out, err = res
            self._set_busy(False)
            if rc == 0:
                self.lbl_status.set_text(f"Połączono z {ssid}.")
                self.refresh_state(rescan=False)
                return

            err_low = err.lower()
            if retry_prompt_if_auth_fail and ("secret" in err_low or "password" in err_low or "auth" in err_low or rc != 0):
                pwd = self._ask_password(ssid, "WPA/PSK")
                if pwd is not None:
                    self._connect_ssid(ssid, password=pwd, retry_prompt_if_auth_fail=False)
                    return

            self.lbl_status.set_text(f"Błąd połączenia z {ssid}: {err.strip() or 'nieznany błąd'}")
            self.refresh_state(rescan=False)

        run_async(_worker, _done, lambda e: self._set_busy(False, f"Błąd połączenia: {e}"))

    def _ask_password(self, ssid: str, security: str):
        dialog = Gtk.Dialog(
            title=f"Hasło do sieci {ssid}",
            transient_for=self.parent_window,
            flags=Gtk.DialogFlags.MODAL | Gtk.DialogFlags.DESTROY_WITH_PARENT
        )
        dialog.add_button("Anuluj", Gtk.ResponseType.CANCEL)
        btn_ok = dialog.add_button("Połącz", Gtk.ResponseType.OK)
        btn_ok.get_style_context().add_class("accent-btn")
        dialog.set_default_response(Gtk.ResponseType.OK)

        area = dialog.get_content_area()
        area.set_spacing(10)
        area.set_margin_top(14)
        area.set_margin_bottom(14)
        area.set_margin_start(16)
        area.set_margin_end(16)

        lbl = Gtk.Label(label=f"Podaj hasło dla zabezpieczonej sieci „{ssid}” ({security}):", xalign=0.0)
        lbl.set_line_wrap(True)
        area.pack_start(lbl, False, False, 0)

        entry = Gtk.Entry()
        entry.set_visibility(False)
        entry.set_activates_default(True)
        entry.set_placeholder_text("Hasło Wi-Fi...")
        area.pack_start(entry, False, False, 0)

        chk_show = Gtk.CheckButton(label="Pokaż hasło")
        chk_show.connect("toggled", lambda w: entry.set_visibility(w.get_active()))
        area.pack_start(chk_show, False, False, 0)

        dialog.show_all()
        resp = dialog.run()
        pwd = entry.get_text() if resp == Gtk.ResponseType.OK else None
        dialog.destroy()
        return pwd if pwd else None
