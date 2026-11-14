# -*- coding: utf-8 -*-

"""
Zakładka „Ekrany i Wyjścia” (wlr-randr + trwały zapis [output:<name>] w ~/.config/wayfire.ini) w GTK3.
Obsługuje rozdzielczość, odświeżanie Hz, skalowanie ułamkowe (0.5x – 3.0x), orientację (obrót)
oraz pozycjonowanie układu wielomonitorowego (X, Y) z natychmiastowym hot-reloadem w Wayfire.
"""

import re
import json
import shutil
from gi.repository import Gtk

from .helpers import (
    apply_wayfire_outputs,
    clear_container,
    run_async,
    run_cmd,
    make_scrolled_page,
    make_page_header,
    make_card,
    make_setting_row,
)

SCALE_PRESETS = [1.0, 1.25, 1.5, 2.0]


def _parse_wlr_randr_json(raw: str) -> list:
    data = json.loads(raw)
    if not isinstance(data, list):
        return []
    outputs = []
    for item in data:
        name = item.get("name", "")
        if not name:
            continue
        desc = item.get("description") or " ".join(
            p for p in (item.get("make", ""), item.get("model", "")) if p
        ).strip()
        pos = item.get("position") or {}
        pos_x = int(pos.get("x", 0) or 0)
        pos_y = int(pos.get("y", 0) or 0)
        transform = str(item.get("transform", "normal") or "normal").strip()
        modes = []
        current_mode = None
        for m in item.get("modes") or []:
            w = int(m.get("width") or 0)
            h = int(m.get("height") or 0)
            if w <= 0 or h <= 0:
                continue
            hz = float(m.get("refresh") or 60.0)
            cur = bool(m.get("current", False))
            pref = bool(m.get("preferred", False))
            entry = {
                "width": w,
                "height": h,
                "res": f"{w}x{h}",
                "refresh": hz,
                "refresh_raw": f"{hz:.6f}",
                "current": cur,
                "preferred": pref,
            }
            modes.append(entry)
            if cur:
                current_mode = entry
        outputs.append({
            "name": name,
            "description": desc or name,
            "enabled": bool(item.get("enabled", True)),
            "scale": float(item.get("scale", 1.0) or 1.0),
            "x": pos_x,
            "y": pos_y,
            "transform": transform,
            "modes": modes,
            "current_mode": current_mode,
        })
    return outputs


def _parse_wlr_randr_text(raw: str) -> list:
    outputs = []
    cur_out = None
    in_modes = False

    mode_re = re.compile(
        r"^\s*(\d+)x(\d+)\s+px,\s*([0-9.]+)\s+Hz(?:\s+\(([^)]+)\))?",
        re.IGNORECASE,
    )
    pos_re = re.compile(r"Position:\s*(-?\d+)\s*,\s*(-?\d+)", re.IGNORECASE)

    for line in raw.splitlines():
        if not line.strip():
            continue
        if not line[0].isspace():
            parts = line.split(None, 1)
            name = parts[0].strip()
            desc = parts[1].strip().strip('"') if len(parts) > 1 else name
            cur_out = {
                "name": name,
                "description": desc or name,
                "enabled": True,
                "scale": 1.0,
                "x": 0,
                "y": 0,
                "transform": "normal",
                "modes": [],
                "current_mode": None,
            }
            outputs.append(cur_out)
            in_modes = False
            continue

        if cur_out is None:
            continue

        stripped = line.strip()
        if stripped.startswith("Enabled:"):
            val = stripped.split(":", 1)[1].strip().lower()
            cur_out["enabled"] = val in ("yes", "true", "1")
            in_modes = False
        elif stripped.startswith("Scale:"):
            try:
                cur_out["scale"] = float(stripped.split(":", 1)[1].strip())
            except ValueError:
                cur_out["scale"] = 1.0
            in_modes = False
        elif stripped.startswith("Position:"):
            mp = pos_re.search(stripped)
            if mp:
                cur_out["x"] = int(mp.group(1))
                cur_out["y"] = int(mp.group(2))
            in_modes = False
        elif stripped.startswith("Transform:"):
            cur_out["transform"] = stripped.split(":", 1)[1].strip() or "normal"
            in_modes = False
        elif stripped == "Modes:":
            in_modes = True
        elif in_modes:
            m = mode_re.match(line)
            if m:
                w = int(m.group(1))
                h = int(m.group(2))
                rate_str = m.group(3)
                flags = (m.group(4) or "").lower()
                is_cur = "current" in flags
                is_pref = "preferred" in flags
                entry = {
                    "width": w,
                    "height": h,
                    "res": f"{w}x{h}",
                    "refresh": float(rate_str),
                    "refresh_raw": rate_str,
                    "current": is_cur,
                    "preferred": is_pref,
                }
                cur_out["modes"].append(entry)
                if is_cur:
                    cur_out["current_mode"] = entry
        elif ":" in stripped:
            in_modes = False

    return outputs


class DisplayTab(Gtk.Box):
    """Zakładka konfiguracji monitorów, skalowania ułamkowego i układu wyjść Wayfire."""

    def __init__(self, _parent_window=None, defer_load: bool = False):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._outputs = []
        self._selected_output = None
        self._res_map = {}
        self._rate_items = []
        self._updating_ui = False
        self._loaded = False

        sw, self.content = make_scrolled_page()
        self.pack_start(sw, True, True, 0)

        self.btn_refresh = Gtk.Button(label="Odśwież ekrany")
        self.btn_refresh.get_style_context().add_class("secondary-btn")
        self.btn_refresh.connect("clicked", lambda _w: self.refresh_displays())

        self.content.pack_start(
            make_page_header(
                "Ekrany i Wyjścia",
                "Rozdzielczość, odświeżanie Hz, skalowanie ułamkowe oraz układ monitorów synchronizowany z [output:...] w wayfire.ini.",
                self.btn_refresh,
            ),
            False,
            False,
            0,
        )

        # Lista podłączonych ekranów
        card_outs, box_outs = make_card("Podłączone ekrany")
        self.outputs_list = Gtk.ListBox()
        self.outputs_list.set_selection_mode(Gtk.SelectionMode.SINGLE)
        self.outputs_list.get_style_context().add_class("settings-list")
        self.outputs_list.connect("row-selected", self._on_output_selected)
        box_outs.pack_start(self.outputs_list, False, False, 0)
        self.content.pack_start(card_outs, False, False, 0)

        # Karta ustawień wybranego monitora
        card_cfg, box_cfg = make_card("Konfiguracja wybranego monitora")

        self.switch_enabled = Gtk.Switch()
        self.switch_enabled.set_active(True)
        self.switch_enabled.connect("notify::active", self._on_enabled_toggled)
        box_cfg.pack_start(
            make_setting_row(
                "Wyjście aktywne",
                "Włącza lub wyłącza sygnał na wybranym monitorze",
                self.switch_enabled,
            ),
            False,
            False,
            0,
        )

        self.combo_res = Gtk.ComboBoxText()
        self.combo_res.connect("changed", self._on_resolution_changed)
        box_cfg.pack_start(
            make_setting_row(
                "Rozdzielczość",
                "Dostępne tryby pracy zgłoszone przez ekran (EDID / DRM)",
                self.combo_res,
            ),
            False,
            False,
            0,
        )

        self.combo_rate = Gtk.ComboBoxText()
        box_cfg.pack_start(
            make_setting_row(
                "Częstotliwość odświeżania (Hz)",
                "Częstotliwość odświeżania obrazu dla wybranej rozdzielczości (60 / 120 / 144+ Hz)",
                self.combo_rate,
            ),
            False,
            False,
            0,
        )

        # Selektor skali: przyciski 1.0x / 1.25x / 1.5x / 2.0x + suwak precyzyjny
        scale_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        pills_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        self._scale_pills = {}
        for s in SCALE_PRESETS:
            tb = Gtk.ToggleButton(label=f"{s:g}x" if s not in (1.0, 2.0) else f"{s:.1f}x")
            tb.get_style_context().add_class("scale-pill")
            tb.connect("toggled", self._on_scale_pill_toggled, s)
            pills_box.pack_start(tb, False, False, 0)
            self._scale_pills[s] = tb

        self.scale_slider = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0.5, 3.0, 0.05)
        self.scale_slider.set_digits(2)
        self.scale_slider.set_size_request(220, -1)
        self.scale_slider.set_value(1.0)
        self.scale_slider.set_value_pos(Gtk.PositionType.RIGHT)
        self.scale_slider.connect("value-changed", self._on_scale_slider_changed)

        scale_box.pack_start(pills_box, False, False, 0)
        scale_box.pack_start(self.scale_slider, False, False, 0)

        box_cfg.pack_start(
            make_setting_row(
                "Skalowanie ułamkowe (Fractional Scaling)",
                "Szybki wybór skali (1.0x, 1.25x, 1.5x, 2.0x) lub płynna regulacja suwakiem",
                scale_box,
            ),
            False,
            False,
            0,
        )

        # Układ monitorów (pozycja X, Y) oraz orientacja
        pos_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        lbl_x = Gtk.Label(label="X:")
        lbl_x.get_style_context().add_class("row-subtitle")
        self.spin_pos_x = Gtk.SpinButton.new_with_range(-16384, 16384, 10)
        self.spin_pos_x.set_value(0)
        lbl_y = Gtk.Label(label="Y:")
        lbl_y.get_style_context().add_class("row-subtitle")
        self.spin_pos_y = Gtk.SpinButton.new_with_range(-16384, 16384, 10)
        self.spin_pos_y.set_value(0)
        pos_box.pack_start(lbl_x, False, False, 0)
        pos_box.pack_start(self.spin_pos_x, False, False, 0)
        pos_box.pack_start(lbl_y, False, False, 0)
        pos_box.pack_start(self.spin_pos_y, False, False, 0)

        box_cfg.pack_start(
            make_setting_row(
                "Pozycja w układzie wielomonitorowym (X, Y)",
                "Współrzędne ekranu zapisywane w sekcji [output:<nazwa>] w ~/.config/wayfire.ini",
                pos_box,
            ),
            False,
            False,
            0,
        )

        self.combo_transform = Gtk.ComboBoxText()
        self.combo_transform.append("normal", "Standardowa (0°)")
        self.combo_transform.append("90", "Obrót 90°")
        self.combo_transform.append("180", "Obrót 180°")
        self.combo_transform.append("270", "Obrót 270°")
        self.combo_transform.set_active_id("normal")
        box_cfg.pack_start(
            make_setting_row(
                "Orientacja ekranu (Transform)",
                "Obrót przestrzeni roboczej wybranego monitora",
                self.combo_transform,
            ),
            False,
            False,
            0,
        )

        # Przycisk Zastosuj + status
        footer = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        self.lbl_status = Gtk.Label(label="", xalign=0.0)
        self.lbl_status.set_line_wrap(True)
        self.lbl_status.get_style_context().add_class("row-subtitle")
        footer.pack_start(self.lbl_status, True, True, 0)

        self.btn_apply = Gtk.Button(label="Zastosuj i zapisz w wayfire.ini")
        self.btn_apply.get_style_context().add_class("accent-btn")
        self.btn_apply.connect("clicked", self._on_apply_clicked)
        footer.pack_end(self.btn_apply, False, False, 0)

        box_cfg.pack_start(footer, False, False, 0)
        self.content.pack_start(card_cfg, False, False, 0)

        if not defer_load:
            self.ensure_loaded()

    def ensure_loaded(self):
        if not self._loaded:
            self._loaded = True
            self.refresh_displays()

    def refresh_displays(self):
        if not shutil.which("wlr-randr"):
            self.lbl_status.set_text("Brak narzędzia `wlr-randr` w systemie.")
            self.btn_apply.set_sensitive(False)
            return

        self.btn_refresh.set_sensitive(False)
        self.lbl_status.set_text("Odczytywanie listy monitorów...")

        def _worker():
            rc_j, out_j, _ = run_cmd(["wlr-randr", "--json"], timeout=5)
            if rc_j == 0 and out_j.strip().startswith("["):
                try:
                    parsed = _parse_wlr_randr_json(out_j)
                    if parsed:
                        return {"ok": True, "outputs": parsed, "err": ""}
                except Exception:
                    pass
            rc_t, out_t, err_t = run_cmd(["wlr-randr"], timeout=5)
            if rc_t != 0:
                return {"ok": False, "outputs": [], "err": err_t.strip() or out_t.strip()}
            return {"ok": True, "outputs": _parse_wlr_randr_text(out_t), "err": ""}

        def _on_done(res: dict):
            self.btn_refresh.set_sensitive(True)
            if not res["ok"]:
                self.lbl_status.set_text(f"Błąd wlr-randr: {res['err']}")
                return
            self._outputs = res["outputs"]
            self._populate_outputs_list()

        run_async(_worker, _on_done, lambda e: (self.btn_refresh.set_sensitive(True), self.lbl_status.set_text(f"Błąd: {e}")))

    def _populate_outputs_list(self):
        prev_name = self._selected_output["name"] if self._selected_output else None
        clear_container(self.outputs_list)

        if not self._outputs:
            self._selected_output = None
            row = Gtk.ListBoxRow()
            row.set_selectable(False)
            lbl = Gtk.Label(label="Nie wykryto monitorów w sesji Wayland.")
            lbl.set_margin_top(10)
            lbl.set_margin_bottom(10)
            row.add(lbl)
            self.outputs_list.add(row)
            self.outputs_list.show_all()
            self.btn_apply.set_sensitive(False)
            return

        target_row = None
        for out in self._outputs:
            row = Gtk.ListBoxRow()
            row._output_data = out

            hbox = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
            icon = Gtk.Image.new_from_icon_name("video-display-symbolic", Gtk.IconSize.MENU)
            hbox.pack_start(icon, False, False, 0)

            cur_m = out.get("current_mode")
            pos_str = f"({out.get('x', 0)}, {out.get('y', 0)})"
            if not out["enabled"]:
                mode_info = "Wyłączony"
            elif cur_m:
                mode_info = f"{cur_m['res']} @ {cur_m['refresh']:.2f} Hz • Skala {out['scale']:.2f}x • Pozycja {pos_str}"
            else:
                mode_info = f"Skala {out['scale']:.2f}x • Pozycja {pos_str}"

            vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=1)
            lbl_n = Gtk.Label(label=f"{out['name']} — {out['description']}", xalign=0.0)
            lbl_n.get_style_context().add_class("row-title")
            lbl_m = Gtk.Label(label=mode_info, xalign=0.0)
            lbl_m.get_style_context().add_class("row-subtitle")
            vbox.pack_start(lbl_n, False, False, 0)
            vbox.pack_start(lbl_m, False, False, 0)
            hbox.pack_start(vbox, True, True, 0)

            if out["enabled"]:
                badge = Gtk.Label(label="Aktywny")
                badge.get_style_context().add_class("badge-connected")
                hbox.pack_end(badge, False, False, 0)

            row.add(hbox)
            self.outputs_list.add(row)
            if prev_name and out["name"] == prev_name:
                target_row = row
            elif target_row is None:
                target_row = row

        self.outputs_list.show_all()
        if target_row is not None:
            self.outputs_list.select_row(target_row)
        self.btn_apply.set_sensitive(True)
        self.lbl_status.set_text(f"Wykryto ekranów: {len(self._outputs)}")

    def _on_output_selected(self, _lb, row):
        out = getattr(row, "_output_data", None) if row else None
        if not out:
            return
        self._selected_output = out
        self._load_output_controls(out)

    def _load_output_controls(self, out: dict):
        self._updating_ui = True
        self.switch_enabled.set_active(bool(out.get("enabled", True)))
        self.spin_pos_x.set_value(float(out.get("x", 0)))
        self.spin_pos_y.set_value(float(out.get("y", 0)))
        tf = str(out.get("transform", "normal") or "normal")
        if not self.combo_transform.set_active_id(tf):
            self.combo_transform.set_active_id("normal")

        self._res_map = {}
        for m in out.get("modes", []):
            self._res_map.setdefault(m["res"], []).append(m)

        def _res_sort_key(r: str):
            try:
                w_s, h_s = r.split("x", 1)
                return (int(w_s), int(h_s))
            except Exception:
                return (0, 0)

        sorted_res = sorted(self._res_map.keys(), key=_res_sort_key, reverse=True)

        self.combo_res.remove_all()
        for r in sorted_res:
            self.combo_res.append_text(r)

        cur_m = out.get("current_mode")
        target_res = cur_m["res"] if cur_m and cur_m["res"] in sorted_res else (sorted_res[0] if sorted_res else None)
        if target_res:
            idx = sorted_res.index(target_res)
            self.combo_res.set_active(idx)
        self._updating_ui = False

        self._populate_rates_for_resolution(target_res, cur_m)

        scale = float(out.get("scale", 1.0) or 1.0)
        self._sync_scale_widgets(scale)
        self._update_controls_sensitivity()

    def _update_controls_sensitivity(self):
        active = bool(self.switch_enabled.get_active())
        self.combo_res.set_sensitive(active)
        self.combo_rate.set_sensitive(active)
        self.scale_slider.set_sensitive(active)
        self.spin_pos_x.set_sensitive(active)
        self.spin_pos_y.set_sensitive(active)
        self.combo_transform.set_sensitive(active)
        for tb in self._scale_pills.values():
            tb.set_sensitive(active)

    def _on_enabled_toggled(self, _switch, _pspec):
        if self._updating_ui:
            return
        self._update_controls_sensitivity()

    def _on_resolution_changed(self, _combo):
        if self._updating_ui:
            return
        res = self.combo_res.get_active_text()
        self._populate_rates_for_resolution(res, None)

    def _populate_rates_for_resolution(self, res_str: str, preferred_cur_mode):
        self._updating_ui = True
        self.combo_rate.remove_all()
        self._rate_items = []
        if res_str and res_str in self._res_map:
            modes = sorted(self._res_map[res_str], key=lambda m: m["refresh"], reverse=True)
            active_idx = 0
            for idx, m in enumerate(modes):
                label = f"{m['refresh']:.2f} Hz"
                if m.get("preferred"):
                    label += " (zalecana)"
                self.combo_rate.append_text(label)
                self._rate_items.append(m)
                if preferred_cur_mode and abs(m["refresh"] - preferred_cur_mode["refresh"]) < 0.05:
                    active_idx = idx
            if self._rate_items:
                self.combo_rate.set_active(active_idx)
        self._updating_ui = False

    def _sync_scale_widgets(self, scale_val: float):
        self._updating_ui = True
        self.scale_slider.set_value(scale_val)
        for preset, tb in self._scale_pills.items():
            tb.set_active(abs(scale_val - preset) < 0.02)
        self._updating_ui = False

    def _on_scale_pill_toggled(self, tb, preset: float):
        if self._updating_ui:
            return
        if tb.get_active():
            self._sync_scale_widgets(preset)
        else:
            self._sync_scale_widgets(self.scale_slider.get_value())

    def _on_scale_slider_changed(self, slider):
        if self._updating_ui:
            return
        val = round(slider.get_value(), 2)
        self._updating_ui = True
        for preset, tb in self._scale_pills.items():
            tb.set_active(abs(val - preset) < 0.02)
        self._updating_ui = False

    def _on_apply_clicked(self, _btn):
        if not self._selected_output:
            return
        out_name = self._selected_output["name"]
        enabled = self.switch_enabled.get_active()
        res_str = self.combo_res.get_active_text()
        rate_idx = self.combo_rate.get_active()
        scale_val = round(self.scale_slider.get_value(), 2)
        pos_x = int(self.spin_pos_x.get_value_as_int())
        pos_y = int(self.spin_pos_y.get_value_as_int())
        transform = self.combo_transform.get_active_id() or "normal"

        mode_obj = self._rate_items[rate_idx] if (0 <= rate_idx < len(self._rate_items)) else None
        width, height, hz = 1920, 1080, 60.0
        if mode_obj:
            width = int(mode_obj.get("width", 1920))
            height = int(mode_obj.get("height", 1080))
            hz = float(mode_obj.get("refresh", 60.0))
        elif res_str and "x" in res_str:
            try:
                w_s, h_s = res_str.split("x", 1)
                width, height = int(w_s), int(h_s)
            except Exception:
                pass

        # Trwały zapis sekcji [output:<name>] w ~/.config/wayfire.ini
        apply_wayfire_outputs([{
            "name": out_name,
            "enabled": enabled,
            "width": width,
            "height": height,
            "refresh": hz,
            "scale": scale_val,
            "x": pos_x,
            "y": pos_y,
            "transform": transform,
        }])

        self.btn_apply.set_sensitive(False)
        self.lbl_status.set_text(f"Stosowanie konfiguracji dla {out_name}...")

        def _worker():
            if not enabled:
                return run_cmd(["wlr-randr", "--output", out_name, "--off"], timeout=6)

            cmd = [
                "wlr-randr", "--output", out_name, "--on",
                "--pos", f"{pos_x},{pos_y}",
                "--transform", transform,
            ]
            if res_str and mode_obj:
                cmd += ["--mode", f"{res_str}@{mode_obj['refresh_raw']}Hz"]
            elif res_str:
                cmd += ["--mode", res_str]
            cmd += ["--scale", f"{scale_val:.2f}"]

            rc, out, err = run_cmd(cmd, timeout=6)
            if rc != 0 and res_str and mode_obj:
                fallback_cmd = [
                    "wlr-randr", "--output", out_name, "--on",
                    "--mode", res_str, "--scale", f"{scale_val:.2f}",
                    "--pos", f"{pos_x},{pos_y}",
                    "--transform", transform,
                ]
                return run_cmd(fallback_cmd, timeout=6)
            return rc, out, err

        def _on_done(res):
            rc, _out, err = res
            self.btn_apply.set_sensitive(True)
            if rc == 0:
                self.lbl_status.set_text(f"Zastosowano i zapisano [output:{out_name}] w ~/.config/wayfire.ini.")
                self.refresh_displays()
            else:
                self.lbl_status.set_text(
                    f"Zapisano [output:{out_name}] w wayfire.ini (wlr-randr: {err.strip() or 'kod ' + str(rc)})."
                )

        run_async(_worker, _on_done, lambda e: (self.btn_apply.set_sensitive(True), self.lbl_status.set_text(f"Błąd: {e}")))
