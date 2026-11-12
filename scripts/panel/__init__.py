# -*- coding: utf-8 -*-

"""
Modularne aplety paska zadań okay-panel (SNI Tray, Quick Settings, Ghost Runner).
"""

from .sni_tray import StatusNotifierTray
from .quick_settings import QuickSettingsPopover
from .ghost_widget import GhostRunnerIndicator

__all__ = [
    "StatusNotifierTray",
    "QuickSettingsPopover",
    "GhostRunnerIndicator",
]
