# -*- coding: utf-8 -*-

"""
Zakładka „Dźwięk” w GTK3 obsługiwana przez WirePlumber (`wpctl`).
"""

import re
import shutil
from gi.repository import Gtk, GLib

from .helpers import (
    run_async,
    run_cmd,
    make_scrolled_page,
    make_page_header,
    make_card,
    make_setting_row,
)


def _parse_wpctl_status(raw: str) -> dict:
    """Parsuje `wpctl status` zwracając listy sinks i sources z sekcji Audio."""
    sinks = []
    sources = []

    section = ""
    subsection = ""
    node_re = re.compile(r"^[\s│├└─]*(\*)?\s*(\d+)\.\s+(.+?)(?:\s+\[vol:\s*([0-9.]+)(\s+MUTED)?\])?\s*$")

    for line in raw.splitlines():
        stripped = line.strip()
        if stripped in ("Audio", "Video", "Settings"):
            section = stripped
            subsection = ""
            continue
        if not stripped:
            subsection = ""
            continue

        if section != "Audio":
            continue

        clean = line.lstrip(" │├└─\t")
        if clean.startswith("Sinks:"):
            subsection = "Sinks"
            continue
        if clean.startswith("Sources:"):
            subsection = "Sources"
            continue
        if clean.startswith(("Devices:", "Filters:", "Streams:")):
            subsection = ""
            continue

        if subsection in ("Sinks", "Sources"):
            m = node_re.match(line)
            if not m:
                continue
            is_default = (m.group(1) == "*")
            node_id = m.group(2)
            name = m.group(3).strip()
            vol = float(m.group(4)) if m.group(4) else None
            muted = bool(m.group(5))
            item = {
                "id": node_id,
                "name": name,
                "default": is_default,
                "volume": vol,
                "muted": muted,
            }
            if subsection == "Sinks":
                sinks.append(item)
            else:
                sources.append(item)

    return {"sinks": sinks, "sources": sources}


def _parse_wpctl_get_volume(raw: str) -> tuple:
    """Parsuje wynik `wpctl get-volume` -> (pct_int, muted_bool)."""
    m = re.search(r"Volume:\s*([0-9.]+)", raw)
    vol_pct = int(round(float(m.group(1)) * 100)) if m else 0
    vol_pct = max(0, min(100, vol_pct))
    muted = "MUTED" in raw.upper()
    return vol_pct, muted


class SoundTab(Gtk.Box):
    """Widok ustawień dźwięku (głośność główna, wyciszanie, urządzenia wyjścia i wejścia)."""

    def __init__(self, _parent_window=None):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._updating_ui = False
        self._vol_debounce_id = 0
        self._mic_debounce_id = 0

        sw, self.content = make_scrolled_page()
        self.pack_start(sw, True, True, 0)

        self.btn_refresh = Gtk.Button(label="Odśwież urządzenia")
        self.btn_refresh.get_style_context().add_class("secondary-btn")
        self.btn_refresh.connect("clicked", lambda _w: self.refresh_sound())

        self.content.pack_start(
            make_page_header(
                "Dźwięk",
                "Sterowanie głośnością PipeWire / WirePlumber (wpctl) oraz wyborem urządzeń audio.",
                self.btn_refresh
            ),
            False, False, 0
        )

        # Karta głośności głównej i mikrofonu
        card_vol, box_vol = make_card("Głośność główna")

        out_ctrl = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        self.out_scale = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0, 100, 1)
        self.out_scale.set_size_request(220, -1)
        self.out_scale.set_value_pos(Gtk.PositionType.RIGHT)
        self.out_scale.connect("value-changed", self._on_out_volume_changed)
        out_ctrl.pack_start(self.out_scale, True, True, 0)

        self.out_mute_switch = Gtk.Switch()
        self.out_mute_switch.set_tooltip_text("Wycisz wyjście audio")
        self.out_mute_switch.connect("notify::active", self._on_out_mute_toggled)
        lbl_mute_out = Gtk.Label(label="Wycisz:")
        lbl_mute_out.get_style_context().add_class("row-subtitle")
        out_ctrl.pack_start(lbl_mute_out, False, False, 0)
        out_ctrl.pack_start(self.out_mute_switch, False, False, 0)

        box_vol.pack_start(
            make_setting_row(
                "Głośność wyjścia (@DEFAULT_AUDIO_SINK@)",
                "Poziom głośności domyślnych głośników lub słuchawek (%)",
                out_ctrl
            ),
            False, False, 0
        )

        mic_ctrl = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        self.mic_scale = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0, 100, 1)
        self.mic_scale.set_size_request(220, -1)
        self.mic_scale.set_value_pos(Gtk.PositionType.RIGHT)
        self.mic_scale.connect("value-changed", self._on_mic_volume_changed)
        mic_ctrl.pack_start(self.mic_scale, True, True, 0)

        self.mic_mute_switch = Gtk.Switch()
        self.mic_mute_switch.set_tooltip_text("Wycisz mikrofon")
        self.mic_mute_switch.connect("notify::active", self._on_mic_mute_toggled)
        lbl_mute_mic = Gtk.Label(label="Wycisz:")
        lbl_mute_mic.get_style_context().add_class("row-subtitle")
        mic_ctrl.pack_start(lbl_mute_mic, False, False, 0)
        mic_ctrl.pack_start(self.mic_mute_switch, False, False, 0)

        box_vol.pack_start(
            make_setting_row(
                "Czułość mikrofonu (@DEFAULT_AUDIO_SOURCE@)",
                "Poziom wzmocnienia domyślnego źródła wejściowego (%)",
                mic_ctrl
            ),
            False, False, 0
        )

        self.content.pack_start(card_vol, False, False, 0)

        # Karta urządzeń wyjściowych (Output devices)
        card_out, box_out = make_card("Urządzenia wyjściowe (Output devices)")
        self.sinks_list = Gtk.ListBox()
        self.sinks_list.set_selection_mode(Gtk.SelectionMode.SINGLE)
        self.sinks_list.get_style_context().add_class("settings-list")
        box_out.pack_start(self.sinks_list, False, False, 0)
        self.content.pack_start(card_out, False, False, 0)

        # Karta urządzeń wejściowych (Input / Mikrofon)
        card_in, box_in = make_card("Urządzenia wejściowe (Input / Mikrofon)")
        self.sources_list = Gtk.ListBox()
        self.sources_list.set_selection_mode(Gtk.SelectionMode.SINGLE)
        self.sources_list.get_style_context().add_class("settings-list")
        box_in.pack_start(self.sources_list, False, False, 0)
        self.content.pack_start(card_in, False, False, 0)

        self.lbl_status = Gtk.Label(label="", xalign=0.0)
        self.lbl_status.get_style_context().add_class("row-subtitle")
        self.content.pack_start(self.lbl_status, False, False, 0)

        self.refresh_sound()

    def refresh_sound(self):
        if not shutil.which("wpctl"):
            self.lbl_status.set_text("Brak narzędzia `wpctl` (pakiet wireplumber) w systemie.")
            self.btn_refresh.set_sensitive(False)
            return

        self.btn_refresh.set_sensitive(False)
        self.lbl_status.set_text("Odczytywanie stanu PipeWire / WirePlumber...")

        def _worker():
            rc_s, out_s, err_s = run_cmd(["wpctl", "status"], timeout=5)
            rc_vo, out_vo, _ = run_cmd(["wpctl", "get-volume", "@DEFAULT_AUDIO_SINK@"], timeout=4)
            rc_vi, out_vi, _ = run_cmd(["wpctl", "get-volume", "@DEFAULT_AUDIO_SOURCE@"], timeout=4)

            parsed = _parse_wpctl_status(out_s) if rc_s == 0 else {"sinks": [], "sources": []}
            out_vol, out_muted = _parse_wpctl_get_volume(out_vo) if rc_vo == 0 else (50, False)
            mic_vol, mic_muted = _parse_wpctl_get_volume(out_vi) if rc_vi == 0 else (50, False)

            return {
                "ok": (rc_s == 0),
                "err": err_s.strip(),
                "sinks": parsed["sinks"],
                "sources": parsed["sources"],
                "out_vol": out_vol,
                "out_muted": out_muted,
                "mic_vol": mic_vol,
                "mic_muted": mic_muted,
            }

        def _on_done(res: dict):
            self.btn_refresh.set_sensitive(True)
            if not res["ok"]:
                self.lbl_status.set_text(f"Błąd wpctl: {res['err'] or 'usługa PipeWire nie odpowiada'}")
                return

            self._updating_ui = True
            self.out_scale.set_value(res["out_vol"])
            self.out_mute_switch.set_active(res["out_muted"])
            self.mic_scale.set_value(res["mic_vol"])
            self.mic_mute_switch.set_active(res["mic_muted"])
            self._updating_ui = False

            self._populate_device_list(self.sinks_list, res["sinks"], is_sink=True)
            self._populate_device_list(self.sources_list, res["sources"], is_sink=False)
            self.lbl_status.set_text(
                f"Wyjścia: {len(res['sinks'])} • Wejścia: {len(res['sources'])}"
            )

        run_async(_worker, _on_done, lambda e: (self.btn_refresh.set_sensitive(True), self.lbl_status.set_text(f"Błąd: {e}")))

    def _populate_device_list(self, listbox: Gtk.ListBox, devices: list, is_sink: bool):
        for child in listbox.get_children():
            listbox.remove(child)

        if not devices:
            row = Gtk.ListBoxRow()
            row.set_selectable(False)
            lbl = Gtk.Label(label="Brak wykrytych urządzeń.")
            lbl.set_margin_top(10)
            lbl.set_margin_bottom(10)
            lbl.get_style_context().add_class("row-subtitle")
            row.add(lbl)
            listbox.add(row)
            listbox.show_all()
            return

        icon_name = "audio-speakers-symbolic" if is_sink else "audio-input-microphone-symbolic"
        for dev in devices:
            row = Gtk.ListBoxRow()
            hbox = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)

            icon = Gtk.Image.new_from_icon_name(icon_name, Gtk.IconSize.MENU)
            hbox.pack_start(icon, False, False, 0)

            lbl_name = Gtk.Label(label=f"[{dev['id']}] {dev['name']}", xalign=0.0)
            lbl_name.get_style_context().add_class("row-title")
            hbox.pack_start(lbl_name, True, True, 0)

            if dev["default"]:
                badge = Gtk.Label(label="Domyślne")
                badge.get_style_context().add_class("badge-connected")
                hbox.pack_end(badge, False, False, 0)
            else:
                btn_use = Gtk.Button(label="Użyj")
                btn_use.get_style_context().add_class("secondary-btn")
                btn_use.connect("clicked", lambda _b, nid=dev["id"]: self._set_default_device(nid))
                hbox.pack_end(btn_use, False, False, 0)

            row.add(hbox)
            listbox.add(row)

        listbox.show_all()

    def _set_default_device(self, node_id: str):
        run_async(
            lambda: run_cmd(["wpctl", "set-default", str(node_id)], timeout=4),
            lambda _r: self.refresh_sound()
        )

    def _on_out_volume_changed(self, scale):
        if self._updating_ui or not shutil.which("wpctl"):
            return
        pct = int(round(scale.get_value()))
        if self._vol_debounce_id:
            GLib.source_remove(self._vol_debounce_id)
        self._vol_debounce_id = GLib.timeout_add(60, self._apply_out_volume, pct)

    def _apply_out_volume(self, pct: int) -> bool:
        self._vol_debounce_id = 0
        run_async(lambda: run_cmd(["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", f"{pct}%"], timeout=3))
        return False

    def _on_out_mute_toggled(self, switch, _pspec):
        if self._updating_ui or not shutil.which("wpctl"):
            return
        mute_arg = "1" if switch.get_active() else "0"
        run_async(lambda: run_cmd(["wpctl", "set-mute", "@DEFAULT_AUDIO_SINK@", mute_arg], timeout=3))

    def _on_mic_volume_changed(self, scale):
        if self._updating_ui or not shutil.which("wpctl"):
            return
        pct = int(round(scale.get_value()))
        if self._mic_debounce_id:
            GLib.source_remove(self._mic_debounce_id)
        self._mic_debounce_id = GLib.timeout_add(60, self._apply_mic_volume, pct)

    def _apply_mic_volume(self, pct: int) -> bool:
        self._mic_debounce_id = 0
        run_async(lambda: run_cmd(["wpctl", "set-volume", "@DEFAULT_AUDIO_SOURCE@", f"{pct}%"], timeout=3))
        return False

    def _on_mic_mute_toggled(self, switch, _pspec):
        if self._updating_ui or not shutil.which("wpctl"):
            return
        mute_arg = "1" if switch.get_active() else "0"
        run_async(lambda: run_cmd(["wpctl", "set-mute", "@DEFAULT_AUDIO_SOURCE@", mute_arg], timeout=3))
