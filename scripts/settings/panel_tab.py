# -*- coding: utf-8 -*-

"""
Zakładka „Wygląd, Animacje i Pasek” w GTK3.
Kompleksowe sterowanie:
- Motywem systemu (Ciemny / Jasny), kolorem akcentu i tapetą pulpitu (swaybg),
- Dekoracjami okien Wayfire SSD / GTK3 / GTK4 / libadwaita / Flatpak (styl przycisków, zaokrąglenia, przezroczystość, rozmycie Kawase),
- Silnikiem płynnych animacji 60/120+ FPS (Reduce Motion, mnożnik prędkości 0.5x–2.0x, efekty otwierania/minimalizacji, fizyka Wobbly),
- Paskiem systemowym OKay Panel (pozycja, tryb Dock/Panel, liczba wirtualnych pulpitów, widżety).
Wszystkie zmiany zapisują się atomowo i działają na żywo (hot-reload).
"""

import os
import shutil
import subprocess
from gi.repository import Gtk, Gdk, GLib

from .helpers import (
    COLOR_PRESETS,
    DEFAULT_ACCENT_COLOR,
    DEFAULT_PANEL_CONFIG,
    apply_settings_css,
    apply_wayfire_animations_and_effects,
    apply_window_decorations,
    atomic_copy_file,
    ensure_user_configs,
    get_state_manager,
    is_pixdecor_installed,
    make_card,
    make_page_header,
    make_scrolled_page,
    make_setting_row,
    normalize_panel_config,
    okay_config_dir,
    parse_hex_rgb,
    rgb_to_hex,
    sanitize_hex_color,
    update_system_accent_color,
)


class PanelTab(Gtk.Box):
    """Widok personalizacji wyglądu, dekoracji okien, silnika animacji Wayfire oraz paska OKay Panel."""

    def __init__(self, parent_window=None, defer_load: bool = False, **_kwargs):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.parent_window = parent_window
        self._loaded = not defer_load
        self._updating_ui = False
        self._preset_buttons = {}
        self._anim_debounce_id = 0
        self._radius_debounce_id = 0
        self._opacity_debounce_id = 0
        self._state = get_state_manager()

        sw, self.content = make_scrolled_page()
        self.pack_start(sw, True, True, 0)

        self.content.pack_start(
            make_page_header(
                "Wygląd, Animacje i Pasek",
                "Motyw ciemny/jasny, akcent kolorystyczny, dekoracje okien Wayfire SSD/GTK/Flatpak oraz silnik animacji 60/120+ FPS.",
            ),
            False,
            False,
            0,
        )

        # =====================================================================
        # 1. Karta: Motyw Systemu i Akcent Kolorystyczny
        # =====================================================================
        card_color, box_color = make_card("Motyw systemu i paleta akcentu")

        self.combo_theme_mode = Gtk.ComboBoxText()
        self.combo_theme_mode.append("dark", "Ciemny (OKay Dark)")
        self.combo_theme_mode.append("light", "Jasny (OKay Light)")
        self.combo_theme_mode.set_active_id("dark")
        self.combo_theme_mode.connect("changed", self._on_theme_mode_changed)
        box_color.pack_start(
            make_setting_row(
                "Motyw kolorystyczny środowiska",
                "Przełącza na żywo motyw paska, ustawień, aplikacji GTK3/GTK4/libadwaita oraz Flatpak",
                self.combo_theme_mode,
            ),
            False,
            False,
            0,
        )

        presets_flow = Gtk.FlowBox()
        presets_flow.set_valign(Gtk.Align.START)
        presets_flow.set_max_children_per_line(5)
        presets_flow.set_min_children_per_line(2)
        presets_flow.set_selection_mode(Gtk.SelectionMode.NONE)
        presets_flow.set_column_spacing(8)
        presets_flow.set_row_spacing(8)

        for hex_code, label_name in COLOR_PRESETS:
            clean_hex = sanitize_hex_color(hex_code)
            btn = Gtk.ToggleButton()
            btn.get_style_context().add_class("color-preset-btn")

            btn_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            swatch = Gtk.Label()
            swatch.set_markup(f'<span foreground="{clean_hex}" size="large">●</span>')
            lbl = Gtk.Label(label=label_name, xalign=0.0)

            btn_box.pack_start(swatch, False, False, 0)
            btn_box.pack_start(lbl, False, False, 0)
            btn.add(btn_box)

            btn.connect("toggled", self._on_preset_toggled, clean_hex)
            presets_flow.add(btn)
            self._preset_buttons[clean_hex] = btn

        box_color.pack_start(presets_flow, False, False, 4)

        self.color_btn = Gtk.ColorButton()
        self.color_btn.set_use_alpha(False)
        self.color_btn.set_title("Wybierz własny kolor akcentu OKay DE")
        self.color_btn.connect("color-set", self._on_custom_color_set)

        box_color.pack_start(
            make_setting_row(
                "Własny kolor akcentu (RGB / HEX)",
                "Aktualizuje na żywo pasek, ramki okien Wayfire, powiadomienia mako i aplikacje GTK/libadwaita",
                self.color_btn,
            ),
            False,
            False,
            2,
        )

        self.lbl_color_status = Gtk.Label(label="", xalign=0.0)
        self.lbl_color_status.set_line_wrap(True)
        self.lbl_color_status.get_style_context().add_class("row-subtitle")
        box_color.pack_start(self.lbl_color_status, False, False, 0)

        self.content.pack_start(card_color, False, False, 0)

        # =====================================================================
        # 2. Karta: Dekoracje Okien (Wayfire SSD + GTK3/GTK4/libadwaita + Flatpak)
        # =====================================================================
        card_decor, box_decor = make_card("Dekoracje okien, zaokrąglenia i efekty kompozytora")

        self.combo_btn_style = Gtk.ComboBoxText()
        self.combo_btn_style.append("rectangles", "Prostokąty z akcentem (Domyślne — po prawej)")
        self.combo_btn_style.append("circles_left", "Kolorowe kółka (Styl macOS — po lewej)")
        self.combo_btn_style.append("circles_right", "Kolorowe kółka (Po prawej stronie)")
        self.combo_btn_style.append("windows", "Klasyczne ikony (Styl Windows — po prawej)")
        self.combo_btn_style.set_active_id("rectangles")
        self.combo_btn_style.connect("changed", self._on_decor_control_changed)
        box_decor.pack_start(
            make_setting_row(
                "Styl i układ przycisków okna (Minimalizuj / Maksymalizuj / Zamknij)",
                "Wymusza spójne przyciski w ramkach Wayfire SSD, przeglądarkach oraz aplikacjach GTK i Flatpak",
                self.combo_btn_style,
            ),
            False,
            False,
            0,
        )

        self.scale_corner_radius = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0, 24, 2)
        self.scale_corner_radius.set_digits(0)
        self.scale_corner_radius.set_size_request(220, -1)
        self.scale_corner_radius.set_value_pos(Gtk.PositionType.RIGHT)
        for mark_val in (0, 8, 12, 16, 24):
            self.scale_corner_radius.add_mark(mark_val, Gtk.PositionType.BOTTOM, f"{mark_val}px")
        self.scale_corner_radius.connect("value-changed", self._on_corner_radius_changed)
        box_decor.pack_start(
            make_setting_row(
                "Zaokrąglenie rogów okien (px)",
                "Promień zaokrąglenia narożników okien, menu, powiadomień mako i nagłówków GTK/libadwaita",
                self.scale_corner_radius,
            ),
            False,
            False,
            0,
        )

        self.scale_win_opacity = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0.55, 1.00, 0.05)
        self.scale_win_opacity.set_digits(2)
        self.scale_win_opacity.set_size_request(220, -1)
        self.scale_win_opacity.set_value_pos(Gtk.PositionType.RIGHT)
        self.scale_win_opacity.add_mark(0.85, Gtk.PositionType.BOTTOM, "85%")
        self.scale_win_opacity.add_mark(1.00, Gtk.PositionType.BOTTOM, "100%")
        self.scale_win_opacity.connect("value-changed", self._on_win_opacity_changed)
        box_decor.pack_start(
            make_setting_row(
                "Przezroczystość okien (Wayfire Alpha)",
                "Domyślna nieprzezroczystość otwieranych okien aplikacji (0.55 – 1.00)",
                self.scale_win_opacity,
            ),
            False,
            False,
            0,
        )

        self.switch_blur = Gtk.Switch()
        self.switch_blur.connect("notify::active", self._on_effects_toggled)
        box_decor.pack_start(
            make_setting_row(
                "Rozmycie tła okien (Kawase Dual-Filter Blur)",
                "Aktywuje sprzętowe rozmycie za półprzezroczystymi oknami w kompozytorze Wayfire",
                self.switch_blur,
            ),
            False,
            False,
            0,
        )

        self.lbl_decor_hint = Gtk.Label(label="", xalign=0.0)
        self.lbl_decor_hint.set_line_wrap(True)
        self.lbl_decor_hint.get_style_context().add_class("row-subtitle")
        box_decor.pack_start(self.lbl_decor_hint, False, False, 0)

        self.content.pack_start(card_decor, False, False, 0)

        # =====================================================================
        # 3. Karta: Silnik Płynnych Animacji (60/120+ FPS) i Wydajność
        # =====================================================================
        card_anim, box_anim = make_card("Silnik płynnych animacji (60/120+ FPS) i wydajność")

        self.switch_reduce_motion = Gtk.Switch()
        self.switch_reduce_motion.connect("notify::active", self._on_reduce_motion_toggled)
        box_anim.pack_start(
            make_setting_row(
                "Ogranicz ruch (Reduce Motion)",
                "Natychmiast wyłącza animacje kompozytora Wayfire i interfejsu DE dla maksymalnej szybkości",
                self.switch_reduce_motion,
            ),
            False,
            False,
            0,
        )

        self.scale_anim_speed = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0.5, 2.0, 0.1)
        self.scale_anim_speed.set_digits(1)
        self.scale_anim_speed.set_size_request(220, -1)
        self.scale_anim_speed.set_value_pos(Gtk.PositionType.RIGHT)
        for spd_mark in (0.5, 1.0, 1.5, 2.0):
            self.scale_anim_speed.add_mark(spd_mark, Gtk.PositionType.BOTTOM, f"{spd_mark:.1f}x")
        self.scale_anim_speed.connect("value-changed", self._on_anim_speed_changed)
        box_anim.pack_start(
            make_setting_row(
                "Mnożnik prędkości animacji (0.5x – 2.0x)",
                "Skaluje na żywo czas trwania krzywych Beziera w Wayfire oraz zegarze klatek Wayland UI",
                self.scale_anim_speed,
            ),
            False,
            False,
            0,
        )

        self.combo_open_effect = Gtk.ComboBoxText()
        self.combo_open_effect.append("zoom", "Sprężyste powiększenie (Zoom)")
        self.combo_open_effect.append("fade", "Płynne przenikanie (Fade)")
        self.combo_open_effect.append("fire", "Płomień (Fire)")
        self.combo_open_effect.append("none", "Brak efektu")
        self.combo_open_effect.set_active_id("zoom")
        self.combo_open_effect.connect("changed", self._on_effects_toggled)
        box_anim.pack_start(
            make_setting_row(
                "Animacja otwierania i zamykania okien",
                "Efekt przejścia wykonywany przez kompozytor Wayfire przy starcie i zamykaniu aplikacji",
                self.combo_open_effect,
            ),
            False,
            False,
            0,
        )

        self.combo_min_effect = Gtk.ComboBoxText()
        self.combo_min_effect.append("squeezimize", "Zwężenie do paska zadań (Squeezimize)")
        self.combo_min_effect.append("zoom", "Pomniejszenie (Zoom)")
        self.combo_min_effect.append("fade", "Przenikanie (Fade)")
        self.combo_min_effect.append("blinds", "Żaluzje (Blinds)")
        self.combo_min_effect.set_active_id("squeezimize")
        self.combo_min_effect.connect("changed", self._on_effects_toggled)
        box_anim.pack_start(
            make_setting_row(
                "Animacja minimalizacji okien",
                "Efekt chowania okna do paska zadań OKay Panel",
                self.combo_min_effect,
            ),
            False,
            False,
            0,
        )

        self.switch_wobbly = Gtk.Switch()
        self.switch_wobbly.connect("notify::active", self._on_effects_toggled)
        box_anim.pack_start(
            make_setting_row(
                "Sprężysta fizyka okien (Wobbly Windows)",
                "Nadaje oknom elastyczną bezwładność podczas przeciągania myszą",
                self.switch_wobbly,
            ),
            False,
            False,
            0,
        )

        self.content.pack_start(card_anim, False, False, 0)

        # =====================================================================
        # 4. Karta: Pasek Systemowy (Panel / Dock) i Obszary Robocze
        # =====================================================================
        card_panel, box_panel = make_card("Pasek systemowy (OKay Panel) i obszary robocze")

        self.combo_mode = Gtk.ComboBoxText()
        self.combo_mode.append("panel", "Pełny pasek (rozciągnięty na całą szerokość)")
        self.combo_mode.append("dock", "Pływający Dock (wyśrodkowany z zaokrągleniem)")
        self.combo_mode.set_active_id("panel")
        self.combo_mode.connect("changed", self._on_panel_setting_changed)
        box_panel.pack_start(
            make_setting_row(
                "Styl paska głównego",
                "Wybierz klasyczny pasek na całą szerokość ekranu lub wyśrodkowany dock",
                self.combo_mode,
            ),
            False,
            False,
            0,
        )

        self.combo_pos = Gtk.ComboBoxText()
        self.combo_pos.append("bottom", "Na dole ekranu")
        self.combo_pos.append("top", "Na górze ekranu")
        self.combo_pos.set_active_id("bottom")
        self.combo_pos.connect("changed", self._on_panel_setting_changed)
        box_panel.pack_start(
            make_setting_row(
                "Pozycja paska na ekranie",
                "Krawędź ekranu, do której przytwierdzony jest pasek systemowy",
                self.combo_pos,
            ),
            False,
            False,
            0,
        )

        self.spin_workspaces = Gtk.SpinButton.new_with_range(1, 9, 1)
        self.spin_workspaces.set_value(3)
        self.spin_workspaces.connect("value-changed", self._on_workspaces_changed)
        box_panel.pack_start(
            make_setting_row(
                "Liczba wirtualnych pulpitów Wayfire",
                "Synchronizuje liczbę obszarów roboczych na pasku oraz w sekcji [core] vwidth",
                self.spin_workspaces,
            ),
            False,
            False,
            0,
        )

        self.switch_icon_only = Gtk.Switch()
        self.switch_icon_only.connect("notify::active", self._on_panel_setting_changed)
        box_panel.pack_start(
            make_setting_row(
                "Tylko ikony na pasku zadań",
                "Ukrywa tekstowe tytuły okien, pozostawiając kompaktowe ikony aplikacji",
                self.switch_icon_only,
            ),
            False,
            False,
            0,
        )

        self.switch_show_workspaces = Gtk.Switch()
        self.switch_show_workspaces.connect("notify::active", self._on_panel_setting_changed)
        box_panel.pack_start(
            make_setting_row(
                "Przełącznik obszarów roboczych na pasku",
                "Wyświetla przyciski pulpitów wirtualnych (1, 2, 3...) obok przycisku Start",
                self.switch_show_workspaces,
            ),
            False,
            False,
            0,
        )

        self.switch_show_sysload = Gtk.Switch()
        self.switch_show_sysload.connect("notify::active", self._on_panel_setting_changed)
        box_panel.pack_start(
            make_setting_row(
                "Monitor obciążenia CPU i RAM w zasobniku",
                "Pokazuje na żywo zużycie procesora i pamięci operacyjnej na pasku zadań",
                self.switch_show_sysload,
            ),
            False,
            False,
            0,
        )

        self.lbl_panel_status = Gtk.Label(label="", xalign=0.0)
        self.lbl_panel_status.set_line_wrap(True)
        self.lbl_panel_status.get_style_context().add_class("row-subtitle")
        box_panel.pack_start(self.lbl_panel_status, False, False, 0)

        self.content.pack_start(card_panel, False, False, 0)

        # =====================================================================
        # 5. Karta: Tapeta Pulpitu (swaybg)
        # =====================================================================
        card_wp, box_wp = make_card("Tapeta pulpitu")

        wp_btn_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.btn_choose_wp = Gtk.Button(label="Wybierz plik obrazu...")
        self.btn_choose_wp.get_style_context().add_class("accent-btn")
        self.btn_choose_wp.connect("clicked", self._on_choose_wallpaper)
        wp_btn_box.pack_start(self.btn_choose_wp, False, False, 0)

        box_wp.pack_start(
            make_setting_row(
                "Obraz tła pulpitu",
                "Kopiuje wybrany obraz PNG/JPG do ~/.config/okay-de/wallpaper i przeładowuje swaybg",
                wp_btn_box,
            ),
            False,
            False,
            0,
        )

        self.lbl_wp_status = Gtk.Label(label="", xalign=0.0)
        self.lbl_wp_status.set_line_wrap(True)
        self.lbl_wp_status.get_style_context().add_class("row-subtitle")
        box_wp.pack_start(self.lbl_wp_status, False, False, 0)

        self.content.pack_start(card_wp, False, False, 0)

        if not defer_load:
            self.load_config()

    def ensure_loaded(self):
        if not self._loaded:
            self._loaded = True
            self.load_config()

    def _sync_color_controls(self, hex_color: str):
        clean = sanitize_hex_color(hex_color)
        r, g, b = parse_hex_rgb(clean)
        rgba = Gdk.RGBA(r / 255.0, g / 255.0, b / 255.0, 1.0)
        self.color_btn.set_rgba(rgba)
        for preset_hex, btn in self._preset_buttons.items():
            btn.set_active(preset_hex == clean)

    def _refresh_decor_hint(self, cfg: dict):
        pix_ok = is_pixdecor_installed()
        style_map = {
            "rectangles": "Prostokąty z akcentem (po prawej)",
            "circles_left": "Kolorowe kółka (po lewej)",
            "circles_right": "Kolorowe kółka (po prawej)",
            "windows": "Styl Windows (po prawej)",
        }
        style_label = style_map.get(cfg.get("window_buttons_style", "rectangles"), "Prostokąty")
        radius = int(cfg.get("window_corner_radius", 12))
        if pix_ok:
            self.lbl_decor_hint.set_text(
                f"Aktywne dekoracje SSD Wayfire (pixdecor) + GTK3/GTK4/libadwaita + Flatpak: {style_label} • zaokrąglenie {radius} px."
            )
        else:
            self.lbl_decor_hint.set_text(
                f"Aktywne dekoracje SSD Wayfire (decoration) + GTK3/GTK4/libadwaita + Flatpak: {style_label} • zaokrąglenie {radius} px."
            )

    def load_config(self):
        ensure_user_configs()
        cfg = self._state.reload(notify=False)

        self._updating_ui = True
        self.combo_theme_mode.set_active_id(cfg.get("theme_mode", "dark"))
        self._sync_color_controls(cfg.get("accent_color", DEFAULT_ACCENT_COLOR))
        self.combo_btn_style.set_active_id(cfg.get("window_buttons_style", "rectangles"))
        self.scale_corner_radius.set_value(float(cfg.get("window_corner_radius", 12)))
        self.scale_win_opacity.set_value(float(cfg.get("window_opacity", 1.0)))
        self.switch_blur.set_active(bool(cfg.get("blur_enabled", False)))

        reduce_motion = bool(cfg.get("reduce_motion", False))
        self.switch_reduce_motion.set_active(reduce_motion)
        self.scale_anim_speed.set_value(float(cfg.get("animation_speed", 1.0)))
        self.scale_anim_speed.set_sensitive(not reduce_motion)
        self.combo_open_effect.set_active_id(cfg.get("open_effect", "zoom"))
        self.combo_open_effect.set_sensitive(not reduce_motion)
        self.combo_min_effect.set_active_id(cfg.get("minimize_effect", "squeezimize"))
        self.combo_min_effect.set_sensitive(not reduce_motion)
        self.switch_wobbly.set_active(bool(cfg.get("wobbly_enabled", False)))
        self.switch_wobbly.set_sensitive(not reduce_motion)

        self.combo_mode.set_active_id(cfg.get("mode", "panel"))
        self.combo_pos.set_active_id(cfg.get("position", "bottom"))
        self.spin_workspaces.set_value(float(cfg.get("workspace_count", 3)))
        self.switch_icon_only.set_active(bool(cfg.get("icon_only", True)))
        self.switch_show_workspaces.set_active(bool(cfg.get("show_workspaces", True)))
        self.switch_show_sysload.set_active(bool(cfg.get("show_sysload", True)))
        self._updating_ui = False

        self._refresh_decor_hint(cfg)
        self._apply_window_anim_class(not reduce_motion)

    def _apply_window_anim_class(self, animations_enabled: bool):
        toplevel = self.get_toplevel()
        if isinstance(toplevel, Gtk.Window):
            ctx = toplevel.get_style_context()
            if animations_enabled:
                ctx.remove_class("no-animations")
            else:
                if not ctx.has_class("no-animations"):
                    ctx.add_class("no-animations")

    def _on_theme_mode_changed(self, _combo):
        if self._updating_ui:
            return
        mode = self.combo_theme_mode.get_active_id() or "dark"
        cfg = self._state.update({"theme_mode": mode}, sync_wayfire=True)
        apply_settings_css(cfg.get("accent_color", DEFAULT_ACCENT_COLOR), theme_mode=mode)
        self.lbl_color_status.set_text(
            f"Zastosowano motyw {'Jasny (Light)' if mode == 'light' else 'Ciemny (Dark)'} w całym środowisku i Flatpaku."
        )

    def _on_preset_toggled(self, button: Gtk.ToggleButton, hex_color: str):
        if self._updating_ui:
            return
        if not button.get_active():
            self._updating_ui = True
            button.set_active(True)
            self._updating_ui = False
            return
        self._apply_accent_color(hex_color)

    def _on_custom_color_set(self, color_btn: Gtk.ColorButton):
        if self._updating_ui:
            return
        rgba = color_btn.get_rgba()
        hex_color = rgb_to_hex(round(rgba.red * 255), round(rgba.green * 255), round(rgba.blue * 255))
        self._apply_accent_color(hex_color)

    def _apply_accent_color(self, hex_color: str):
        clean = sanitize_hex_color(hex_color)
        self._updating_ui = True
        self._sync_color_controls(clean)
        self._updating_ui = False

        res = update_system_accent_color(clean)
        cfg = self._state.reload(notify=True)
        apply_settings_css(clean, theme_mode=cfg.get("theme_mode", "dark"))
        targets = ", ".join(res.get("notes", [])) or "panel.json"
        self.lbl_color_status.set_text(f"Zastosowano kolor akcentu {clean.upper()} na żywo ({targets}).")

    def _on_decor_control_changed(self, _widget):
        if self._updating_ui:
            return
        style = self.combo_btn_style.get_active_id() or "rectangles"
        radius = int(round(self.scale_corner_radius.get_value()))
        cfg = self._state.update({
            "window_buttons_style": style,
            "window_corner_radius": radius,
            "window_rounding": radius,
        }, sync_wayfire=True)
        self._refresh_decor_hint(cfg)

    def _on_corner_radius_changed(self, _scale):
        if self._updating_ui:
            return
        if self._radius_debounce_id:
            GLib.source_remove(self._radius_debounce_id)
        self._radius_debounce_id = GLib.timeout_add(100, self._apply_debounced_radius)

    def _apply_debounced_radius(self):
        self._radius_debounce_id = 0
        self._on_decor_control_changed(None)
        return False

    def _on_win_opacity_changed(self, _scale):
        if self._updating_ui:
            return
        if self._opacity_debounce_id:
            GLib.source_remove(self._opacity_debounce_id)
        self._opacity_debounce_id = GLib.timeout_add(100, self._apply_debounced_opacity)

    def _apply_debounced_opacity(self):
        self._opacity_debounce_id = 0
        val = round(self.scale_win_opacity.get_value(), 2)
        self._state.update({"window_opacity": val}, sync_wayfire=True)
        return False

    def _on_reduce_motion_toggled(self, switch, _pspec):
        if self._updating_ui:
            return
        reduce_motion = bool(switch.get_active())
        self.scale_anim_speed.set_sensitive(not reduce_motion)
        self.combo_open_effect.set_sensitive(not reduce_motion)
        self.combo_min_effect.set_sensitive(not reduce_motion)
        self.switch_wobbly.set_sensitive(not reduce_motion)
        self._apply_window_anim_class(not reduce_motion)
        self._state.update({
            "reduce_motion": reduce_motion,
            "animations": not reduce_motion,
        }, sync_wayfire=True)

    def _on_anim_speed_changed(self, _scale):
        if self._updating_ui:
            return
        if self._anim_debounce_id:
            GLib.source_remove(self._anim_debounce_id)
        self._anim_debounce_id = GLib.timeout_add(90, self._apply_debounced_anim_speed)

    def _apply_debounced_anim_speed(self):
        self._anim_debounce_id = 0
        spd = round(self.scale_anim_speed.get_value(), 2)
        self._state.update({"animation_speed": spd}, sync_wayfire=True)
        return False

    def _on_effects_toggled(self, *_args):
        if self._updating_ui:
            return
        self._state.update({
            "blur_enabled": bool(self.switch_blur.get_active()),
            "wobbly_enabled": bool(self.switch_wobbly.get_active()),
            "open_effect": self.combo_open_effect.get_active_id() or "zoom",
            "minimize_effect": self.combo_min_effect.get_active_id() or "squeezimize",
        }, sync_wayfire=True)

    def _on_workspaces_changed(self, spin):
        if self._updating_ui:
            return
        ws = int(spin.get_value_as_int())
        self._state.update({"workspace_count": ws}, sync_wayfire=True)
        self.lbl_panel_status.set_text(f"Zaktualizowano liczbę obszarów roboczych Wayfire: {ws}.")

    def _on_panel_setting_changed(self, *_args):
        if self._updating_ui:
            return
        mode = self.combo_mode.get_active_id() or "panel"
        pos = self.combo_pos.get_active_id() or "bottom"
        icon_only = bool(self.switch_icon_only.get_active())
        show_ws = bool(self.switch_show_workspaces.get_active())
        show_load = bool(self.switch_show_sysload.get_active())

        self._state.update({
            "mode": mode,
            "floating": (mode == "dock"),
            "position": pos,
            "icon_only": icon_only,
            "show_labels": not icon_only,
            "show_workspaces": show_ws,
            "show_sysload": show_load,
        }, sync_wayfire=False)
        self.lbl_panel_status.set_text("Zapisano ustawienia paska zadań (przeładowano na żywo).")

    def _on_choose_wallpaper(self, _btn):
        dialog = Gtk.FileChooserDialog(
            title="Wybierz tapetę pulpitu",
            parent=self.parent_window,
            action=Gtk.FileChooserAction.OPEN,
        )
        dialog.add_buttons(
            "Anuluj", Gtk.ResponseType.CANCEL,
            "Ustaw tapetę", Gtk.ResponseType.OK,
        )

        flt = Gtk.FileFilter()
        flt.set_name("Obrazy (PNG, JPG, WEBP)")
        flt.add_mime_type("image/png")
        flt.add_mime_type("image/jpeg")
        flt.add_mime_type("image/webp")
        flt.add_pattern("*.png")
        flt.add_pattern("*.jpg")
        flt.add_pattern("*.jpeg")
        flt.add_pattern("*.webp")
        dialog.add_filter(flt)

        resp = dialog.run()
        selected_path = dialog.get_filename() if resp == Gtk.ResponseType.OK else None
        dialog.destroy()

        if not selected_path or not os.path.isfile(selected_path):
            return

        dest_dir = okay_config_dir()
        dest_wp = os.path.join(dest_dir, "wallpaper")
        dest_wp_png = os.path.join(dest_dir, "wallpaper.png")
        try:
            os.makedirs(dest_dir, exist_ok=True)
            atomic_copy_file(selected_path, dest_wp, mode=0o644)
            atomic_copy_file(selected_path, dest_wp_png, mode=0o644)
            if shutil.which("swaybg"):
                subprocess.run(["pkill", "-x", "swaybg"], check=False)
                subprocess.Popen(
                    ["swaybg", "-i", dest_wp, "-m", "fill"],
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    start_new_session=True,
                )
                self.lbl_wp_status.set_text(f"Ustawiono nową tapetę: {os.path.basename(selected_path)}")
            else:
                self.lbl_wp_status.set_text(
                    f"Zapisano tapetę w {dest_wp} (zainstaluj pakiet `swaybg`, aby wyświetlić tło)."
                )
        except Exception as exc:
            self.lbl_wp_status.set_text(f"Błąd kopiowania tapety: {exc}")
