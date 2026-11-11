# -*- coding: utf-8 -*-

"""
Zakładka „System i Aktualizacje” w GTK3.
Prezentuje informacje o sprzęcie/zRAM oraz umożliwia aktualizację OKay DE z GitHub.
"""

from gi.repository import Gtk

from .helpers import (
    SystemHelper,
    UpdateManager,
    run_async,
    make_scrolled_page,
    make_page_header,
    make_card,
    make_setting_row,
)


class SystemTab(Gtk.Box):
    """Informacje o systemie OKsavi Linux oraz aktualizator środowiska OKay DE."""

    def __init__(self, _parent_window=None):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._remote_sha = ""

        sw, self.content = make_scrolled_page()
        self.pack_start(sw, True, True, 0)

        self.content.pack_start(
            make_page_header(
                "System i Aktualizacje",
                "Informacje o systemie OKsavi Linux oraz aktualizacje OKay DE z GitHub."
            ),
            False, False, 0
        )

        # Informacje systemowe
        card_sys, box_sys = make_card("Specyfikacja urządzenia")
        rows = [
            ("Dystrybucja", SystemHelper.get_distro_name()),
            ("Środowisko graficzne", "OKay DE (Wayland / labwc / GTK3)"),
            ("Jądro Linux", SystemHelper.get_kernel_release()),
            ("Procesor", SystemHelper.get_cpu_info()),
            ("Pamięć operacyjna", SystemHelper.get_memory_info()),
            ("Pamięć zRAM", SystemHelper.get_zram_status()),
        ]
        for label, val in rows:
            lbl_val = Gtk.Label(label=val, xalign=1.0)
            lbl_val.set_selectable(True)
            lbl_val.get_style_context().add_class("row-subtitle")
            box_sys.pack_start(make_setting_row(label, "", lbl_val), False, False, 0)
        self.content.pack_start(card_sys, False, False, 0)

        # Aktualizacje GitHub
        card_upd, box_upd = make_card("Aktualizacje OKay DE")
        local_ver = UpdateManager.get_local_commit()
        self.lbl_version = Gtk.Label(label=f"Zainstalowany commit: {local_ver}", xalign=0.0)
        self.lbl_version.get_style_context().add_class("row-title")
        box_upd.pack_start(self.lbl_version, False, False, 0)

        self.lbl_upd_status = Gtk.Label(label="Kliknij „Sprawdź aktualizacje”, aby porównać z gałęzią main.", xalign=0.0)
        self.lbl_upd_status.set_line_wrap(True)
        self.lbl_upd_status.get_style_context().add_class("row-subtitle")
        box_upd.pack_start(self.lbl_upd_status, False, False, 0)

        btns = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.btn_check = Gtk.Button(label="Sprawdź aktualizacje")
        self.btn_check.get_style_context().add_class("secondary-btn")
        self.btn_check.connect("clicked", self._on_check_clicked)
        btns.pack_start(self.btn_check, False, False, 0)

        self.btn_install = Gtk.Button(label="Pobierz i zainstaluj")
        self.btn_install.get_style_context().add_class("accent-btn")
        self.btn_install.set_sensitive(False)
        self.btn_install.connect("clicked", self._on_install_clicked)
        btns.pack_start(self.btn_install, False, False, 0)

        box_upd.pack_start(btns, False, False, 0)
        self.content.pack_start(card_upd, False, False, 0)

    def _on_check_clicked(self, _btn):
        self.btn_check.set_sensitive(False)
        self.lbl_upd_status.set_text("Łączenie z repozytorium GitHub (h3v3n/okay-de)...")

        def _done(info: dict):
            self.btn_check.set_sensitive(True)
            sha = info.get("sha", "")
            msg = info.get("message", "")
            date = info.get("date", "")
            local_ver = UpdateManager.get_local_commit()
            self._remote_sha = sha
            if sha and sha == local_ver:
                self.lbl_upd_status.set_text(f"System jest aktualny ({sha}, {date}): {msg}")
                self.btn_install.set_sensitive(False)
            else:
                self.lbl_upd_status.set_text(f"Dostępna aktualizacja: {sha} ({date}) — {msg}")
                self.btn_install.set_sensitive(True)

        def _err(exc):
            self.btn_check.set_sensitive(True)
            self.lbl_upd_status.set_text(f"Nie udało się sprawdzić aktualizacji: {exc}")

        run_async(UpdateManager.check_remote_commit, _done, _err)

    def _on_install_clicked(self, _btn):
        self.btn_check.set_sensitive(False)
        self.btn_install.set_sensitive(False)
        self.lbl_upd_status.set_text("Pobieranie i instalowanie aktualizacji...")

        def _done(_out: str):
            self.btn_check.set_sensitive(True)
            self.lbl_version.set_text(f"Zainstalowany commit: {UpdateManager.get_local_commit()}")
            self.lbl_upd_status.set_text("Aktualizacja zainstalowana pomyślnie. Przeładuj sesję lub panel.")

        def _err(exc):
            self.btn_check.set_sensitive(True)
            self.btn_install.set_sensitive(True)
            self.lbl_upd_status.set_text(f"Błąd instalacji aktualizacji: {exc}")

        run_async(lambda: UpdateManager.download_and_install(self._remote_sha), _done, _err)
