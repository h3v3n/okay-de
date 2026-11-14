# -*- coding: utf-8 -*-

"""
Modularne aplety paska zadań okay-panel (SNI Tray, Quick Settings, Window Preview).
"""

try:
    from .animations import (
        AnimationController,
        ANIMATION_CONTROLLER,
        get_animation_controller,
        configure_animations,
        apply_animation_class,
        prepare_fade_in,
        start_fade_in,
        start_fade_out,
        fade_out_and_hide,
        fade_widget,
        stop_fade,
    )
except Exception:
    AnimationController = None
    ANIMATION_CONTROLLER = None
    def get_animation_controller(): return None
    def configure_animations(reduce_motion=False, speed_multiplier=1.0): pass
    def apply_animation_class(widget, enabled=True): pass
    def prepare_fade_in(window, enabled=True): pass
    def start_fade_in(window, enabled=True, duration_ms=144, interval_ms=16): pass
    def start_fade_out(window, enabled=True, duration_ms=112, interval_ms=16, on_finish=None):
        if on_finish:
            on_finish()
    def fade_out_and_hide(window, enabled=True, duration_ms=112, after_hide_cb=None):
        if window is not None:
            window.hide()
        if after_hide_cb:
            after_hide_cb()
    def fade_widget(widget, enabled=True, start_alpha=0.0, end_alpha=1.0, duration_ms=160, interval_ms=16, on_finish=None):
        if on_finish:
            on_finish()
    def stop_fade(window, reset_opacity=True): pass

try:
    from .sni_tray import StatusNotifierTray
except Exception:
    StatusNotifierTray = None

try:
    from .quick_settings import QuickSettingsPopover
except Exception:
    QuickSettingsPopover = None

try:
    from .window_preview import WindowPreviewPopup
except Exception:
    WindowPreviewPopup = None

try:
    from .start_menu import StartMenuPopover
except Exception:
    StartMenuPopover = None

__all__ = [
    "StatusNotifierTray",
    "QuickSettingsPopover",
    "WindowPreviewPopup",
    "StartMenuPopover",
    "apply_animation_class",
    "prepare_fade_in",
    "start_fade_in",
    "start_fade_out",
    "fade_out_and_hide",
    "fade_widget",
    "stop_fade",
]
