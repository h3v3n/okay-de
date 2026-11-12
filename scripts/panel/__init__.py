# -*- coding: utf-8 -*-

"""
Modularne aplety paska zadań okay-panel (SNI Tray, Quick Settings, Window Preview).
"""

try:
    from .animations import apply_animation_class, prepare_fade_in, start_fade_in, stop_fade
except Exception:
    def apply_animation_class(widget, enabled=True): pass
    def prepare_fade_in(window): pass
    def start_fade_in(window, on_finish=None): pass
    def stop_fade(window): pass

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

__all__ = [
    "StatusNotifierTray",
    "QuickSettingsPopover",
    "WindowPreviewPopup",
    "apply_animation_class",
    "prepare_fade_in",
    "start_fade_in",
    "stop_fade",
]
