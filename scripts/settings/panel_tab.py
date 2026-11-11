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
    make_scrolled_page,
    make_page_header,
    make_card,
    make_setting_row,
)

PANEL_CONFIG_DIR = os.path.expanduser("~/.config/okay-de")
PANEL_CONFIG_FILE = os.path.join(PANEL_CONFIG_DIR, "panel.json")
SYSTEM_PANEL_CONFIG = "/etc/xdg/okay-de/panel.json"
USER_WALLPAPER = os.path.join(PANEL_CONFIG_DIR, "wallpaper.png")
SYSTEM_WALLPAPER = "/usr/share/backgrounds/okay-de/default.png"


class PanelTab(Gtk.Box):
    """Widok konfiguracji paska zadań (panel.json) oraz pulpitu OKay DE."""

    def __init__(self, parent_window=None):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.parent_window = parent_window
        self._updating_ui = False

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
                f"Aktualny plik: {USER_WALLPAPER}",
                wp_btns
            ),
            False, False, 0
        )

        self.content.pack_start(card_desk, False, False, 0)

        self.lbl_status = Gtk.Label(label="", xalign=0.0)
        self.lbl_status.get_style_context().add_class("row-subtitle")
        self.content.pack_start(self.lbl_status, False, False, 0)

        self.load_config()

    def _read_panel_json(self) -> dict:
        data = {"icon_only": True, "show_labels": False, "position": "bottom", "pinned_apps": []}
        src = PANEL_CONFIG_FILE if os.path.exists(PANEL_CONFIG_FILE) else SYSTEM_PANEL_CONFIG
        if os.path.exists(src):
            try:
                with open(src, "r", encoding="utf-8") as f:
                    loaded = json.load(f)
                if isinstance(loaded, dict):
                    data.update(loaded)
            except Exception:
                pass
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
        return data

    def load_config(self):
        cfg = self._read_panel_json()
        self._updating_ui = True
        self.switch_icon_only.set_active(bool(cfg["icon_only"]))
        self.combo_position.set_active_id(cfg["position"])
        self._updating_ui = False

        pins = cfg.get("pinned_apps", [])
        names = ", ".join(p.get("name", p.get("app_id", "")) for p in pins if isinstance(p, dict))
        self.lbl_pins_info.set_text(
            f"Przypięte aplikacje ({len(pins)}): {names}" if pins else "Brak przypiętych aplikacji."
        )
        self.lbl_status.set_text(f"Konfiguracja wczytana z: {PANEL_CONFIG_FILE}")

    def _on_setting_changed(self, *_args):
        if self._updating_ui:
            return
        cfg = self._read_panel_json()
        icon_only = bool(self.switch_icon_only.get_active())
        pos = self.combo_position.get_active_id() or "bottom"

        cfg["icon_only"] = icon_only
        cfg["show_labels"] = not icon_only
        cfg["position"] = pos

        try:
            os.makedirs(PANEL_CONFIG_DIR, exist_ok=True)
            tmp_file = PANEL_CONFIG_FILE + ".tmp"
            with open(tmp_file, "w", encoding="utf-8") as f:
                json.dump(cfg, f, indent=2, ensure_ascii=False)
            os.replace(tmp_file, PANEL_CONFIG_FILE)
            self.lbl_status.set_text(
                f"Zapisano panel.json (tylko ikony={icon_only}, pozycja={pos}). Pasek odświeży się automatycznie."
            )
        except Exception as exc:
            self.lbl_status.set_text(f"Błąd zapisu {PANEL_CONFIG_FILE}: {exc}")

    def _on_restart_panel_clicked(self, _btn):
        try:
            panel_bin = shutil.which("okay-panel") or os.path.expanduser("~/.local/bin/okay-panel")
            if os.path.exists(panel_bin):
                subprocess.Popen(
                    [panel_bin, "--restart"],
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
        try:
            os.makedirs(PANEL_CONFIG_DIR, exist_ok=True)
            if os.path.abspath(img_path) != os.path.abspath(USER_WALLPAPER):
                shutil.copyfile(img_path, USER_WALLPAPER)
            if shutil.which("swaybg"):
                subprocess.run(["pkill", "-x", "swaybg"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                subprocess.Popen(
                    ["swaybg", "-c", "#1e1e2e", "-i", USER_WALLPAPER, "-m", "fill"],
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
        if os.path.exists(SYSTEM_WALLPAPER):
            self._apply_wallpaper(SYSTEM_WALLPAPER)
        else:
            self.lbl_status.set_text(f"Nie znaleziono domyślnej tapety w {SYSTEM_WALLPAPER}.")
