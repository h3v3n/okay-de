# -*- coding: utf-8 -*-

"""
Zakładka „System i Aktualizacje” w GTK3.
Prezentuje informacje o sprzęcie oraz umożliwia aktualizację OKay DE z GitHub.
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

    def __init__(self, parent_window=None):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.parent_window = parent_window
        self._remote_sha = ""

        sw, self.content = make_scrolled_page()
        self.pack_start(sw, True, True, 0)

        self.content.pack_start(
            make_page_header(
                "System i Aktualizacje",
                "Informacje o systemie OKsavi Linux oraz aktualizacje OKay DE w przestrzeni użytkownika."
            ),
            False, False, 0
        )

        # Informacje systemowe
        card_sys, box_sys = make_card("Specyfikacja urządzenia")
        rows = [
            ("Dystrybucja", SystemHelper.get_distro_name()),
            ("Środowisko graficzne", "OKay DE (Wayland / Wayfire / GTK3)"),
            ("Jądro Linux", SystemHelper.get_kernel_release()),
            ("Procesor", SystemHelper.get_cpu_info()),
            ("Pamięć operacyjna", SystemHelper.get_memory_info()),
        ]
        for label, val in rows:
            lbl_val = Gtk.Label(label=val, xalign=1.0)
            lbl_val.set_selectable(True)
            lbl_val.get_style_context().add_class("row-subtitle")
            box_sys.pack_start(make_setting_row(label, "", lbl_val), False, False, 0)
        self.content.pack_start(card_sys, False, False, 0)

        # Aktualizacje w przestrzeni użytkownika (~/.local/share/okay-de/repo -> ./install.sh --user)
        card_upd, box_upd = make_card("Aktualizacje OKay DE (tryb użytkownika)")
        local_ver = UpdateManager.get_local_commit()
        self.lbl_version = Gtk.Label(label=f"Zainstalowany commit: {local_ver}", xalign=0.0)
        self.lbl_version.get_style_context().add_class("row-title")
        box_upd.pack_start(self.lbl_version, False, False, 0)

        self.lbl_upd_status = Gtk.Label(
            label=f"Katalog aktualizacji: {UpdateManager.user_repo_dir()} (bez uprawnień root)",
            xalign=0.0
        )
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

        self.btn_local = Gtk.Button(label="Z lokalnego folderu...")
        self.btn_local.get_style_context().add_class("secondary-btn")
        self.btn_local.connect("clicked", self._on_local_install_clicked)
        btns.pack_start(self.btn_local, False, False, 0)

        box_upd.pack_start(btns, False, False, 0)
        self.content.pack_start(card_upd, False, False, 0)

    def _set_upd_busy(self, busy: bool):
        self.btn_check.set_sensitive(not busy)
        self.btn_local.set_sensitive(not busy)
        if busy:
            self.btn_install.set_sensitive(False)

    def _on_check_clicked(self, _btn):
        self._set_upd_busy(True)
        self.lbl_upd_status.set_text("Łączenie z repozytorium GitHub (h3v3n/okay-de)...")

        def _done(info: dict):
            self._set_upd_busy(False)
            sha = info.get("sha", "")
            msg = info.get("message", "")
            date = info.get("date", "")
            local_ver = UpdateManager.get_local_commit()
            self._remote_sha = sha
            if sha and sha == local_ver:
                self.lbl_upd_status.set_text(f"Środowisko jest aktualne ({sha}, {date}): {msg}")
                self.btn_install.set_sensitive(False)
            else:
                self.lbl_upd_status.set_text(f"Dostępna aktualizacja: {sha} ({date}) — {msg}")
                self.btn_install.set_sensitive(True)

        def _err(exc):
            self._set_upd_busy(False)
            self.lbl_upd_status.set_text(f"Nie udało się sprawdzić aktualizacji: {exc}")

        run_async(UpdateManager.check_remote_commit, _done, _err)

    def _run_update(self, expected_sha: str = "", local_source_dir: str = ""):
        self._set_upd_busy(True)
        src_hint = local_source_dir or UpdateManager.user_repo_dir()
        self.lbl_upd_status.set_text(f"Pobieranie i instalacja w ~/.local ({src_hint})...")

        def _done(_out: str):
            self._set_upd_busy(False)
            self.btn_install.set_sensitive(False)
            self.lbl_version.set_text(f"Zainstalowany commit: {UpdateManager.get_local_commit()}")
            if self.parent_window is not None and hasattr(self.parent_window, "refresh_after_update"):
                self.parent_window.refresh_after_update()
            self.lbl_upd_status.set_text(
                "Aktualizacja zainstalowana w ~/.local (konfiguracja zachowana). Przeładowano labwc i okay-panel."
            )

        def _err(exc):
            self._set_upd_busy(False)
            self.btn_install.set_sensitive(bool(self._remote_sha))
            self.lbl_upd_status.set_text(f"Błąd instalacji aktualizacji: {exc}")

        run_async(
            lambda: UpdateManager.download_and_install(expected_sha, local_source_dir=local_source_dir),
            _done,
            _err,
        )

    def _on_install_clicked(self, _btn):
        self._run_update(expected_sha=self._remote_sha, local_source_dir="")

    def _on_local_install_clicked(self, _btn):
        dialog = Gtk.FileChooserDialog(
            title="Wybierz lokalny folder repozytorium OKay DE (z plikiem install.sh)",
            transient_for=self.parent_window,
            action=Gtk.FileChooserAction.SELECT_FOLDER,
        )
        dialog.add_buttons("Anuluj", Gtk.ResponseType.CANCEL, "Zainstaluj (--user)", Gtk.ResponseType.OK)
        resp = dialog.run()
        folder = dialog.get_filename()
        dialog.destroy()
        if resp == Gtk.ResponseType.OK and folder:
            self._run_update(expected_sha="", local_source_dir=folder)
