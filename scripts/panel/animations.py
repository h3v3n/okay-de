# -*- coding: utf-8 -*-

"""
Natywny silnik płynnych animacji 60/120+ FPS dla komponentów OKay DE (GTK3 / Wayland).

Kluczowe cechy:
- Synchronizacja z natywnym zegarem klatek kompozytora Wayland (`widget.add_tick_callback` / `Gdk.FrameClock` vsync)
  z automatycznym fallbackiem do `GLib.timeout_add` dla okien jeszcze niepowiązanych z ekranem.
- Przerywalne w locie przejścia (interruptible transitions) oparte na krzywych Beziera oraz tłumionej sprężynie.
- Globalny kontroler `AnimationController` obsługujący przełącznik „Reduce Motion” oraz mnożnik prędkości
  animacji (`0.5x – 2.0x`) synchronizowany na żywo z `panel.json` i `wayfire.ini`.
"""

import math
import time
from typing import Callable, Optional, Union
from gi.repository import Gtk, GLib


def _clamp(value: float, low: float, high: float) -> float:
    return max(low, min(high, float(value)))


def _ease_out_cubic(t: float) -> float:
    t = _clamp(t, 0.0, 1.0)
    inv = 1.0 - t
    return 1.0 - (inv * inv * inv)


def _ease_in_cubic(t: float) -> float:
    t = _clamp(t, 0.0, 1.0)
    return t * t * t


def _ease_spring(t: float) -> float:
    """Krytycznie tłumiona sprężyna dla responsywnych przejść UI (60/120+ Hz)."""
    t = _clamp(t, 0.0, 1.0)
    if t <= 0.0:
        return 0.0
    if t >= 1.0:
        return 1.0
    omega = 7.5
    return 1.0 - math.exp(-omega * t) * (1.0 + omega * t * 0.65)


class AnimationController:
    """
    Centralny kontroler animacji interfejsu OKay DE.
    Przechowuje globalne parametry `reduce_motion` oraz `speed_multiplier` (0.5x – 2.0x).
    """

    def __init__(self):
        self.reduce_motion: bool = False
        self.speed_multiplier: float = 1.0

    def configure(self, reduce_motion: Optional[bool] = None, speed_multiplier: Optional[float] = None):
        if reduce_motion is not None:
            self.reduce_motion = bool(reduce_motion)
        if speed_multiplier is not None:
            try:
                self.speed_multiplier = _clamp(float(speed_multiplier), 0.5, 2.0)
            except (TypeError, ValueError):
                self.speed_multiplier = 1.0

    def effective_duration_ms(self, base_duration_ms: int, local_enabled: bool = True) -> int:
        if not local_enabled or self.reduce_motion or base_duration_ms <= 0:
            return 0
        scaled = float(base_duration_ms) / max(0.5, self.speed_multiplier)
        return max(16, int(round(scaled)))


ANIMATION_CONTROLLER = AnimationController()


def configure_animations(reduce_motion: Optional[bool] = None, speed_multiplier: Optional[float] = None):
    """Aktualizuje globalny stan silnika animacji (Reduce Motion oraz mnożnik prędkości 0.5x–2.0x)."""
    ANIMATION_CONTROLLER.configure(reduce_motion=reduce_motion, speed_multiplier=speed_multiplier)


def get_animation_controller() -> AnimationController:
    return ANIMATION_CONTROLLER


def _ensure_destroy_hook(widget: Gtk.Widget):
    if getattr(widget, "_okay_anim_destroy_hooked", False):
        return
    widget._okay_anim_destroy_hooked = True

    def _on_destroy(w):
        _cancel_active_tasks(w)

    widget.connect("destroy", _on_destroy)


def _cancel_active_tasks(widget: Gtk.Widget):
    tick_id = getattr(widget, "_okay_anim_tick_id", 0)
    if tick_id:
        try:
            widget.remove_tick_callback(tick_id)
        except Exception:
            pass
        widget._okay_anim_tick_id = 0

    timer_id = getattr(widget, "_okay_anim_timer_id", 0)
    if timer_id:
        try:
            GLib.source_remove(timer_id)
        except Exception:
            pass
        widget._okay_anim_timer_id = 0

    class_timer_id = getattr(widget, "_okay_anim_class_timer_id", 0)
    if class_timer_id:
        try:
            GLib.source_remove(class_timer_id)
        except Exception:
            pass
        widget._okay_anim_class_timer_id = 0


def stop_fade(widget: Optional[Gtk.Widget], reset_opacity: bool = True):
    """Zatrzymuje aktywną animację opacity na zadanym widgecie."""
    if widget is None:
        return
    _cancel_active_tasks(widget)
    if reset_opacity:
        try:
            widget.set_opacity(1.0)
        except Exception:
            pass


def apply_animation_class(
    widget: Optional[Gtk.Widget],
    enabled_or_class: Union[bool, str] = True,
    duration_ms: int = 180,
    on_done: Optional[Callable[[], None]] = None,
):
    """
    Synchronizuje klasę `.no-animations` (gdy `enabled_or_class` jest typu `bool`)
    lub nadaje tymczasową klasę CSS wyzwalającą przejście stanu (gdy `enabled_or_class` jest `str`).
    """
    if widget is None:
        if callable(on_done):
            on_done()
        return

    ctx = widget.get_style_context()
    if isinstance(enabled_or_class, bool):
        is_enabled = enabled_or_class and (not ANIMATION_CONTROLLER.reduce_motion)
        if is_enabled:
            if ctx.has_class("no-animations"):
                ctx.remove_class("no-animations")
        else:
            if not ctx.has_class("no-animations"):
                ctx.add_class("no-animations")
        if callable(on_done):
            on_done()
        return

    css_class = str(enabled_or_class).strip()
    _ensure_destroy_hook(widget)
    old_timer = getattr(widget, "_okay_anim_class_timer_id", 0)
    if old_timer:
        try:
            GLib.source_remove(old_timer)
        except Exception:
            pass
        widget._okay_anim_class_timer_id = 0

    old_cls = getattr(widget, "_okay_anim_active_class", "")
    if old_cls and ctx.has_class(old_cls):
        ctx.remove_class(old_cls)

    eff_ms = ANIMATION_CONTROLLER.effective_duration_ms(duration_ms)
    if eff_ms <= 0:
        if callable(on_done):
            on_done()
        return

    if css_class and not ctx.has_class(css_class):
        ctx.add_class(css_class)
    widget._okay_anim_active_class = css_class

    def _finish():
        widget._okay_anim_class_timer_id = 0
        widget._okay_anim_active_class = ""
        try:
            c = widget.get_style_context()
            if css_class and c.has_class(css_class):
                c.remove_class(css_class)
        except Exception:
            pass
        if callable(on_done):
            try:
                on_done()
            except Exception:
                pass
        return False

    widget._okay_anim_class_timer_id = GLib.timeout_add(eff_ms, _finish)


def animate_property(
    widget: Optional[Gtk.Widget],
    start_val: float,
    end_val: float,
    duration_ms: int,
    setter: Callable[[float], None],
    easing: Callable[[float], float] = _ease_spring,
    on_done: Optional[Callable[[], None]] = None,
    local_enabled: bool = True,
):
    """
    Ogólna funkcja animująca dowolną właściwość liczbową w synchronizacji z `Gdk.FrameClock` (vsync).
    Wspiera płynne przerwanie w locie i zmianę kierunku animacji.
    """
    if widget is None:
        if callable(on_done):
            on_done()
        return

    _ensure_destroy_hook(widget)
    _cancel_active_tasks(widget)

    eff_ms = ANIMATION_CONTROLLER.effective_duration_ms(duration_ms, local_enabled=local_enabled)
    if eff_ms <= 0 or abs(end_val - start_val) < 0.002:
        try:
            setter(end_val)
        except Exception:
            pass
        if callable(on_done):
            on_done()
        return

    duration_us = float(eff_ms) * 1000.0
    state = {"start_us": 0.0}

    def _step_progress(now_us: float) -> bool:
        if state["start_us"] <= 0.0:
            state["start_us"] = now_us
        elapsed = max(0.0, now_us - state["start_us"])
        t = min(1.0, elapsed / duration_us)
        eased = easing(t)
        cur = start_val + (end_val - start_val) * eased
        try:
            setter(cur)
        except Exception:
            return False

        if t >= 1.0:
            widget._okay_anim_tick_id = 0
            widget._okay_anim_timer_id = 0
            try:
                setter(end_val)
            except Exception:
                pass
            if callable(on_done):
                try:
                    on_done()
                except Exception:
                    pass
            return False
        return True

    # Preferuj natywny Gdk.FrameClock (vsync 60/120/144+ Hz)
    try:
        def _tick_cb(_w, frame_clock):
            now_us = float(frame_clock.get_frame_time()) if frame_clock is not None else (time.monotonic() * 1_000_000.0)
            return _step_progress(now_us)

        tick_id = widget.add_tick_callback(_tick_cb)
        if tick_id:
            widget._okay_anim_tick_id = tick_id
            return
    except Exception:
        pass

    # Fallback do zegara monotonicznego ~120 Hz (8 ms)
    def _timer_cb():
        return _step_progress(time.monotonic() * 1_000_000.0)

    widget._okay_anim_timer_id = GLib.timeout_add(8, _timer_cb)


def fade_widget(
    widget: Optional[Gtk.Widget],
    enabled: bool = True,
    start_alpha: float = 0.0,
    end_alpha: float = 1.0,
    duration_ms: int = 160,
    interval_ms: int = 16,
    on_finish: Optional[Callable[[], None]] = None,
    easing: Callable[[float], float] = _ease_out_cubic,
):
    """
    Animuje `widget.set_opacity()` od aktualnego (lub `start_alpha`) do `end_alpha`
    w oparciu o natywny zegar klatek Waylanda.
    """
    _ = interval_ms
    if widget is None:
        if callable(on_finish):
            on_finish()
        return

    start_val = _clamp(start_alpha, 0.0, 1.0)
    end_val = _clamp(end_alpha, 0.0, 1.0)

    is_running = bool(getattr(widget, "_okay_anim_tick_id", 0) or getattr(widget, "_okay_anim_timer_id", 0))
    if is_running:
        try:
            cur = float(widget.get_opacity())
            if 0.0 <= cur <= 1.0:
                start_val = cur
        except Exception:
            pass

    try:
        widget.set_opacity(start_val)
    except Exception:
        pass

    animate_property(
        widget=widget,
        start_val=start_val,
        end_val=end_val,
        duration_ms=duration_ms,
        setter=lambda v: widget.set_opacity(_clamp(v, 0.0, 1.0)),
        easing=easing,
        on_done=on_finish,
        local_enabled=bool(enabled),
    )


def prepare_fade_in(widget: Optional[Gtk.Widget], enabled: bool = True):
    """Ustawia początkową przezroczystość przed mapowaniem okna, o ile Reduce Motion nie jest aktywne."""
    if widget is None:
        return
    _cancel_active_tasks(widget)
    if (not enabled) or ANIMATION_CONTROLLER.reduce_motion:
        try:
            widget.set_opacity(1.0)
        except Exception:
            pass
        return
    try:
        widget.set_opacity(0.0)
    except Exception:
        pass


def start_fade_in(
    widget: Optional[Gtk.Widget],
    enabled: bool = True,
    duration_ms: int = 144,
    interval_ms: int = 16,
    on_done: Optional[Callable[[], None]] = None,
):
    """Uruchamia płynne pojawienie się (fade-in + spring) widgetu/okna."""
    _ = interval_ms
    if widget is None:
        if callable(on_done):
            on_done()
        return

    is_running = bool(getattr(widget, "_okay_anim_tick_id", 0) or getattr(widget, "_okay_anim_timer_id", 0))
    try:
        cur = float(widget.get_opacity())
    except Exception:
        cur = 0.0
    if cur >= 0.98 and not is_running:
        cur = 0.0

    fade_widget(
        widget,
        enabled=bool(enabled),
        start_alpha=cur,
        end_alpha=1.0,
        duration_ms=duration_ms,
        on_finish=on_done,
        easing=_ease_spring,
    )


def start_fade_out(
    widget: Optional[Gtk.Widget],
    enabled: bool = True,
    duration_ms: int = 112,
    interval_ms: int = 16,
    on_finish: Optional[Callable[[], None]] = None,
):
    """Uruchamia płynne wygaszanie (fade-out) widgetu/okna."""
    _ = interval_ms
    if widget is None:
        if callable(on_finish):
            on_finish()
        return

    try:
        start_val = float(widget.get_opacity())
    except Exception:
        start_val = 1.0

    fade_widget(
        widget,
        enabled=bool(enabled),
        start_alpha=start_val,
        end_alpha=0.0,
        duration_ms=duration_ms,
        on_finish=on_finish,
        easing=_ease_in_cubic,
    )


def fade_out_and_hide(
    widget: Optional[Gtk.Widget],
    enabled: bool = True,
    duration_ms: int = 112,
    after_hide_cb: Optional[Callable[[], None]] = None,
    on_hidden: Optional[Callable[[], None]] = None,
):
    """
    Animuje zanikanie okna do `0.0`, a po zakończeniu wywołuje `widget.hide()`
    i przywraca docelową wartość `opacity=1.0`.
    """
    cb = after_hide_cb or on_hidden
    if widget is None:
        if callable(cb):
            cb()
        return

    if not widget.get_visible():
        stop_fade(widget, reset_opacity=True)
        if callable(cb):
            cb()
        return

    def _after_fade():
        try:
            widget.hide()
            widget.set_opacity(1.0)
        except Exception:
            pass
        if callable(cb):
            try:
                cb()
            except Exception:
                pass

    start_fade_out(
        widget,
        enabled=bool(enabled),
        duration_ms=duration_ms,
        on_finish=_after_fade,
    )
