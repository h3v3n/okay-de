# -*- coding: utf-8 -*-

"""
Modularne aplety paska zadań okay-panel (SNI Tray, Quick Settings).
"""

from .animations import apply_animation_class, prepare_fade_in, start_fade_in, stop_fade
from .sni_tray import StatusNotifierTray
from .quick_settings import QuickSettingsPopover

__all__ = [
    "StatusNotifierTray",
    "QuickSettingsPopover",
    "apply_animation_class",
    "prepare_fade_in",
    "start_fade_in",
    "stop_fade",
]
