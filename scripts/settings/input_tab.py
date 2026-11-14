# -*- coding: utf-8 -*-

"""
Zakładka „Mysz, Klawiatura i Skróty” w GTK3.
Parsuje i modyfikuje sekcję [input] (parametry libinput i XKB) oraz skróty klawiszowe
w sekcjach [command], [wm-actions] i [expo] w ~/.config/wayfire.ini (hot-reload).
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


SHORTCUT_DEFINITIONS = [
    ("command", "binding_terminal", "Uruchom terminal", "<super> KEY_ENTER | <ctrl> <alt> KEY_T"),
    ("command", "binding_launcher", "Launcher aplikacji (Rofi / Wofi)", "<super> KEY_SPACE | KEY_LEFTMETA"),
    ("command", "binding_files", "Menedżer plików", "<super> KEY_E"),
    ("command", "binding_settings", "Ustawienia OKay DE", "<super> KEY_I"),
    ("command", "binding_close", "Zamknij aktywne okno", "<alt> KEY_F4 | <super> KEY_Q"),
    ("command", "binding_lock", "Zablokuj ekran (okay-lock)", "<super> KEY_L"),
    ("command", "binding_clipboard", "Historia schowka (cliphist)", "<super> KEY_V"),
    ("wm-actions", "toggle_maximize", "Maksymalizuj / przywróć okno", "<super> KEY_UP"),
    ("wm-actions", "minimize", "Minimalizuj okno", "<super> KEY_H"),
    ("wm-actions", "toggle_fullscreen", "Pełny ekran (Fullscreen)", "<super> KEY_F11"),
    ("wm-actions", "toggle_showdesktop", "Pokaż pulpit", "<super> KEY_D"),
    ("expo", "toggle", "Podgląd obszarów roboczych (Expo)", "<super> KEY_TAB"),
]


class InputTab(Gtk.Box):
    """Widok konfiguracji myszy, touchpada, klawiatury i skrótów klawiszowych Wayfire."""

    def __init__(self, _parent_window=None, defer_load: bool = False):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._loaded = False
        self._shortcut_entries = {}

        sw, self.content = make_scrolled_page()
        self.pack_start(sw, True, True, 0)

        self.content.pack_start(
            make_page_header(
                "Mysz, Klawiatura i Skróty",
                "Parametry libinput, układ klawiatury XKB oraz skróty klawiszowe synchronizowane z ~/.config/wayfire.ini.",
            ),
            False,
            False,
            0,
        )

        # 1. Karta ustawień libinput (Mysz i Touchpad)
        card, box = make_card("Zachowanie wskaźnika myszy i gładzika (libinput)")

        self.speed_scale = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, -1.0, 1.0, 0.05)
        self.speed_scale.set_digits(2)
        self.speed_scale.set_size_request(240, -1)
        self.speed_scale.set_value(0.0)
        self.speed_scale.set_value_pos(Gtk.PositionType.RIGHT)
        self.speed_scale.add_mark(0.0, Gtk.PositionType.BOTTOM, "0.0")
        box.pack_start(
            make_setting_row(
                "Prędkość wskaźnika (cursor_speed)",
                "Czułość kursora myszy i touchpada w zakresie od -1.0 do 1.0",
                self.speed_scale,
            ),
            False,
            False,
            0,
        )

        self.switch_tap = Gtk.Switch()
        box.pack_start(
            make_setting_row(
                "Stuknięcie jako kliknięcie (tap_to_click)",
                "Włącza kliknięcie lekkim dotknięciem touchpada",
                self.switch_tap,
            ),
            False,
            False,
            0,
        )

        self.switch_natural = Gtk.Switch()
        box.pack_start(
            make_setting_row(
                "Naturalne przewijanie (natural_scroll)",
                "Odwraca kierunek przewijania (jak na ekranach dotykowych)",
                self.switch_natural,
            ),
            False,
            False,
            0,
        )

        self.combo_layout = Gtk.ComboBoxText()
        self.combo_layout.append("pl", "Polski (Programisty — pl)")
        self.combo_layout.append("us", "Angielski (US — us)")
        self.combo_layout.append("de", "Niemiecki (DE — de)")
        self.combo_layout.append("pl,us", "Polski + Angielski (pl,us)")
        self.combo_layout.set_active_id("pl")
        box.pack_start(
            make_setting_row(
                "Układ klawiatury (xkb_layout)",
                "Układ znaków klawiatury w sesji Wayland",
                self.combo_layout,
            ),
            False,
            False,
            0,
        )

        self.content.pack_start(card, False, False, 0)

        # 2. Karta skrótów klawiszowych Wayfire
        card_keys, box_keys = make_card("Skróty klawiszowe kompozytora Wayfire")

        for sec, key, title, default_val in SHORTCUT_DEFINITIONS:
            entry = Gtk.Entry()
            entry.set_width_chars(28)
            entry.set_text(default_val)
            self._shortcut_entries[(sec, key)] = (entry, default_val)
            box_keys.pack_start(
                make_setting_row(
                    title,
                    f"Sekcja [{sec}] → {key}",
                    entry,
                ),
                False,
                False,
                0,
            )

        footer = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        self.lbl_status = Gtk.Label(label="", xalign=0.0)
        self.lbl_status.set_line_wrap(True)
        self.lbl_status.get_style_context().add_class("row-subtitle")
        footer.pack_start(self.lbl_status, True, True, 0)

        self.btn_defaults = Gtk.Button(label="Przywróć domyślne skróty")
        self.btn_defaults.get_style_context().add_class("secondary-btn")
        self.btn_defaults.connect("clicked", self._on_restore_default_shortcuts)
        footer.pack_end(self.btn_defaults, False, False, 0)

        self.btn_reload = Gtk.Button(label="Wczytaj ponownie")
        self.btn_reload.get_style_context().add_class("secondary-btn")
        self.btn_reload.connect("clicked", lambda _w: self.load_from_rc())
        footer.pack_end(self.btn_reload, False, False, 0)

        self.btn_save = Gtk.Button(label="Zapisz ustawienia i skróty")
        self.btn_save.get_style_context().add_class("accent-btn")
        self.btn_save.connect("clicked", self._on_save_clicked)
        footer.pack_end(self.btn_save, False, False, 0)

        box_keys.pack_start(footer, False, False, 4)
        self.content.pack_start(card_keys, False, False, 0)

        if not defer_load:
            self.ensure_loaded()

    def ensure_loaded(self):
        if not self._loaded:
            self._loaded = True
            self.load_from_rc()

    def _on_restore_default_shortcuts(self, _btn):
        for (_sec, _key), (entry, default_val) in self._shortcut_entries.items():
            entry.set_text(default_val)
        self.lbl_status.set_text("Przywrócono domyślne skróty — kliknij „Zapisz ustawienia i skróty”, aby zastosować.")

    def load_from_rc(self):
        ensure_user_configs()
        speed = 0.0
        tap = True
        natural = False
        xkb = "pl"

        wf_speed = read_wayfire_value("input", "mouse_cursor_speed", "")
        if wf_speed == "":
            wf_speed = read_wayfire_value("input", "touchpad_cursor_speed", "")
        wf_tap = read_wayfire_value("input", "tap_to_click", "")
        wf_nat = read_wayfire_value("input", "natural_scroll", "")
        wf_xkb = read_wayfire_value("input", "xkb_layout", "pl")

        try:
            if wf_speed != "":
                speed = max(-1.0, min(1.0, float(wf_speed)))
            if wf_tap != "":
                tap = _parse_bool(wf_tap, default=True)
            if wf_nat != "":
                natural = _parse_bool(wf_nat, default=False)
            if wf_xkb:
                xkb = wf_xkb.strip()
            self.lbl_status.set_text("Wczytano konfigurację z ~/.config/wayfire.ini.")
        except Exception as exc:
            self.lbl_status.set_text(f"Błąd odczytu wayfire.ini ({exc}) — użyto wartości domyślnych.")

        self.speed_scale.set_value(speed)
        self.switch_tap.set_active(tap)
        self.switch_natural.set_active(natural)
        if not self.combo_layout.set_active_id(xkb):
            self.combo_layout.set_active_id("pl")

        for (sec, key), (entry, default_val) in self._shortcut_entries.items():
            val = read_wayfire_value(sec, key, default_val)
            entry.set_text(val if val else default_val)

    def _on_save_clicked(self, _btn):
        speed = round(self.speed_scale.get_value(), 2)
        tap = self.switch_tap.get_active()
        natural = self.switch_natural.get_active()
        xkb = self.combo_layout.get_active_id() or "pl"

        ok = update_wayfire_section("input", {
            "mouse_cursor_speed": f"{speed:.2f}",
            "touchpad_cursor_speed": f"{speed:.2f}",
            "tap_to_click": "true" if tap else "false",
            "natural_scroll": "true" if natural else "false",
            "xkb_layout": xkb,
        })

        by_sec = {}
        for (sec, key), (entry, default_val) in self._shortcut_entries.items():
            txt = entry.get_text().strip() or default_val
            by_sec.setdefault(sec, {})[key] = txt

        for sec, updates in by_sec.items():
            ok = update_wayfire_section(sec, updates) and ok

        if ok:
            self.lbl_status.set_text(
                "Zapisano ustawienia wejścia oraz skróty klawiszowe w ~/.config/wayfire.ini (przeładowano na żywo)."
            )
        else:
            self.lbl_status.set_text("Błąd zapisu ustawień do ~/.config/wayfire.ini.")
