# -*- coding: utf-8 -*-

"""
Lekkie animacje fade-in (ok. 60 FPS, ~144 ms) dla popoverów i menu OKay Panel.
Timery zatrzymują się natychmiast po zakończeniu lub ukryciu/zniszczeniu okna.
"""

from gi.repository import Gtk, GLib


def apply_animation_class(widget, enabled: bool) -> None:
    """Przełącza klasę CSS .no-animations na widżecie i (jeśli istnieje) jego oknie nadrzędnym."""
    if widget is None:
        return
    targets = [widget]
    if hasattr(widget, "get_toplevel"):
        top = widget.get_toplevel()
        if top is not None and top is not widget:
            targets.append(top)
    for tgt in targets:
        try:
            ctx = tgt.get_style_context()
            if enabled:
                ctx.remove_class("no-animations")
            else:
                ctx.add_class("no-animations")
        except Exception:
            pass


def _resolve_fade_target(widget):
    if widget is None:
        return None
    if isinstance(widget, Gtk.Window):
        return widget
    if hasattr(widget, "get_toplevel"):
        top = widget.get_toplevel()
        if isinstance(top, Gtk.Window):
            return top
    return widget


def stop_fade(widget, reset_opacity: bool = True) -> None:
    """Zatrzymuje aktywny timer animacji dla danego okna/menu i opcjonalnie przywraca opacity=1.0."""
    target = _resolve_fade_target(widget)
    if target is None:
        return
    tid = getattr(target, "_okay_fade_tid", 0)
    if tid:
        try:
            GLib.source_remove(tid)
        except Exception:
            pass
        target._okay_fade_tid = 0
    if reset_opacity:
        try:
            target.set_opacity(1.0)
        except Exception:
            pass


def _ensure_cleanup_hooks(target) -> None:
    if getattr(target, "_okay_fade_hooked", False):
        return
    target._okay_fade_hooked = True
    try:
        target.connect("hide", lambda _w, tgt=target: stop_fade(tgt, reset_opacity=True))
    except Exception:
        pass
    for sig in ("unrealize", "destroy"):
        try:
            target.connect(sig, lambda _w, tgt=target: stop_fade(tgt, reset_opacity=False))
        except Exception:
            pass


def prepare_fade_in(widget, enabled: bool) -> None:
    """Zeruje opacity przed pierwszym mapowaniem, aby uniknąć mignięcia pełnej jasności."""
    target = _resolve_fade_target(widget)
    if target is None:
        return
    stop_fade(target)
    _ensure_cleanup_hooks(target)
    try:
        target.set_opacity(0.0 if enabled else 1.0)
    except Exception:
        pass


def start_fade_in(widget, enabled: bool, duration_ms: int = 144, interval_ms: int = 16) -> None:
    """Płynnie podnosi window.set_opacity() co 16 ms (~60 FPS) w czasie 120-160 ms."""
    target = _resolve_fade_target(widget)
    if target is None:
        return
    stop_fade(target)
    _ensure_cleanup_hooks(target)

    if not enabled:
        try:
            target.set_opacity(1.0)
        except Exception:
            pass
        return

    steps = max(1, round(duration_ms / max(1, interval_ms)))
    state = {"step": 0}
    try:
        target.set_opacity(0.0)
    except Exception:
        return

    def _tick() -> bool:
        if getattr(target, "_okay_fade_tid", 0) == 0:
            return False
        state["step"] += 1
        progress = min(1.0, state["step"] / float(steps))
        # Krzywa ease-out dająca naturalne, szybkie pojawienie się widżetu
        alpha = 1.0 - (1.0 - progress) * (1.0 - progress)
        try:
            target.set_opacity(alpha)
        except Exception:
            target._okay_fade_tid = 0
            return False
        if state["step"] >= steps:
            target._okay_fade_tid = 0
            return False
        return True

    target._okay_fade_tid = GLib.timeout_add(interval_ms, _tick)
