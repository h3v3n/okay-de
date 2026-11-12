# -*- coding: utf-8 -*-

"""
Zakładka „Mysz i Touchpad” w GTK3.
Parsuje i modyfikuje sekcję <libinput> w ~/.config/labwc/rc.xml,
a następnie wysyła sygnał przeładowania przez `labwc --reconfigure`.
"""

import io
import os
import shutil
import xml.etree.ElementTree as ET
from gi.repository import Gtk

from .helpers import (
    labwc_config_dir,
    labwc_template_file,
    atomic_write_bytes,
    atomic_copy_file,
    ensure_user_configs,
    read_wayfire_value,
    update_wayfire_section,
    run_async,
    run_cmd,
    make_scrolled_page,
    make_page_header,
    make_card,
    make_setting_row,
)


def _user_rc_xml() -> str:
    return os.path.join(labwc_config_dir(), "rc.xml")


def _parse_xml_bool(val: str, default: bool = False) -> bool:
    if val is None:
        return default
    return val.strip().lower() in ("yes", "true", "1", "on")


def _bool_to_labwc(val: bool) -> str:
    return "yes" if val else "no"


class InputTab(Gtk.Box):
    """Widok konfiguracji myszy i touchpada (libinput w labwc rc.xml)."""

    def __init__(self, _parent_window=None, defer_load: bool = False):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self._loaded = False

        sw, self.content = make_scrolled_page()
        self.pack_start(sw, True, True, 0)

        self.content.pack_start(
            make_page_header(
                "Mysz i Touchpad",
                "Konfiguracja parametrów libinput zapisywana w ~/.config/wayfire.ini (oraz ~/.config/labwc/rc.xml)."
            ),
            False, False, 0
        )

        # Karta ustawień libinput
        card, box = make_card("Zachowanie wskaźnika i gładzika")

        self.speed_scale = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, -1.0, 1.0, 0.05)
        self.speed_scale.set_digits(2)
        self.speed_scale.set_size_request(240, -1)
        self.speed_scale.set_value(0.0)
        self.speed_scale.set_value_pos(Gtk.PositionType.RIGHT)
        self.speed_scale.add_mark(0.0, Gtk.PositionType.BOTTOM, "0.0")
        box.pack_start(
            make_setting_row(
                "Prędkość wskaźnika (<pointerSpeed>)",
                "Czułość kursora myszy i touchpada w zakresie od -1.0 do 1.0",
                self.speed_scale
            ),
            False, False, 0
        )

        self.switch_tap = Gtk.Switch()
        box.pack_start(
            make_setting_row(
                "Stuknięcie jako kliknięcie",
                "Włącza kliknięcie lekkim dotknięciem touchpada (tap-to-click)",
                self.switch_tap
            ),
            False, False, 0
        )

        self.switch_natural = Gtk.Switch()
        box.pack_start(
            make_setting_row(
                "Naturalne przewijanie (<naturalScroll>)",
                "Odwraca kierunek przewijania (jak na ekranach dotykowych)",
                self.switch_natural
            ),
            False, False, 0
        )

        # Przycisk Zapisz + Zastosuj
        footer = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        self.lbl_status = Gtk.Label(label="", xalign=0.0)
        self.lbl_status.set_line_wrap(True)
        self.lbl_status.get_style_context().add_class("row-subtitle")
        footer.pack_start(self.lbl_status, True, True, 0)

        self.btn_reload = Gtk.Button(label="Wczytaj ponownie")
        self.btn_reload.get_style_context().add_class("secondary-btn")
        self.btn_reload.connect("clicked", lambda _w: self.load_from_rc())
        footer.pack_end(self.btn_reload, False, False, 0)

        self.btn_save = Gtk.Button(label="Zapisz ustawienia")
        self.btn_save.get_style_context().add_class("accent-btn")
        self.btn_save.connect("clicked", self._on_save_clicked)
        footer.pack_end(self.btn_save, False, False, 0)

        box.pack_start(footer, False, False, 0)
        self.content.pack_start(card, False, False, 0)

        if not defer_load:
            self.ensure_loaded()

    def ensure_loaded(self):
        if not self._loaded:
            self._loaded = True
            self.load_from_rc()

    def _resolve_source_xml(self) -> str:
        ensure_user_configs()
        user_rc = _user_rc_xml()
        if os.path.exists(user_rc):
            return user_rc
        return labwc_template_file("rc.xml")

    @staticmethod
    def _find_libinput_device(root: ET.Element):
        libinput = root.find("libinput")
        if libinput is None:
            return None, None
        devices = libinput.findall("device")
        for wanted in ("default", "touchpad", "touch"):
            for dev in devices:
                cat = (dev.get("category") or "").strip().lower()
                if cat == wanted:
                    return libinput, dev
        first = libinput.find("device")
        return libinput, first

    def load_from_rc(self):
        src = self._resolve_source_xml()
        speed = 0.0
        tap = True
        natural = False
        wf_loaded = False

        wf_speed = read_wayfire_value("input", "mouse_cursor_speed", "")
        wf_tap = read_wayfire_value("input", "tap_to_click", "")
        wf_nat = read_wayfire_value("input", "natural_scroll", "")
        if wf_speed != "" or wf_tap != "" or wf_nat != "":
            try:
                if wf_speed != "":
                    speed = max(-1.0, min(1.0, float(wf_speed)))
                if wf_tap != "":
                    tap = _parse_xml_bool(wf_tap, default=True)
                if wf_nat != "":
                    natural = _parse_xml_bool(wf_nat, default=False)
                wf_loaded = True
                self.lbl_status.set_text("Wczytano konfigurację z ~/.config/wayfire.ini.")
            except Exception:
                wf_loaded = False

        if not wf_loaded and src:
            try:
                tree = ET.parse(src)
                root = tree.getroot()
                _libinput, dev = self._find_libinput_device(root)
                if dev is not None:
                    el_speed = dev.find("pointerSpeed")
                    if el_speed is not None and el_speed.text:
                        try:
                            speed = max(-1.0, min(1.0, float(el_speed.text.strip())))
                        except ValueError:
                            pass
                    el_tap = dev.find("tap")
                    if el_tap is None:
                        el_tap = dev.find("tapToClick")
                    if el_tap is not None and el_tap.text:
                        tap = _parse_xml_bool(el_tap.text, default=True)

                    el_nat = dev.find("naturalScroll")
                    if el_nat is not None and el_nat.text:
                        natural = _parse_xml_bool(el_nat.text, default=False)
                self.lbl_status.set_text(f"Wczytano konfigurację z: {src}")
            except Exception as exc:
                self.lbl_status.set_text(f"Błąd odczytu rc.xml ({exc}) — użyto wartości domyślnych.")
        elif not wf_loaded:
            self.lbl_status.set_text("Użyto domyślnych parametrów myszy i touchpada.")

        self.speed_scale.set_value(speed)
        self.switch_tap.set_active(tap)
        self.switch_natural.set_active(natural)

    def _on_save_clicked(self, _btn):
        speed = round(self.speed_scale.get_value(), 2)
        tap = self.switch_tap.get_active()
        natural = self.switch_natural.get_active()
        user_rc = _user_rc_xml()

        wf_ok = update_wayfire_section("input", {
            "mouse_cursor_speed": f"{speed:.2f}",
            "touchpad_cursor_speed": f"{speed:.2f}",
            "tap_to_click": "true" if tap else "false",
            "natural_scroll": "true" if natural else "false",
        })

        try:
            src = self._resolve_source_xml()
            tmpl_rc = labwc_template_file("rc.xml")
            root = None
            if src:
                try:
                    tree = ET.parse(src)
                    root = tree.getroot()
                except ET.ParseError:
                    if src == user_rc:
                        atomic_copy_file(user_rc, user_rc + ".bak", mode=0o644)
                    if tmpl_rc and os.path.exists(tmpl_rc):
                        try:
                            tree = ET.parse(tmpl_rc)
                            root = tree.getroot()
                        except ET.ParseError:
                            root = None
            if root is None:
                root = ET.Element("labwc_config")
                tree = ET.ElementTree(root)

            libinput, dev = self._find_libinput_device(root)
            if libinput is None:
                libinput = ET.SubElement(root, "libinput")
            if dev is None:
                dev = ET.SubElement(libinput, "device", {"category": "default"})

            def _set_child(parent, tag: str, value: str):
                el = parent.find(tag)
                if el is None:
                    el = ET.SubElement(parent, tag)
                el.text = value

            _set_child(dev, "pointerSpeed", f"{speed:.2f}")
            _set_child(dev, "tap", _bool_to_labwc(tap))
            if dev.find("tapToClick") is not None:
                _set_child(dev, "tapToClick", _bool_to_labwc(tap))
            _set_child(dev, "naturalScroll", _bool_to_labwc(natural))

            if hasattr(ET, "indent"):
                ET.indent(tree, space="  ")

            buf = io.BytesIO()
            tree.write(buf, encoding="utf-8", xml_declaration=True)
            atomic_write_bytes(user_rc, buf.getvalue() + b"\n", mode=0o644)
        except Exception as exc:
            self.lbl_status.set_text(f"Błąd zapisu {user_rc}: {exc}")
            return

        if not shutil.which("labwc") or "wayfire" in os.environ.get("XDG_CURRENT_DESKTOP", "").lower():
            self.lbl_status.set_text("Zapisano ustawienia myszy i touchpada w ~/.config/wayfire.ini (przeładowano automatycznie).")
            return

        self.btn_save.set_sensitive(False)
        self.lbl_status.set_text("Zapisano rc.xml. Przeładowywanie labwc...")

        def _worker():
            return run_cmd(["labwc", "--reconfigure"], timeout=5)

        def _on_done(res):
            rc, _out, err = res
            self.btn_save.set_sensitive(True)
            if rc == 0:
                self.lbl_status.set_text(f"Zapisano {user_rc} i przeładowano labwc.")
            else:
                self.lbl_status.set_text(f"Zapisano rc.xml, ale `labwc --reconfigure` zwrócił błąd: {err.strip()}")

        run_async(_worker, _on_done, lambda e: (self.btn_save.set_sensitive(True), self.lbl_status.set_text(f"Błąd: {e}")))
