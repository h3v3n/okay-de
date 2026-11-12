# -*- coding: utf-8 -*-

"""
Modularne aplety paska zadań okay-panel (SNI Tray, Quick Settings).
"""

from .sni_tray import StatusNotifierTray
from .quick_settings import QuickSettingsPopover

__all__ = [
    "StatusNotifierTray",
    "QuickSettingsPopover",
]
