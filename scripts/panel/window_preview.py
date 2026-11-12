# -*- coding: utf-8 -*-

"""
Lekki podgląd zgrupowanych (stackowanych) okien jednej aplikacji na hover dla okay-panel.
Wykorzystuje warstwę GtkLayerShell.Layer.TOP (Catppuccin Mocha + akcent OKsavi #00e676).
"""

import os
import gi

gi.require_version("Gtk", "3.0")
gi.require_version("GtkLayerShell", "0.1")
from gi.repository import Gtk, Gdk, GLib, Pango, GtkLayerShell

from .animations import apply_animation_class, prepare_fade_in, start_fade_in, fade_out_and_hide, stop_fade


def _clear_box(box) -> None:
    for child in box.get_children():
        box.remove(child)
        child.destroy()


class WindowPreviewPopup(Gtk.Window):
    """Wyświetla podgląd kart wszystkich otwartych okien zgrupowanej aplikacji po najechaniu myszką."""

    def __init__(self, on_activate_window_cb, on_close_window_cb,
                 get_icon_pixbuf_cb=None, animations_enabled_cb=None):
        super().__init__(type=Gtk.WindowType.TOPLEVEL)
        self.set_name("okay-window-preview")
        self.set_title("okay-window-preview")
        self._on_activate_window = on_activate_window_cb
        self._on_close_window = on_close_window_cb
        self._get_icon_pixbuf = get_icon_pixbuf_cb
        self._animations_enabled = animations_enabled_cb or (lambda: True)

        self._show_tid = 0
        self._hide_tid = 0
        self._current_group_key = ""
        self._last_anchor = None

        GtkLayerShell.init_for_window(self)
        GtkLayerShell.set_layer(self, GtkLayerShell.Layer.TOP)
        GtkLayerShell.set_namespace(self, "okay-window-preview")
        GtkLayerShell.set_keyboard_mode(self, GtkLayerShell.KeyboardMode.NONE)

        self.set_decorated(False)
        self.set_resizable(False)
        self.set_skip_taskbar_hint(True)
        self.set_skip_pager_hint(True)

        screen = self.get_screen()
        visual = screen.get_rgba_visual() if screen is not None else None
        if visual is not None:
            self.set_visual(visual)
            self.set_app_paintable(True)

        self.add_events(
            Gdk.EventMask.ENTER_NOTIFY_MASK
            | Gdk.EventMask.LEAVE_NOTIFY_MASK
            | Gdk.EventMask.BUTTON_PRESS_MASK
        )
        self.connect("enter-notify-event", self._on_enter)
        self.connect("leave-notify-event", self._on_leave)
        self.connect("destroy", self._on_destroy)

        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        outer.get_style_context().add_class("preview-card")
        self.add(outer)

        # Nagłówek: ikona + nazwa aplikacji + liczba otwartych okien
        hdr = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        self.hdr_icon = Gtk.Image.new_from_icon_name("application-x-executable", Gtk.IconSize.MENU)
        self.hdr_title = Gtk.Label(label="", xalign=0.0)
        self.hdr_title.get_style_context().add_class("preview-app-title")
        self.hdr_count = Gtk.Label(label="")
        self.hdr_count.get_style_context().add_class("preview-count-pill")
        hdr.pack_start(self.hdr_icon, False, False, 0)
        hdr.pack_start(self.hdr_title, True, True, 0)
        hdr.pack_end(self.hdr_count, False, False, 0)
        outer.pack_start(hdr, False, False, 0)

        # Kontener poziomych kart podglądu okien (z zawijaniem przy > 4 oknach)
        self.cards_flow = Gtk.FlowBox()
        self.cards_flow.set_selection_mode(Gtk.SelectionMode.NONE)
        self.cards_flow.set_activate_on_single_click(False)
        self.cards_flow.set_homogeneous(True)
        self.cards_flow.set_min_children_per_line(1)
        self.cards_flow.set_max_children_per_line(4)
        self.cards_flow.set_column_spacing(8)
        self.cards_flow.set_row_spacing(8)
        outer.pack_start(self.cards_flow, False, False, 0)

    def current_group_key(self) -> str:
        return self._current_group_key if self.get_visible() else ""

    def _cancel_timers(self) -> None:
        if self._show_tid:
            try:
                GLib.source_remove(self._show_tid)
            except Exception:
                pass
            self._show_tid = 0
        if self._hide_tid:
            try:
                GLib.source_remove(self._hide_tid)
            except Exception:
                pass
            self._hide_tid = 0

    def cancel_hide(self) -> None:
        if self._hide_tid:
            try:
                GLib.source_remove(self._hide_tid)
            except Exception:
                pass
            self._hide_tid = 0

    def schedule_show(self, anchor_btn, item: dict, pos_info_cb) -> None:
        self.cancel_hide()
        if self._show_tid:
            try:
                GLib.source_remove(self._show_tid)
            except Exception:
                pass
            self._show_tid = 0

        if self.get_visible() and self._current_group_key == item["group_key"]:
            self.refresh_group(item, pos_info_cb(anchor_btn, len(item["windows"])))
            return

        delay_ms = 0 if self.get_visible() else 110

        def _fire() -> bool:
            self._show_tid = 0
            self._show_now(anchor_btn, item, pos_info_cb(anchor_btn, len(item["windows"])))
            return False

        if delay_ms == 0:
            _fire()
        else:
            self._show_tid = GLib.timeout_add(delay_ms, _fire)

    def schedule_hide(self, delay_ms: int = 220) -> None:
        if self._show_tid:
            try:
                GLib.source_remove(self._show_tid)
            except Exception:
                pass
            self._show_tid = 0
        if self._hide_tid:
            return

        def _fire() -> bool:
            self._hide_tid = 0
            self.hide_now()
            return False

        self._hide_tid = GLib.timeout_add(delay_ms, _fire)

    def hide_now(self, immediate: bool = False) -> None:
        self._cancel_timers()
        self._current_group_key = ""
        self._last_anchor = None
        anim = False if immediate else bool(self._animations_enabled())
        fade_out_and_hide(self, anim, duration_ms=112, after_hide_cb=lambda: _clear_box(self.cards_flow))

    def _set_app_image(self, img_widget: Gtk.Image, icon_name: str, size_px: int = 16) -> None:
        if icon_name and os.path.isabs(icon_name) and os.path.exists(icon_name) and self._get_icon_pixbuf:
            try:
                pb = self._get_icon_pixbuf(icon_name, size_px)
                if pb is not None:
                    img_widget.set_from_pixbuf(pb)
                    return
            except Exception:
                pass
        fallback = icon_name if (icon_name and not os.path.isabs(icon_name)) else "application-x-executable"
        img_widget.set_from_icon_name(fallback, Gtk.IconSize.DND if size_px >= 24 else Gtk.IconSize.MENU)

    def _sync_layer_position(self, anchor_info: dict) -> None:
        pos = str(anchor_info.get("position", "bottom")).lower()
        is_top = (pos == "top")
        margin_left = int(anchor_info.get("margin_left", 16))
        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.TOP, is_top)
        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.BOTTOM, not is_top)
        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.LEFT, True)
        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.RIGHT, False)
        GtkLayerShell.set_margin(self, GtkLayerShell.Edge.TOP, 8 if is_top else 0)
        GtkLayerShell.set_margin(self, GtkLayerShell.Edge.BOTTOM, 0 if is_top else 8)
        GtkLayerShell.set_margin(self, GtkLayerShell.Edge.LEFT, max(8, margin_left))

    def _populate_cards(self, item: dict) -> None:
        icon_name = item.get("icon", "application-x-executable")
        app_name = item.get("name") or item.get("app_id") or "Aplikacja"
        wins = item.get("windows", [])

        self._set_app_image(self.hdr_icon, icon_name, 16)
        self.hdr_title.set_text(app_name)
        n = len(wins)
        word = "okna" if 2 <= (n % 10) <= 4 and not (12 <= (n % 100) <= 14) else "okien"
        self.hdr_count.set_text(f"{n} {word}")

        _clear_box(self.cards_flow)
        for idx, win in enumerate(wins, start=1):
            card = self._build_window_card(idx, win, icon_name, app_name)
            self.cards_flow.add(card)
        self.cards_flow.show_all()

    def _build_window_card(self, idx: int, win: dict, icon_name: str, app_name: str) -> Gtk.Widget:
        title = (win.get("title") or "").strip() or f"{app_name} #{idx}"
        focused = bool(win.get("focused", False))
        minimized = bool(win.get("minimized", False))

        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        box.set_size_request(164, 108)
        ctx = box.get_style_context()
        ctx.add_class("preview-win-tile")
        if focused:
            ctx.add_class("preview-win-focused")
        elif minimized:
            ctx.add_class("preview-win-minimized")

        # Górna mini-belka okna + przycisk zamknięcia
        topbar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
        topbar.get_style_context().add_class("preview-win-chrome")

        dots = Gtk.Label(label="● ● ●")
        dots.get_style_context().add_class("preview-win-dots")
        topbar.pack_start(dots, False, False, 0)

        state_lbl = Gtk.Label(label="Aktywne" if focused else ("Zmin." if minimized else f"#{idx}"))
        state_lbl.get_style_context().add_class(
            "preview-state-active" if focused else "preview-state-idle"
        )
        topbar.pack_start(state_lbl, True, True, 0)

        btn_close = Gtk.Button(label="✕")
        btn_close.get_style_context().add_class("preview-close-btn")
        btn_close.set_tooltip_text(f"Zamknij: {title}")
        btn_close.connect("clicked", lambda _b, w=win: self._handle_close(w))
        topbar.pack_end(btn_close, False, False, 0)
        box.pack_start(topbar, False, False, 0)

        # Miniatura treści okna (przycisk aktywacji z ikoną aplikacji)
        btn_body = Gtk.Button()
        btn_body.get_style_context().add_class("preview-win-body")
        btn_body.set_tooltip_text(title)
        body_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        body_box.set_valign(Gtk.Align.CENTER)
        body_box.set_halign(Gtk.Align.CENTER)

        img = Gtk.Image()
        self._set_app_image(img, icon_name, 28)
        body_box.pack_start(img, False, False, 0)

        lbl = Gtk.Label(label=title)
        lbl.set_ellipsize(Pango.EllipsizeMode.END)
        lbl.set_max_width_chars(20)
        lbl.set_xalign(0.5)
        lbl.get_style_context().add_class("preview-win-title")
        body_box.pack_start(lbl, False, False, 0)

        btn_body.add(body_box)
        btn_body.connect("clicked", lambda _b, w=win: self._handle_activate(w))
        box.pack_start(btn_body, True, True, 0)

        return box

    def _handle_activate(self, win: dict) -> None:
        self.hide_now()
        if self._on_activate_window:
            self._on_activate_window(win)

    def _handle_close(self, win: dict) -> None:
        if self._on_close_window:
            self._on_close_window(win)

    def _show_now(self, anchor_btn, item: dict, anchor_info: dict) -> None:
        self._current_group_key = item["group_key"]
        self._last_anchor = anchor_btn
        anim = bool(self._animations_enabled())
        apply_animation_class(self, anim)
        self._sync_layer_position(anchor_info)
        self._populate_cards(item)

        was_visible = self.get_visible()
        if not was_visible:
            prepare_fade_in(self, anim)
        self.resize(1, 1)
        self.show_all()
        if not was_visible:
            start_fade_in(self, anim)

    def refresh_group(self, item: dict, anchor_info: dict) -> None:
        if not self.get_visible() or self._current_group_key != item["group_key"]:
            return
        if len(item.get("windows", [])) < 2:
            self.hide_now()
            return
        self._sync_layer_position(anchor_info)
        self._populate_cards(item)
        self.resize(1, 1)

    def _on_enter(self, _w, _ev):
        self.cancel_hide()
        return False

    def _on_leave(self, _w, event):
        if getattr(event, "detail", None) == Gdk.NotifyType.INFERIOR:
            return False
        self.schedule_hide()
        return False

    def _on_destroy(self, _w):
        self._cancel_timers()
        stop_fade(self, reset_opacity=False)
