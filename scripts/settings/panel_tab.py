# -*- coding: utf-8 -*-

"""
Zakładka „Pulpit i Panel” w GTK3.
Edytuje ~/.config/okay-de/panel.json (tryb tylko ikon, pozycja panelu)
oraz opcjonalnie ustawia tapetę pulpitu (swaybg).
"""

import os
import json
import shutil
import subprocess
from gi.repository import Gtk

from .helpers import (
    okay_config_dir,
    okay_template_file,
    wallpaper_template_file,
    xdg_bin_home,
    atomic_write_json,
    atomic_copy_file,
    normalize_panel_config,
    ensure_user_configs,
    make_scrolled_page,
    make_page_header,
    make_card,
    make_setting_row,
)


def _panel_json_path() -> str:
    return os.path.join(okay_config_dir(), "panel.json")


def _user_wallpaper_path() -> str:
    return os.path.join(okay_config_dir(), "wallpaper.png")


class PanelTab(Gtk.Box):
    """Widok konfiguracji paska zadań (panel.json) oraz pulpitu OKay DE."""

    def __init__(self, parent_window=None, defer_load: bool = False, on_animations_changed=None):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.parent_window = parent_window
        self._on_animations_changed = on_animations_changed
        self._updating_ui = False
        self._loaded = False

        sw, self.content = make_scrolled_page()
        self.pack_start(sw, True, True, 0)

        self.btn_restart_panel = Gtk.Button(label="Przeładuj panel")
        self.btn_restart_panel.get_style_context().add_class("secondary-btn")
        self.btn_restart_panel.connect("clicked", self._on_restart_panel_clicked)

        self.content.pack_start(
            make_page_header(
                "Pulpit i Panel",
                "Konfiguracja zachowania paska zadań (~/.config/okay-de/panel.json) oraz tapety.",
                self.btn_restart_panel
            ),
            False, False, 0
        )

        # Karta ustawień paska zadań
        card_panel, box_panel = make_card("Pasek zadań (okay-panel)")

        self.switch_icon_only = Gtk.Switch()
        self.switch_icon_only.connect("notify::active", self._on_setting_changed)
        box_panel.pack_start(
            make_setting_row(
                "Ukrywaj etykiety okien (tylko ikony)",
                "Pokazuje na pasku zadań same ikony aplikacji bez długich tytułów okien",
                self.switch_icon_only
            ),
            False, False, 0
        )

        self.combo_mode = Gtk.ComboBoxText()
        self.combo_mode.append("panel", "Pasek zadań (pełna szerokość)")
        self.combo_mode.append("dock", "Pływający Dock (wyśrodkowany)")
        self.combo_mode.connect("changed", self._on_setting_changed)
        box_panel.pack_start(
            make_setting_row(
                "Styl paska",
                "Tryb pełnego paska zadań albo wyśrodkowanego pływającego Docka (z grupowaniem okien)",
                self.combo_mode
            ),
            False, False, 0
        )

        self.combo_position = Gtk.ComboBoxText()
        self.combo_position.append("bottom", "Dół ekranu (domyślna)")
        self.combo_position.append("top", "Góra ekranu")
        self.combo_position.connect("changed", self._on_setting_changed)
        box_panel.pack_start(
            make_setting_row(
                "Pozycja panelu",
                "Krawędź ekranu, do której zakotwiczony jest pasek zadań",
                self.combo_position
            ),
            False, False, 0
        )

        self.switch_animations = Gtk.Switch()
        self.switch_animations.connect("notify::active", self._on_setting_changed)
        box_panel.pack_start(
            make_setting_row(
                "Animacje interfejsu",
                "Płynne przejścia zakładek, przycisków i menu (panel.json)",
                self.switch_animations
            ),
            False, False, 0
        )

        self.lbl_pins_info = Gtk.Label(label="", xalign=0.0)
        self.lbl_pins_info.get_style_context().add_class("row-subtitle")
        box_panel.pack_start(self.lbl_pins_info, False, False, 0)

        self.content.pack_start(card_panel, False, False, 0)

        # Karta pulpitu / tapety
        card_desk, box_desk = make_card("Pulpit i tło")

        wp_btns = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.btn_wallpaper = Gtk.Button(label="Wybierz obraz...")
        self.btn_wallpaper.get_style_context().add_class("secondary-btn")
        self.btn_wallpaper.connect("clicked", self._on_choose_wallpaper)
        wp_btns.pack_start(self.btn_wallpaper, False, False, 0)

        self.btn_wp_default = Gtk.Button(label="Domyślna OKsavi")
        self.btn_wp_default.get_style_context().add_class("secondary-btn")
        self.btn_wp_default.connect("clicked", self._on_reset_wallpaper)
        wp_btns.pack_start(self.btn_wp_default, False, False, 0)

        box_desk.pack_start(
            make_setting_row(
                "Tapeta pulpitu",
                f"Aktualny plik: {_user_wallpaper_path()}",
                wp_btns
            ),
            False, False, 0
        )

        self.content.pack_start(card_desk, False, False, 0)

        self.lbl_status = Gtk.Label(label="", xalign=0.0)
        self.lbl_status.get_style_context().add_class("row-subtitle")
        self.content.pack_start(self.lbl_status, False, False, 0)

        if not defer_load:
            self.ensure_loaded()

    def ensure_loaded(self):
        if not self._loaded:
            self._loaded = True
            self.load_config()

    def _read_panel_json(self) -> dict:
        ensure_user_configs()
        panel_file = _panel_json_path()
        src = panel_file if os.path.exists(panel_file) else okay_template_file("panel.json")
        loaded = {}
        if src and os.path.exists(src):
            try:
                with open(src, "r", encoding="utf-8") as f:
                    parsed = json.load(f)
                if isinstance(parsed, dict):
                    loaded = parsed
            except Exception:
                pass
        return normalize_panel_config(loaded)

    def load_config(self):
        cfg = self._read_panel_json()
        self._updating_ui = True
        self.switch_icon_only.set_active(bool(cfg["icon_only"]))
        self.combo_mode.set_active_id(cfg["mode"])
        self.combo_position.set_active_id(cfg["position"])
        self.switch_animations.set_active(bool(cfg["animations"]))
        self._updating_ui = False

        pins = cfg.get("pinned_apps", [])
        names = ", ".join(p.get("name", p.get("app_id", "")) for p in pins if isinstance(p, dict))
        self.lbl_pins_info.set_text(
            f"Przypięte aplikacje ({len(pins)}): {names}" if pins else "Brak przypiętych aplikacji."
        )
        self.lbl_status.set_text(f"Konfiguracja wczytana z: {_panel_json_path()}")

    def _on_setting_changed(self, *_args):
        if self._updating_ui:
            return
        cfg = self._read_panel_json()
        icon_only = bool(self.switch_icon_only.get_active())
        mode = self.combo_mode.get_active_id() or "panel"
        pos = self.combo_position.get_active_id() or "bottom"
        animations = bool(self.switch_animations.get_active())

        cfg["icon_only"] = icon_only
        cfg["show_labels"] = not icon_only
        cfg["mode"] = "dock" if mode == "dock" else "panel"
        cfg["position"] = pos
        cfg["animations"] = animations

        if self._on_animations_changed is not None:
            try:
                self._on_animations_changed(animations)
            except Exception:
                pass

        panel_file = _panel_json_path()
        try:
            atomic_write_json(panel_file, cfg)
            self.lbl_status.set_text(
                f"Zapisano panel.json (styl={cfg['mode']}, pozycja={pos}, tylko ikony={icon_only}, animacje={animations}). Pasek odświeży się automatycznie."
            )
        except Exception as exc:
            self.lbl_status.set_text(f"Błąd zapisu {panel_file}: {exc}")

    def _on_restart_panel_clicked(self, _btn):
        try:
            local_panel = os.path.join(xdg_bin_home(), "okay-panel")
            panel_bin = local_panel if os.path.isfile(local_panel) and os.access(local_panel, os.X_OK) else (shutil.which("okay-panel") or "")
            if panel_bin and os.path.exists(panel_bin):
                subprocess.Popen(
                    [panel_bin, "--restart"],
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    start_new_session=True
                )
                self.lbl_status.set_text("Wysłano polecenie przeładowania do okay-panel.")
            else:
                self.lbl_status.set_text("Nie znaleziono pliku wykonywalnego okay-panel.")
        except Exception as exc:
            self.lbl_status.set_text(f"Błąd uruchamiania panelu: {exc}")

    def _apply_wallpaper(self, img_path: str):
        user_wp = _user_wallpaper_path()
        try:
            if os.path.abspath(img_path) != os.path.abspath(user_wp):
                atomic_copy_file(img_path, user_wp, mode=0o644)
            if shutil.which("swaybg"):
                subprocess.run(["pkill", "-x", "swaybg"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                subprocess.Popen(
                    ["swaybg", "-c", "#1e1e2e", "-i", user_wp, "-m", "fill"],
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    start_new_session=True
                )
            self.lbl_status.set_text(f"Ustawiono nową tapetę: {os.path.basename(img_path)}")
        except Exception as exc:
            self.lbl_status.set_text(f"Błąd zmiany tapety: {exc}")

    def _on_choose_wallpaper(self, _btn):
        dialog = Gtk.FileChooserDialog(
            title="Wybierz tapetę pulpitu",
            transient_for=self.parent_window,
            action=Gtk.FileChooserAction.OPEN
        )
        dialog.add_buttons("Anuluj", Gtk.ResponseType.CANCEL, "Otwórz", Gtk.ResponseType.OK)
        flt = Gtk.FileFilter()
        flt.set_name("Obrazy (PNG, JPG, WebP)")
        flt.add_mime_type("image/png")
        flt.add_mime_type("image/jpeg")
        flt.add_mime_type("image/webp")
        dialog.add_filter(flt)

        if dialog.run() == Gtk.ResponseType.OK:
            path = dialog.get_filename()
            dialog.destroy()
            if path:
                self._apply_wallpaper(path)
        else:
            dialog.destroy()

    def _on_reset_wallpaper(self, _btn):
        sys_wp = wallpaper_template_file()
        if sys_wp:
            self._apply_wallpaper(sys_wp)
        else:
            self.lbl_status.set_text("Nie znaleziono domyślnej tapety systemowej (read-only).")
