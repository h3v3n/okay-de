# -*- coding: utf-8 -*-

"""
Zakładka „Mysz i Touchpad” w GTK3.
Parsuje i modyfikuje sekcję [input] (parametry libinput) w ~/.config/wayfire.ini,
którą kompozytor Wayfire przeładowuje automatycznie na żywo.
"""

from gi.repository import Gtk

from .helpers import (
    ensure_user_configs,
    read_wayfire_value,
    update_wayfire_section,
    make_scrolled_page,
    make_page_header,
    make_card,
    make_setting_row,
)


def _parse_bool(val: str, default: bool = False) -> bool:
    if val is None:
        return default
    return val.strip().lower() in ("yes", "true", "1", "on")


class InputTab(Gtk.Box):
    """Widok konfiguracji myszy i touchpada (sekcja [input] w ~/.config/wayfire.ini)."""

    def __init__(self, _parent_window=None, defer_load: bool = False):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._loaded = False

        sw, self.content = make_scrolled_page()
        self.pack_start(sw, True, True, 0)

        self.content.pack_start(
            make_page_header(
                "Mysz i Touchpad",
                "Konfiguracja parametrów libinput kompozytora Wayfire (~/.config/wayfire.ini)."
            ),
            False, False, 0
        )

        # Karta ustawień libinput
        card, box = make_card("Zachowanie wskaźnika i gładzika")

        self.speed_scale = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, -1.0, 1.0, 0.05)
        self.speed_scale.set_digits(2)
        self.speed_scale.set_size_request(240, -1)
        self.speed_scale.set_value(0.0)
        self.speed_scale.set_value_pos(Gtk.PositionType.RIGHT)
        self.speed_scale.add_mark(0.0, Gtk.PositionType.BOTTOM, "0.0")
        box.pack_start(
            make_setting_row(
                "Prędkość wskaźnika (pointer_speed)",
                "Czułość kursora myszy i touchpada w zakresie od -1.0 do 1.0",
                self.speed_scale
            ),
            False, False, 0
        )

        self.switch_tap = Gtk.Switch()
        box.pack_start(
            make_setting_row(
                "Stuknięcie jako kliknięcie (tap_to_click)",
                "Włącza kliknięcie lekkim dotknięciem touchpada",
                self.switch_tap
            ),
            False, False, 0
        )

        self.switch_natural = Gtk.Switch()
        box.pack_start(
            make_setting_row(
                "Naturalne przewijanie (natural_scroll)",
                "Odwraca kierunek przewijania (jak na ekranach dotykowych)",
                self.switch_natural
            ),
            False, False, 0
        )

        # Przycisk Zapisz + Wczytaj ponownie
        footer = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        self.lbl_status = Gtk.Label(label="", xalign=0.0)
        self.lbl_status.set_line_wrap(True)
        self.lbl_status.get_style_context().add_class("row-subtitle")
        footer.pack_start(self.lbl_status, True, True, 0)

        self.btn_reload = Gtk.Button(label="Wczytaj ponownie")
        self.btn_reload.get_style_context().add_class("secondary-btn")
        self.btn_reload.connect("clicked", lambda _w: self.load_from_rc())
        footer.pack_end(self.btn_reload, False, False, 0)

        self.btn_save = Gtk.Button(label="Zapisz ustawienia")
        self.btn_save.get_style_context().add_class("accent-btn")
        self.btn_save.connect("clicked", self._on_save_clicked)
        footer.pack_end(self.btn_save, False, False, 0)

        box.pack_start(footer, False, False, 0)
        self.content.pack_start(card, False, False, 0)

        if not defer_load:
            self.ensure_loaded()

    def ensure_loaded(self):
        if not self._loaded:
            self._loaded = True
            self.load_from_rc()

    def load_from_rc(self):
        ensure_user_configs()
        speed = 0.0
        tap = True
        natural = False

        wf_speed = read_wayfire_value("input", "mouse_cursor_speed", "")
        if wf_speed == "":
            wf_speed = read_wayfire_value("input", "touchpad_cursor_speed", "")
        wf_tap = read_wayfire_value("input", "tap_to_click", "")
        wf_nat = read_wayfire_value("input", "natural_scroll", "")

        try:
            if wf_speed != "":
                speed = max(-1.0, min(1.0, float(wf_speed)))
            if wf_tap != "":
                tap = _parse_bool(wf_tap, default=True)
            if wf_nat != "":
                natural = _parse_bool(wf_nat, default=False)
            self.lbl_status.set_text("Wczytano konfigurację z ~/.config/wayfire.ini.")
        except Exception as exc:
            self.lbl_status.set_text(f"Błąd odczytu wayfire.ini ({exc}) — użyto wartości domyślnych.")

        self.speed_scale.set_value(speed)
        self.switch_tap.set_active(tap)
        self.switch_natural.set_active(natural)

    def _on_save_clicked(self, _btn):
        speed = round(self.speed_scale.get_value(), 2)
        tap = self.switch_tap.get_active()
        natural = self.switch_natural.get_active()

        ok = update_wayfire_section("input", {
            "mouse_cursor_speed": f"{speed:.2f}",
            "touchpad_cursor_speed": f"{speed:.2f}",
            "tap_to_click": "true" if tap else "false",
            "natural_scroll": "true" if natural else "false",
        })
        if ok:
            self.lbl_status.set_text(
                "Zapisano ustawienia myszy i touchpada w ~/.config/wayfire.ini (przeładowano automatycznie)."
            )
        else:
            self.lbl_status.set_text("Błąd zapisu ustawień do ~/.config/wayfire.ini.")
