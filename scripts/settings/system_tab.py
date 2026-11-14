# -*- coding: utf-8 -*-

"""
Zakładka „Prywatność i System” w GTK3.
Zawiera zarządzanie prywatnością (historia schowka cliphist, automatyczna blokada ekranu, Tryb Skupienia),
specyfikację sprzętową oraz menedżer aktualizacji z GitHub.
"""

import shutil
from gi.repository import Gtk

from .helpers import (
    SystemHelper,
    UpdateManager,
    get_state_manager,
    run_async,
    run_cmd,
    set_focus_mode,
    make_scrolled_page,
    make_page_header,
    make_card,
    make_setting_row,
)


class SystemTab(Gtk.Box):
    """Widok prywatności, informacji o systemie oraz aktualizatora OKay DE."""

    def __init__(self, parent_window=None, defer_load: bool = False, **_kwargs):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.parent_window = parent_window
        self._loaded = True
        self._remote_info = None
        self._state = get_state_manager()
        self._updating_ui = False

        sw, self.content = make_scrolled_page()
        self.pack_start(sw, True, True, 0)

        self.content.pack_start(
            make_page_header(
                "Prywatność i System",
                "Bezpieczeństwo sesji, historia schowka Wayland, informacje o sprzęcie oraz aktualizacje z GitHub.",
            ),
            False,
            False,
            0,
        )

        # 1. Karta prywatności i sesji
        card_priv, box_priv = make_card("Prywatność, blokada ekranu i powiadomienia")

        self.combo_idle = Gtk.ComboBoxText()
        self.combo_idle.append("0", "Nigdy (wyłączone)")
        self.combo_idle.append("300", "Po 5 minutach")
        self.combo_idle.append("600", "Po 10 minutach")
        self.combo_idle.append("900", "Po 15 minutach")
        self.combo_idle.append("1800", "Po 30 minutach")
        self.combo_idle.set_active_id(str(self._state.get("idle_timeout", 600)))
        if self.combo_idle.get_active_id() is None:
            self.combo_idle.set_active_id("600")
        self.combo_idle.connect("changed", self._on_idle_changed)
        box_priv.pack_start(
            make_setting_row(
                "Automatyczne wygaszanie ekranu (Wayfire Idle)",
                "Czas bezczynności przed wygaszeniem ekranu i blokadą sesji",
                self.combo_idle,
            ),
            False,
            False,
            0,
        )

        self.switch_focus = Gtk.Switch()
        self.switch_focus.set_active(bool(self._state.get("focus_mode", False)))
        self.switch_focus.connect("notify::active", self._on_focus_toggled)
        box_priv.pack_start(
            make_setting_row(
                "Tryb Skupienia (Nie przeszkadzać — mako)",
                "Wycisza wyskakujące powiadomienia na pulpicie, zachowując je w historii",
                self.switch_focus,
            ),
            False,
            False,
            0,
        )

        self.btn_clear_clip = Gtk.Button(label="Wyczyść historię schowka")
        self.btn_clear_clip.get_style_context().add_class("danger-btn")
        self.btn_clear_clip.connect("clicked", self._on_clear_clipboard)
        box_priv.pack_start(
            make_setting_row(
                "Historia schowka Wayland (cliphist)",
                "Usuwa wszystkie zapisane wpisy tekstowe i obrazy z bazy schowka",
                self.btn_clear_clip,
            ),
            False,
            False,
            0,
        )

        self.lbl_priv_status = Gtk.Label(label="", xalign=0.0)
        self.lbl_priv_status.get_style_context().add_class("row-subtitle")
        box_priv.pack_start(self.lbl_priv_status, False, False, 0)

        self.content.pack_start(card_priv, False, False, 0)

        # 2. Karta specyfikacji sprzętowej
        card_hw, box_hw = make_card("Specyfikacja sprzętowa i systemowa")

        specs = [
            ("Dystrybucja", SystemHelper.get_distro_name()),
            ("Środowisko graficzne", "OKay DE (Wayfire / Wayland)"),
            ("Jądro Linux (Kernel)", SystemHelper.get_kernel_release()),
            ("Procesor (CPU)", SystemHelper.get_cpu_info()),
            ("Pamięć operacyjna", SystemHelper.get_memory_info()),
        ]

        for label_text, value_text in specs:
            val_lbl = Gtk.Label(label=value_text, xalign=1.0)
            val_lbl.set_selectable(True)
            val_lbl.get_style_context().add_class("row-title")
            box_hw.pack_start(make_setting_row(label_text, "", val_lbl), False, False, 0)

        self.content.pack_start(card_hw, False, False, 0)

        # 3. Karta aktualizacji OKay DE
        card_up, box_up = make_card("Aktualizacje środowiska OKay DE")

        self.lbl_local_ver = Gtk.Label(
            label=f"Zainstalowana wersja lokalna: {UpdateManager.get_local_commit()}",
            xalign=0.0,
        )
        self.lbl_local_ver.get_style_context().add_class("row-title")
        box_up.pack_start(self.lbl_local_ver, False, False, 0)

        self.lbl_remote_ver = Gtk.Label(
            label="Kliknij „Sprawdź aktualizacje”, aby porównać wersję z gałęzią main na GitHubie.",
            xalign=0.0,
        )
        self.lbl_remote_ver.set_line_wrap(True)
        self.lbl_remote_ver.get_style_context().add_class("row-subtitle")
        box_up.pack_start(self.lbl_remote_ver, False, False, 0)

        btn_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        self.spinner = Gtk.Spinner()
        btn_box.pack_start(self.spinner, False, False, 0)

        self.btn_check = Gtk.Button(label="Sprawdź aktualizacje")
        self.btn_check.get_style_context().add_class("secondary-btn")
        self.btn_check.connect("clicked", self._on_check_clicked)
        btn_box.pack_start(self.btn_check, False, False, 0)

        self.btn_install = Gtk.Button(label="Pobierz i zainstaluj aktualizację")
        self.btn_install.get_style_context().add_class("accent-btn")
        self.btn_install.set_sensitive(False)
        self.btn_install.connect("clicked", self._on_install_clicked)
        btn_box.pack_start(self.btn_install, False, False, 0)

        box_up.pack_start(btn_box, False, False, 4)
        self.content.pack_start(card_up, False, False, 0)

    def ensure_loaded(self):
        return

    def _on_idle_changed(self, combo):
        if self._updating_ui:
            return
        try:
            sec = int(combo.get_active_id() or "600")
        except ValueError:
            sec = 600
        self._state.update({"idle_timeout": sec}, sync_wayfire=True)
        self.lbl_priv_status.set_text(
            "Wyłączono wygaszanie ekranu." if sec == 0 else f"Ustawiono wygaszanie ekranu po {sec // 60} min."
        )

    def _on_focus_toggled(self, switch, _pspec):
        if self._updating_ui:
            return
        active = bool(switch.get_active())
        set_focus_mode(active)
        self.lbl_priv_status.set_text(
            "Włączono Tryb Skupienia (powiadomienia wyciszone)." if active else "Wyłączono Tryb Skupienia."
        )

    def _on_clear_clipboard(self, _btn):
        def _worker():
            if shutil.which("cliphist"):
                run_cmd(["cliphist", "wipe"], timeout=4)
            if shutil.which("wl-copy"):
                run_cmd(["wl-copy", "--clear"], timeout=3)
            return True

        def _done(_res):
            self.lbl_priv_status.set_text("Wyczyszczono historię schowka Wayland.")

        run_async(_worker, _done)

    def _on_check_clicked(self, _btn):
        self.btn_check.set_sensitive(False)
        self.btn_install.set_sensitive(False)
        self.spinner.start()
        self.lbl_remote_ver.set_text("Łączenie z repozytorium GitHub (h3v3n/okay-de)...")

        def _done(info: dict):
            self.spinner.stop()
            self.btn_check.set_sensitive(True)
            self._remote_info = info
            local_sha = UpdateManager.get_local_commit()
            remote_sha = info.get("sha", "")
            msg = info.get("message", "")
            date = info.get("date", "")

            if local_sha != "nieznana" and remote_sha and local_sha == remote_sha:
                self.lbl_remote_ver.set_text(
                    f"System jest aktualny (wersja {remote_sha} z {date}: „{msg}”). Możesz mimo to przeinstalować."
                )
            else:
                self.lbl_remote_ver.set_text(
                    f"Dostępna wersja w repozytorium: {remote_sha} ({date}) — „{msg}”"
                )
            self.btn_install.set_sensitive(True)

        def _err(exc):
            self.spinner.stop()
            self.btn_check.set_sensitive(True)
            self.lbl_remote_ver.set_text(f"Błąd sprawdzania aktualizacji: {exc}")

        run_async(UpdateManager.check_remote_commit, _done, _err)

    def _on_install_clicked(self, _btn):
        self.btn_check.set_sensitive(False)
        self.btn_install.set_sensitive(False)
        self.spinner.start()
        self.lbl_remote_ver.set_text("Pobieranie i instalowanie aktualizacji OKay DE w katalogu użytkownika...")
        expected_sha = self._remote_info.get("sha", "") if isinstance(self._remote_info, dict) else ""

        def _done(_out: str):
            self.spinner.stop()
            self.btn_check.set_sensitive(True)
            self.btn_install.set_sensitive(True)
            new_local = UpdateManager.get_local_commit()
            self.lbl_local_ver.set_text(f"Zainstalowana wersja lokalna: {new_local}")
            self.lbl_remote_ver.set_text(
                "Aktualizacja zakończona pomyślnie! Pliki użytkownika i pasek zostały przeładowane."
            )

        def _err(exc):
            self.spinner.stop()
            self.btn_check.set_sensitive(True)
            self.btn_install.set_sensitive(True)
            self.lbl_remote_ver.set_text(f"Błąd instalacji aktualizacji: {exc}")

        run_async(lambda: UpdateManager.download_and_install(expected_sha=expected_sha), _done, _err)
