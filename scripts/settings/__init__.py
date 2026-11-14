# -*- coding: utf-8 -*-

"""
Pakiet modularnych zakładek centrum sterowania OKay Settings (GTK3).
"""

from .helpers import (
    apply_settings_css,
    ensure_user_configs,
    StateManager,
    get_state_manager,
)
from .wifi_tab import WifiTab
from .display_tab import DisplayTab
from .input_tab import InputTab
from .sound_tab import SoundTab
from .panel_tab import PanelTab
from .system_tab import SystemTab

__all__ = [
    "apply_settings_css",
    "ensure_user_configs",
    "StateManager",
    "get_state_manager",
    "WifiTab",
    "DisplayTab",
    "InputTab",
    "SoundTab",
    "PanelTab",
    "SystemTab",
]
