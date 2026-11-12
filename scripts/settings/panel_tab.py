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
from gi.repository import Gtk, Gdk

from .helpers import (
    DEFAULT_ACCENT_COLOR,
    COLOR_PRESETS,
    okay_config_dir,
    okay_template_file,
    wallpaper_template_file,
    xdg_bin_home,
    atomic_write_json,
    atomic_copy_file,
    normalize_panel_config,
    ensure_user_configs,
    sanitize_hex_color,
    compute_contrast_text,
    apply_settings_css,
    apply_window_decorations,
    update_system_accent_color,
    run_async,
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
    """Widok konfiguracji paska zadań (panel.json), kolorów akcentu oraz pulpitu OKay DE."""

    def __init__(self, parent_window=None, defer_load: bool = False,
                 on_animations_changed=None, on_accent_changed=None):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.parent_window = parent_window
        self._on_animations_changed = on_animations_changed
        self._on_accent_changed = on_accent_changed
        self._updating_ui = False
        self._loaded = False
        self._current_accent = DEFAULT_ACCENT_COLOR
        self._preset_buttons = {}

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

        # Karta personalizacji kolorów (akcent systemu, ramki okien, outline docka)
        card_accent, box_accent = make_card("Kolor akcentu i motywu systemu")

        lbl_accent_desc = Gtk.Label(
            label="Zmienia kolor wiodący w całym środowisku graficznym: ramki okien labwc, "
                  "kontur i podświetlenia taskbara / docka, przycisk menu oraz powiadomienia mako.",
            xalign=0.0
        )
        lbl_accent_desc.set_line_wrap(True)
        lbl_accent_desc.get_style_context().add_class("row-subtitle")
        box_accent.pack_start(lbl_accent_desc, False, False, 0)

        # Siatka gotowych palet kolorów
        self.flow_presets = Gtk.FlowBox()
        self.flow_presets.set_selection_mode(Gtk.SelectionMode.NONE)
        self.flow_presets.set_activate_on_single_click(False)
        self.flow_presets.set_homogeneous(True)
        self.flow_presets.set_min_children_per_line(2)
        self.flow_presets.set_max_children_per_line(5)
        self.flow_presets.set_column_spacing(8)
        self.flow_presets.set_row_spacing(8)

        for hex_val, name in COLOR_PRESETS:
            btn = Gtk.Button()
            btn.get_style_context().add_class("color-preset-btn")
            btn_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            dot = Gtk.Label()
            dot.set_markup(f"<span foreground='{hex_val}' font_desc='13'>●</span>")
            lbl_name = Gtk.Label(label=name)
            btn_box.pack_start(dot, False, False, 0)
            btn_box.pack_start(lbl_name, False, False, 0)
            btn.add(btn_box)
            btn.connect("clicked", lambda _b, h=hex_val: self._on_preset_clicked(h))
            self._preset_buttons[hex_val] = btn
            self.flow_presets.add(btn)

        box_accent.pack_start(self.flow_presets, False, False, 0)

        # Własny wybór koloru (GtkColorButton + kod hex)
        custom_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.color_button = Gtk.ColorButton()
        self.color_button.set_use_alpha(False)
        self.color_button.set_title("Wybierz własny kolor akcentu")
        self.color_button.connect("color-set", self._on_color_set)
        custom_box.pack_start(self.color_button, False, False, 0)

        self.entry_accent = Gtk.Entry()
        self.entry_accent.set_max_length(7)
        self.entry_accent.set_width_chars(8)
        self.entry_accent.set_placeholder_text("#00e676")
        self.entry_accent.connect("activate", self._on_hex_entry_apply)
        custom_box.pack_start(self.entry_accent, False, False, 0)

        self.btn_apply_hex = Gtk.Button(label="Zastosuj")
        self.btn_apply_hex.get_style_context().add_class("secondary-btn")
        self.btn_apply_hex.connect("clicked", self._on_hex_entry_apply)
        custom_box.pack_start(self.btn_apply_hex, False, False, 0)

        box_accent.pack_start(
            make_setting_row(
                "Własny kolor (RGB / Hex)",
                "Wybierz dowolny odcień z palety lub wpisz dokładny kod szesnastkowy",
                custom_box
            ),
            False, False, 0
        )

        # Pasek podglądu elementów akcentu
        self.preview_frame = Gtk.Frame()
        self.preview_frame.set_shadow_type(Gtk.ShadowType.NONE)
        self.preview_content = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=14)
        self.preview_content.set_margin_top(8)
        self.preview_content.set_margin_bottom(8)
        self.preview_content.set_margin_start(10)
        self.preview_content.set_margin_end(10)
        self.preview_frame.add(self.preview_content)

        self.lbl_preview_title = Gtk.Label(label="Podgląd motywu:")
        self.lbl_preview_title.get_style_context().add_class("row-subtitle")
        self.preview_content.pack_start(self.lbl_preview_title, False, False, 0)

        self.preview_win_mock = Gtk.Label(label="[ Aktywna ramka okna ]")
        self.preview_content.pack_start(self.preview_win_mock, False, False, 0)

        self.preview_dock_mock = Gtk.Label(label="[ Kontur docka / panelu ]")
        self.preview_content.pack_start(self.preview_dock_mock, False, False, 0)

        self.preview_badge_mock = Gtk.Label(label="[ Akcent #OK ]")
        self.preview_content.pack_start(self.preview_badge_mock, False, False, 0)

        box_accent.pack_start(self.preview_frame, False, False, 0)
        self.content.pack_start(card_accent, False, False, 0)

        # Karta stylów ramek i przycisków okien labwc
        card_window, box_window = make_card("Styl ramek i przyciski okien (labwc)")

        self.combo_buttons_style = Gtk.ComboBoxText()
        self.combo_buttons_style.append("rectangles", "Spłaszczone prostokąty (Domyślny styl OKay)")
        self.combo_buttons_style.append("circles_left", "Kropki macOS (Lewa strona / Traffic Lights)")
        self.combo_buttons_style.append("circles_right", "Kropki macOS (Prawa strona)")
        self.combo_buttons_style.append("windows", "Klasyczne przyciski (Styl Windows / X-Kwadrat-Minus)")
        self.combo_buttons_style.connect("changed", self._on_window_decorations_changed)
        box_window.pack_start(
            make_setting_row(
                "Styl przycisków paska tytułowego",
                "Wybierz kształt oraz położenie przycisków zamykania, maksymalizacji i minimalizacji",
                self.combo_buttons_style
            ),
            False, False, 0
        )

        self.combo_corner_radius = Gtk.ComboBoxText()
        self.combo_corner_radius.append("0", "0 px - Ostre narożniki 90° (Domyślny styl OKay)")
        self.combo_corner_radius.append("6", "6 px - Dyskretnie zaokrąglone")
        self.combo_corner_radius.append("10", "10 px - Zaokrąglone (Styl nowoczesny)")
        self.combo_corner_radius.append("14", "14 px - Mocno zaokrąglone (Styl macOS / GNOME)")
        self.combo_corner_radius.connect("changed", self._on_window_decorations_changed)
        box_window.pack_start(
            make_setting_row(
                "Zaokrąglenie narożników okien",
                "Promień zaokrąglenia krawędzi okien labwc (window.cornerRadius w themerc-override i rc.xml)",
                self.combo_corner_radius
            ),
            False, False, 0
        )

        self.content.pack_start(card_window, False, False, 0)

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
        btn_style = cfg.get("window_buttons_style", "rectangles")
        self.combo_buttons_style.set_active_id(btn_style)
        corner_radius = str(cfg.get("window_corner_radius", 0))
        self.combo_corner_radius.set_active_id(corner_radius)
        accent = sanitize_hex_color(cfg.get("accent_color", DEFAULT_ACCENT_COLOR))
        self._update_accent_ui(accent)
        self._updating_ui = False

        pins = cfg.get("pinned_apps", [])
        names = ", ".join(p.get("name", p.get("app_id", "")) for p in pins if isinstance(p, dict))
        self.lbl_pins_info.set_text(
            f"Przypięte aplikacje ({len(pins)}): {names}" if pins else "Brak przypiętych aplikacji."
        )
        self.lbl_status.set_text(f"Konfiguracja wczytana z: {_panel_json_path()}")

    def _update_accent_ui(self, hex_val: str):
        self._current_accent = hex_val
        self.entry_accent.set_text(hex_val)
        try:
            rgba = Gdk.RGBA()
            if rgba.parse(hex_val):
                self.color_button.set_rgba(rgba)
        except Exception:
            pass

        for preset_hex, btn in self._preset_buttons.items():
            ctx = btn.get_style_context()
            if preset_hex.lower() == hex_val.lower():
                ctx.add_class("scale-pill")
                btn.set_relief(Gtk.ReliefStyle.NORMAL)
                btn.set_state_flags(Gtk.StateFlags.CHECKED, False)
            else:
                btn.unset_state_flags(Gtk.StateFlags.CHECKED)

        contrast_text = compute_contrast_text(hex_val)
        self.preview_win_mock.set_markup(
            f"<span background='#1e1e2e' foreground='{hex_val}' font_desc='11'><b> ⯌ Ramka okna </b></span>"
        )
        self.preview_dock_mock.set_markup(
            f"<span background='#181825' foreground='{hex_val}' font_desc='11'><b> ⯁ Kontur docka </b></span>"
        )
        self.preview_badge_mock.set_markup(
            f"<span background='{hex_val}' foreground='{contrast_text}' font_desc='11'><b> OK </b></span>"
        )

    def _apply_accent_color(self, hex_val: str):
        clean = sanitize_hex_color(hex_val)
        self._update_accent_ui(clean)
        apply_settings_css(clean)
        if self._on_accent_changed is not None:
            try:
                self._on_accent_changed(clean)
            except Exception:
                pass
        self.lbl_status.set_text(f"Stosowanie koloru akcentu: {clean}...")

        def _on_done(_res):
            self.lbl_status.set_text(
                f"Zastosowano kolor akcentu: {clean} (zaktualizowano ramki labwc, pasek zadań/dock, mako, fuzzel)."
            )

        def _on_err(exc):
            self.lbl_status.set_text(f"Błąd zmiany koloru akcentu: {exc}")

        run_async(lambda: update_system_accent_color(clean), on_done=_on_done, on_error=_on_err)

    def _on_preset_clicked(self, hex_val: str):
        self._apply_accent_color(hex_val)

    def _on_color_set(self, btn):
        rgba = btn.get_rgba()
        r = max(0, min(255, int(round(rgba.red * 255))))
        g = max(0, min(255, int(round(rgba.green * 255))))
        b = max(0, min(255, int(round(rgba.blue * 255))))
        hex_val = f"#{r:02x}{g:02x}{b:02x}"
        self._apply_accent_color(hex_val)

    def _on_hex_entry_apply(self, *_args):
        raw = self.entry_accent.get_text().strip()
        clean = sanitize_hex_color(raw, "")
        if clean:
            self._apply_accent_color(clean)
        else:
            self.lbl_status.set_text(f"Niepoprawny kod koloru: '{raw}'. Podaj kod w formacie np. #00e676")

    def _on_window_decorations_changed(self, *_args):
        if self._updating_ui:
            return
        btn_style = self.combo_buttons_style.get_active_id() or "rectangles"
        try:
            corner_radius = int(self.combo_corner_radius.get_active_id() or "0")
        except Exception:
            corner_radius = 0

        self.lbl_status.set_text(f"Stosowanie stylu przycisków ({btn_style}) i zaokrąglenia ({corner_radius}px)...")

        def _worker():
            return apply_window_decorations(
                buttons_style=btn_style,
                corner_radius=corner_radius,
                accent_hex=self._current_accent
            )

        def _on_done(_res):
            self.lbl_status.set_text(
                f"Zastosowano styl okien (przyciski={btn_style}, zaokrąglenie={corner_radius}px). Przeładowano labwc."
            )

        def _on_err(exc):
            self.lbl_status.set_text(f"Błąd zmiany dekoracji okien: {exc}")

        run_async(_worker, on_done=_on_done, on_error=_on_err)

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
