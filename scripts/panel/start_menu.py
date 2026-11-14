"""
OKay DE - Natywne menu startowe (Start Menu) dla okay-panel.
Zastępuje zewnętrzny launcher fuzzel, integrując się bezpośrednio z panelem
(GtkLayerShell, Catppuccin Mocha, dynamiczny kolor akcentu, animacje fade).
"""

import configparser
import os
import shlex
import shutil
import subprocess

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
gi.require_version("GtkLayerShell", "0.1")

from gi.repository import Gdk, GdkPixbuf, GLib, Gtk, GtkLayerShell, Pango

from .animations import (
    apply_animation_class,
    fade_out_and_hide,
    prepare_fade_in,
    start_fade_in,
    stop_fade,
)

_FIELD_CODES = {"%f", "%F", "%u", "%U", "%d", "%D", "%n", "%N", "%i", "%c", "%k", "%v", "%m"}

_CATEGORY_ORDER = [
    ("Wszystkie", None),
    ("Internet", {"Network", "WebBrowser", "Email", "InstantMessaging", "FileTransfer"}),
    ("Narzędzia", {"Utility", "Accessories", "TextEditor", "Archiving", "FileTools", "Calculator"}),
    ("System", {"System", "Settings", "PackageManager", "Monitor", "Security", "TerminalEmulator", "DesktopSettings"}),
    ("Multimedia", {"AudioVideo", "Audio", "Video", "Graphics", "Photography", "Player", "Recorder"}),
    ("Biuro", {"Office", "Development", "IDE", "Education", "Science", "WordProcessor", "Spreadsheet"}),
    ("Gry", {"Game", "ArcadeGame", "BoardGame", "Emulator", "RolePlaying", "StrategyGame"}),
]


def _clean_exec(raw_exec: str) -> str:
    if not raw_exec:
        return ""
    try:
        parts = shlex.split(raw_exec)
    except ValueError:
        parts = raw_exec.split()
    cleaned = [p for p in parts if p not in _FIELD_CODES]
    if not cleaned:
        return ""
    return " ".join(shlex.quote(p) for p in cleaned)


def _spawn_detached(cmd_str: str, terminal: bool = False):
    if not cmd_str or not cmd_str.strip():
        return
    try:
        args = shlex.split(cmd_str)
    except ValueError:
        args = cmd_str.split()
    if not args:
        return

    if os.sep not in args[0]:
        local_exe = os.path.expanduser(f"~/.local/bin/{args[0]}")
        if os.path.isfile(local_exe) and os.access(local_exe, os.X_OK):
            args[0] = local_exe

    if terminal:
        term = shutil.which("foot") or shutil.which("kitty") or shutil.which("alacritty") or shutil.which("xterm")
        if term:
            args = [term, "-e", *args]

    try:
        proc = subprocess.Popen(
            args,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
        GLib.child_watch_add(proc.pid, lambda _pid, _st: None)
    except Exception:
        pass


class DesktopAppScanner:
    """Skaner i cache plików .desktop zgodny z XDG."""

    def __init__(self):
        self._apps = []
        self._last_mtimes = {}
        self._pixbuf_cache = {}
        self._icon_theme = Gtk.IconTheme.get_default()

    @staticmethod
    def _app_dirs() -> list:
        dirs = []
        user_dir = os.path.join(
            os.environ.get("XDG_DATA_HOME") or os.path.expanduser("~/.local/share"),
            "applications",
        )
        dirs.append(user_dir)
        data_dirs = os.environ.get("XDG_DATA_DIRS", "/usr/local/share:/usr/share")
        for base in data_dirs.split(":"):
            base = base.strip()
            if base:
                cand = os.path.join(base, "applications")
                if cand not in dirs:
                    dirs.append(cand)
        return dirs

    def _dirs_changed(self, dirs: list) -> bool:
        cur = {}
        for d in dirs:
            try:
                cur[d] = os.path.getmtime(d) if os.path.isdir(d) else -1.0
            except OSError:
                cur[d] = -1.0
        if cur != self._last_mtimes or not self._apps:
            self._last_mtimes = cur
            return True
        return False

    @staticmethod
    def _classify_categories(raw_cats: str) -> set:
        tokens = {c.strip() for c in raw_cats.split(";") if c.strip()}
        matched = set()
        for label, group_tokens in _CATEGORY_ORDER:
            if group_tokens is not None and (tokens & group_tokens):
                matched.add(label)
        return matched

    @staticmethod
    def _parse_desktop_file(path: str, desktop_id: str) -> dict | None:
        cp = configparser.RawConfigParser(strict=False, interpolation=None)
        try:
            with open(path, "r", encoding="utf-8", errors="replace") as f:
                cp.read_file(f)
        except Exception:
            return None

        if not cp.has_section("Desktop Entry"):
            return None
        sec = cp["Desktop Entry"]

        if sec.get("Type", "").strip() != "Application":
            return None
        if sec.get("NoDisplay", "").strip().lower() == "true":
            return None
        if sec.get("Hidden", "").strip().lower() == "true":
            return None

        cur_desktops = {
            d.strip().lower()
            for d in os.environ.get("XDG_CURRENT_DESKTOP", "OKay").split(":")
            if d.strip()
        }
        only_show = sec.get("OnlyShowIn", "").strip()
        if only_show:
            allowed = {d.strip().lower() for d in only_show.split(";") if d.strip()}
            if allowed and not (allowed & cur_desktops):
                return None
        not_show = sec.get("NotShowIn", "").strip()
        if not_show:
            blocked = {d.strip().lower() for d in not_show.split(";") if d.strip()}
            if blocked & cur_desktops:
                return None

        exec_cmd = _clean_exec(sec.get("Exec", "").strip())
        if not exec_cmd:
            return None

        name = (sec.get("Name[pl]") or sec.get("Name") or desktop_id).strip()
        generic = (sec.get("GenericName[pl]") or sec.get("GenericName") or "").strip()
        comment = (sec.get("Comment[pl]") or sec.get("Comment") or "").strip()
        icon = sec.get("Icon", "").strip() or "application-x-executable"
        terminal = sec.get("Terminal", "").strip().lower() == "true"
        raw_cats = sec.get("Categories", "")
        keywords = (sec.get("Keywords[pl]") or sec.get("Keywords") or "").strip()

        app_id = desktop_id[:-8] if desktop_id.endswith(".desktop") else desktop_id
        desc = comment or generic
        cats = DesktopAppScanner._classify_categories(raw_cats)

        search_blob = " ".join([name, generic, comment, app_id, exec_cmd, keywords, raw_cats]).lower()

        return {
            "desktop_id": desktop_id,
            "app_id": app_id,
            "name": name,
            "desc": desc,
            "icon": icon,
            "cmd": exec_cmd,
            "terminal": terminal,
            "categories": cats,
            "path": path,
            "search_blob": search_blob,
        }

    def get_apps(self) -> list:
        dirs = self._app_dirs()
        if not self._dirs_changed(dirs):
            return self._apps

        seen_ids = set()
        found = []
        for d in dirs:
            if not os.path.isdir(d):
                continue
            try:
                entries = sorted(os.listdir(d))
            except OSError:
                continue
            for fname in entries:
                if not fname.endswith(".desktop"):
                    continue
                if fname in seen_ids:
                    continue
                seen_ids.add(fname)
                full_path = os.path.join(d, fname)
                if not os.path.isfile(full_path):
                    continue
                parsed = self._parse_desktop_file(full_path, fname)
                if parsed is not None:
                    found.append(parsed)

        found.sort(key=lambda a: a["name"].casefold())
        self._apps = found
        return self._apps

    def get_pixbuf(self, icon_name: str, size: int = 28) -> GdkPixbuf.Pixbuf | None:
        key = (icon_name or "application-x-executable", size)
        if key in self._pixbuf_cache:
            return self._pixbuf_cache[key]

        pb = None
        if icon_name and os.path.isabs(icon_name) and os.path.isfile(icon_name):
            try:
                pb = GdkPixbuf.Pixbuf.new_from_file_at_scale(icon_name, size, size, True)
            except Exception:
                pb = None

        if pb is None and self._icon_theme is not None:
            for candidate in (icon_name, (icon_name or "").lower(), "application-x-executable"):
                if not candidate:
                    continue
                try:
                    if self._icon_theme.has_icon(candidate):
                        pb = self._icon_theme.load_icon(
                            candidate, size, Gtk.IconLookupFlags.FORCE_SIZE
                        )
                        if pb is not None:
                            break
                except Exception:
                    pb = None

        self._pixbuf_cache[key] = pb
        return pb


class StartMenuPopover(Gtk.Window):
    """Natywne menu startowe na warstwie GtkLayerShell.Layer.TOP."""

    def __init__(
        self,
        get_panel_pos_cb,
        get_panel_mode_cb=None,
        get_pinned_apps_cb=None,
        on_toggle_pin_cb=None,
        animations_enabled_cb=None,
    ):
        super().__init__(type=Gtk.WindowType.TOPLEVEL)
        self._get_panel_pos = get_panel_pos_cb
        self._get_panel_mode = get_panel_mode_cb or (lambda: "panel")
        self._get_pinned_apps = get_pinned_apps_cb or (lambda: [])
        self._on_toggle_pin = on_toggle_pin_cb
        self._animations_enabled = animations_enabled_cb or (lambda: True)

        self._scanner = DesktopAppScanner()
        self._active_category = "Wszystkie"
        self._cat_buttons = {}
        self._context_menu_open = False
        self._row_apps = {}

        GtkLayerShell.init_for_window(self)
        GtkLayerShell.set_layer(self, GtkLayerShell.Layer.TOP)
        GtkLayerShell.set_namespace(self, "okay-menu")
        GtkLayerShell.set_keyboard_mode(self, GtkLayerShell.KeyboardMode.ON_DEMAND)
        self._sync_anchor()

        self.set_decorated(False)
        self.set_resizable(False)
        self.set_skip_taskbar_hint(True)
        self.set_skip_pager_hint(True)
        self.set_size_request(460, 510)

        screen = self.get_screen()
        visual = screen.get_rgba_visual() if screen is not None else None
        if visual is not None:
            self.set_visual(visual)
            self.set_app_paintable(True)

        self.connect("key-press-event", self._on_key_press)
        self.connect("focus-out-event", self._on_focus_out)
        self.connect("destroy", lambda _w: stop_fade(self, reset_opacity=False))

        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        root.get_style_context().add_class("popover-card")
        root.get_style_context().add_class("start-menu-card")
        self.add(root)

        # 1. Wyszukiwarka na górze
        self.search_entry = Gtk.SearchEntry()
        self.search_entry.set_placeholder_text("Szukaj aplikacji...")
        self.search_entry.get_style_context().add_class("start-search-entry")
        self.search_entry.connect("search-changed", self._on_search_changed)
        self.search_entry.connect("activate", self._on_search_activate)
        root.pack_start(self.search_entry, False, False, 0)

        # 2. Pasek przypiętych aplikacji (widoczny gdy brak wyszukiwania)
        self.pinned_section = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        lbl_pin = Gtk.Label(label="Przypięte", xalign=0.0)
        lbl_pin.get_style_context().add_class("popover-status")
        self.pinned_section.pack_start(lbl_pin, False, False, 0)

        self.pinned_flow = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        self.pinned_section.pack_start(self.pinned_flow, False, False, 0)
        root.pack_start(self.pinned_section, False, False, 0)

        # 3. Kategorie (pigułki filtrów)
        self.cat_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
        for cat_name, _tokens in _CATEGORY_ORDER:
            btn = Gtk.Button(label=cat_name)
            btn.get_style_context().add_class("start-cat-btn")
            btn.connect("clicked", self._on_category_clicked, cat_name)
            self._cat_buttons[cat_name] = btn
            self.cat_box.pack_start(btn, False, False, 0)
        root.pack_start(self.cat_box, False, False, 0)

        # 4. Przewijana lista aplikacji
        scroll = Gtk.ScrolledWindow()
        scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scroll.set_vexpand(True)

        self.app_list = Gtk.ListBox()
        self.app_list.set_selection_mode(Gtk.SelectionMode.SINGLE)
        self.app_list.set_activate_on_single_click(True)
        self.app_list.get_style_context().add_class("start-app-list")
        self.app_list.connect("row-activated", self._on_row_activated)
        scroll.add(self.app_list)
        root.pack_start(scroll, True, True, 0)

        # 5. Stopka (użytkownik + szybkie akcje + zasilanie)
        root.pack_start(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL), False, False, 0)
        footer = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)

        user_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        user_box.pack_start(
            Gtk.Image.new_from_icon_name("avatar-default-symbolic", Gtk.IconSize.MENU),
            False,
            False,
            0,
        )
        lbl_user = Gtk.Label(label=GLib.get_user_name() or "użytkownik", xalign=0.0)
        lbl_user.get_style_context().add_class("start-user-label")
        user_box.pack_start(lbl_user, False, False, 0)
        footer.pack_start(user_box, True, True, 0)

        for icon_name, tip, cmd in (
            ("system-file-manager-symbolic", "Menedżer plików", "pcmanfm"),
            ("utilities-terminal-symbolic", "Terminal (foot)", "foot"),
            ("preferences-system-symbolic", "Ustawienia OKay DE", "okay-settings"),
        ):
            b = Gtk.Button()
            b.get_style_context().add_class("qs-icon-btn")
            b.set_tooltip_text(tip)
            b.add(Gtk.Image.new_from_icon_name(icon_name, Gtk.IconSize.MENU))
            b.connect("clicked", self._on_quick_cmd, cmd)
            footer.pack_start(b, False, False, 0)

        self.btn_power = Gtk.Button()
        self.btn_power.get_style_context().add_class("qs-icon-btn")
        self.btn_power.get_style_context().add_class("start-power-btn")
        self.btn_power.set_tooltip_text("Zasilanie i sesja")
        self.btn_power.add(Gtk.Image.new_from_icon_name("system-shutdown-symbolic", Gtk.IconSize.MENU))
        self.btn_power.connect("clicked", self._on_power_clicked)
        footer.pack_start(self.btn_power, False, False, 0)

        root.pack_start(footer, False, False, 0)

    def _sync_anchor(self):
        pos = self._get_panel_pos()
        mode = self._get_panel_mode()
        is_top = (pos == "top")
        is_dock = (mode == "dock")

        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.TOP, is_top)
        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.BOTTOM, not is_top)
        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.LEFT, not is_dock)
        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.RIGHT, False)
        GtkLayerShell.set_margin(self, GtkLayerShell.Edge.TOP, 8 if is_top else 0)
        GtkLayerShell.set_margin(self, GtkLayerShell.Edge.BOTTOM, 0 if is_top else 8)
        GtkLayerShell.set_margin(self, GtkLayerShell.Edge.LEFT, 0 if is_dock else 8)

    def toggle(self):
        if self.get_visible():
            self.hide_menu()
        else:
            self.show_menu()

    def show_menu(self):
        anim = bool(self._animations_enabled())
        apply_animation_class(self, anim)
        self._sync_anchor()
        self._active_category = "Wszystkie"
        self.search_entry.set_text("")
        self._populate_pinned()
        self._refresh_category_buttons()
        self._populate_apps()

        prepare_fade_in(self, anim)
        self.show_all()
        self.pinned_section.set_visible(bool(self.pinned_flow.get_children()))
        self._update_category_visibility()
        self.present()
        self.search_entry.grab_focus()
        start_fade_in(self, anim, duration_ms=115)

    def hide_menu(self):
        self._context_menu_open = False
        fade_out_and_hide(self, bool(self._animations_enabled()), duration_ms=100)

    def _on_key_press(self, _w, event):
        kv = event.keyval
        if kv == Gdk.KEY_Escape:
            self.hide_menu()
            return True

        if kv in (Gdk.KEY_Down, Gdk.KEY_Up):
            rows = self.app_list.get_children()
            if not rows:
                return True
            sel = self.app_list.get_selected_row()
            idx = rows.index(sel) if sel in rows else -1
            if kv == Gdk.KEY_Down:
                nxt = 0 if idx < 0 else min(len(rows) - 1, idx + 1)
            else:
                nxt = len(rows) - 1 if idx < 0 else max(0, idx - 1)
            target = rows[nxt]
            self.app_list.select_row(target)
            target.grab_focus()
            return True

        if kv in (Gdk.KEY_Return, Gdk.KEY_KP_Enter):
            sel = self.app_list.get_selected_row()
            if sel is None:
                rows = self.app_list.get_children()
                sel = rows[0] if rows else None
            if sel is not None and sel in self._row_apps:
                self._launch_app(self._row_apps[sel])
                return True

        # Przekieruj zwykłe pisanie bezpośrednio do pola wyszukiwania
        if not self.search_entry.has_focus():
            ch = Gdk.keyval_to_unicode(kv)
            if ch and chr(ch).isprintable() and not (event.state & (Gdk.ModifierType.CONTROL_MASK | Gdk.ModifierType.MOD1_MASK)):
                self.search_entry.grab_focus()
                pos = self.search_entry.get_position()
                self.search_entry.insert_text(chr(ch), pos)
                self.search_entry.set_position(pos + 1)
                return True
        return False

    def _on_focus_out(self, _w, _ev):
        def _check():
            if not self._context_menu_open and not self.is_active():
                self.hide_menu()
            return False

        GLib.idle_add(_check)
        return False

    def _on_search_changed(self, _entry):
        query = self.search_entry.get_text().strip()
        is_searching = bool(query)
        self.pinned_section.set_visible((not is_searching) and bool(self.pinned_flow.get_children()))
        self.cat_box.set_visible(not is_searching)
        self._populate_apps()

    def _on_search_activate(self, _entry):
        sel = self.app_list.get_selected_row()
        if sel is None:
            rows = self.app_list.get_children()
            sel = rows[0] if rows else None
        if sel is not None and sel in self._row_apps:
            self._launch_app(self._row_apps[sel])

    def _on_category_clicked(self, _btn, cat_name: str):
        self._active_category = cat_name
        self._refresh_category_buttons()
        self._populate_apps()
        self.search_entry.grab_focus()

    def _update_category_visibility(self):
        apps = self._scanner.get_apps()
        present = {"Wszystkie"}
        for a in apps:
            present |= a["categories"]
        for cat_name, btn in self._cat_buttons.items():
            btn.set_visible(cat_name in present)

    def _refresh_category_buttons(self):
        for cat_name, btn in self._cat_buttons.items():
            ctx = btn.get_style_context()
            if cat_name == self._active_category:
                ctx.add_class("start-cat-active")
            else:
                ctx.remove_class("start-cat-active")

    def _populate_pinned(self):
        for child in self.pinned_flow.get_children():
            self.pinned_flow.remove(child)
            child.destroy()

        pins = self._get_pinned_apps() or []
        apps_by_id = {a.get("app_id", "").lower(): a for a in self._scanner.get_apps()}
        for pin in pins[:6]:
            if isinstance(pin, str):
                key = pin.lower().removesuffix(".desktop")
                found = apps_by_id.get(key) or apps_by_id.get(pin.lower())
                if found:
                    cmd = found["exec"]
                    name = found["name"]
                    icon = found["icon"]
                else:
                    cmd = key
                    name = key.capitalize()
                    icon = key
            elif isinstance(pin, dict):
                cmd = str(pin.get("cmd", "")).strip()
                name = str(pin.get("name", pin.get("app_id", ""))).strip()
                icon = str(pin.get("icon", "application-x-executable")).strip()
            else:
                continue
            if not cmd:
                continue

            btn = Gtk.Button()
            btn.get_style_context().add_class("start-pinned-btn")
            btn.set_tooltip_text(name)

            box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            pb = self._scanner.get_pixbuf(icon, 20)
            if pb is not None:
                box.pack_start(Gtk.Image.new_from_pixbuf(pb), False, False, 0)
            lbl = Gtk.Label(label=name)
            lbl.set_max_width_chars(11)
            lbl.set_ellipsize(Pango.EllipsizeMode.END)
            box.pack_start(lbl, False, False, 0)
            btn.add(box)
            btn.connect("clicked", self._on_quick_cmd, cmd)
            self.pinned_flow.pack_start(btn, False, False, 0)

        self.pinned_flow.show_all()

    @staticmethod
    def _fuzzy_score(query: str, app: dict) -> int:
        """
        Oblicza wynik dopasowania rozmytego (fuzzy search) dla zapytania `query`:
        - 1000+ pkt: dokładny prefiks nazwy aplikacji
        - 800+ pkt : prefiks słowa w nazwie
        - 600+ pkt : podciąg w nazwie
        - 400+ pkt : podciąg w opisie / słowach kluczowych / poleceniu
        - 100+ pkt : dopasowanie podciągu znakowego (subsequence fuzzy match) w nazwie
        Zwraca 0 gdy brak dopasowania.
        """
        q = query.strip().lower()
        if not q:
            return 1
        name_l = str(app.get("name", "")).lower()
        blob_l = str(app.get("search_blob") or f"{name_l} {app.get('desc', '')} {app.get('cmd', '')}").lower()

        if name_l == q:
            return 1500
        if name_l.startswith(q):
            return 1200 - min(100, len(name_l))
        for word in name_l.replace("-", " ").replace("_", " ").split():
            if word.startswith(q):
                return 950 - min(100, len(name_l))
        if q in name_l:
            return 750 - name_l.find(q) * 4

        terms = [t for t in q.split() if t]
        if terms and all(t in blob_l for t in terms):
            return 500

        # Subsequence fuzzy match (np. "ffx" -> "Firefox", "okst" -> "OKay Settings")
        q_clean = q.replace(" ", "")
        idx = 0
        q_len = len(q_clean)
        consec_bonus = 0
        last_match = -2
        for pos, ch in enumerate(name_l):
            if idx < q_len and ch == q_clean[idx]:
                if pos == last_match + 1:
                    consec_bonus += 18
                last_match = pos
                idx += 1
        if idx == q_len:
            return 180 + consec_bonus - min(60, len(name_l))
        return 0

    def _populate_apps(self):
        self._row_apps.clear()
        for child in self.app_list.get_children():
            self.app_list.remove(child)
            child.destroy()

        apps = self._scanner.get_apps()
        query = self.search_entry.get_text().strip().lower()

        if query:
            scored = []
            for a in apps:
                sc = self._fuzzy_score(query, a)
                if sc > 0:
                    scored.append((sc, a))
            scored.sort(key=lambda item: (-item[0], item[1]["name"].casefold()))
            filtered = [item[1] for item in scored]
        elif self._active_category != "Wszystkie":
            filtered = [a for a in apps if self._active_category in a["categories"]]
        else:
            filtered = apps

        if not filtered:
            row = Gtk.ListBoxRow()
            row.set_selectable(False)
            row.set_activatable(False)
            lbl = Gtk.Label(label="Nie znaleziono pasujących aplikacji")
            lbl.get_style_context().add_class("popover-status")
            lbl.set_margin_top(18)
            lbl.set_margin_bottom(18)
            row.add(lbl)
            self.app_list.add(row)
            self.app_list.show_all()
            return

        for app in filtered:
            row = Gtk.ListBoxRow()
            row.get_style_context().add_class("start-app-row")

            eb = Gtk.EventBox()
            eb.set_visible_window(False)
            hbox = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
            hbox.set_margin_start(6)
            hbox.set_margin_end(6)
            hbox.set_margin_top(5)
            hbox.set_margin_bottom(5)

            pb = self._scanner.get_pixbuf(app["icon"], 28)
            if pb is not None:
                hbox.pack_start(Gtk.Image.new_from_pixbuf(pb), False, False, 0)
            else:
                hbox.pack_start(
                    Gtk.Image.new_from_icon_name("application-x-executable", Gtk.IconSize.DND),
                    False,
                    False,
                    0,
                )

            vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=1)
            lbl_name = Gtk.Label(label=app["name"], xalign=0.0)
            lbl_name.get_style_context().add_class("start-app-name")
            lbl_name.set_ellipsize(Pango.EllipsizeMode.END)
            vbox.pack_start(lbl_name, False, False, 0)

            if app["desc"]:
                lbl_desc = Gtk.Label(label=app["desc"], xalign=0.0)
                lbl_desc.get_style_context().add_class("start-app-desc")
                lbl_desc.set_ellipsize(Pango.EllipsizeMode.END)
                vbox.pack_start(lbl_desc, False, False, 0)

            hbox.pack_start(vbox, True, True, 0)
            eb.add(hbox)
            eb.connect("button-press-event", self._on_row_button_press, app)
            row.add(eb)

            self._row_apps[row] = app
            self.app_list.add(row)

        self.app_list.show_all()
        first = self.app_list.get_row_at_index(0)
        if first is not None and first in self._row_apps:
            self.app_list.select_row(first)

    def _on_row_activated(self, _lb, row):
        app = self._row_apps.get(row)
        if app is not None:
            self._launch_app(app)

    def _on_row_button_press(self, _eb, event, app: dict):
        if event.button != 3:
            return False

        self._context_menu_open = True
        menu = Gtk.Menu()
        menu.connect("deactivate", lambda _m: setattr(self, "_context_menu_open", False))

        mi_run = Gtk.MenuItem(label="Uruchom")
        mi_run.connect("activate", lambda _m: self._launch_app(app))
        menu.append(mi_run)

        if self._on_toggle_pin is not None:
            pins = self._get_pinned_apps() or []
            app_norm = app["app_id"].lower()
            is_pinned = any(
                (str(p.get("app_id", "")).lower() if isinstance(p, dict) else str(p).lower().removesuffix(".desktop")) == app_norm
                for p in pins
            )
            mi_pin = Gtk.MenuItem(label="Odepnij z paska zadań" if is_pinned else "Przypnij do paska zadań")
            mi_pin.connect("activate", lambda _m: self._toggle_pin(app))
            menu.append(mi_pin)

        mi_desk = Gtk.MenuItem(label="Dodaj skrót na pulpit")
        mi_desk.connect("activate", lambda _m: self._add_to_desktop(app))
        menu.append(mi_desk)

        menu.show_all()
        menu.popup_at_pointer(event)
        return True

    def _toggle_pin(self, app: dict):
        if self._on_toggle_pin is not None:
            self._on_toggle_pin(app)
            self._populate_pinned()
            self.pinned_section.set_visible(bool(self.pinned_flow.get_children()))

    @staticmethod
    def _add_to_desktop(app: dict):
        src = app.get("path", "")
        if not src or not os.path.isfile(src):
            return
        desk_dir = os.path.expanduser("~/Desktop")
        try:
            os.makedirs(desk_dir, exist_ok=True)
            dst = os.path.join(desk_dir, os.path.basename(src))
            shutil.copy2(src, dst)
            os.chmod(dst, 0o755)
        except Exception:
            pass

    def _launch_app(self, app: dict):
        self.hide_menu()
        _spawn_detached(app["cmd"], terminal=bool(app.get("terminal", False)))

    def _on_quick_cmd(self, _btn, cmd: str):
        self.hide_menu()
        _spawn_detached(cmd, terminal=False)

    def _on_power_clicked(self, btn):
        self._context_menu_open = True
        menu = Gtk.Menu()
        menu.connect("deactivate", lambda _m: setattr(self, "_context_menu_open", False))

        actions = (
            ("Zablokuj ekran", self._act_lock),
            ("Wyloguj", self._act_logout),
            ("Uruchom ponownie", self._act_reboot),
            ("Wyłącz komputer", self._act_shutdown),
        )
        for label, cb in actions:
            mi = Gtk.MenuItem(label=label)
            mi.connect("activate", lambda _m, fn=cb: (self.hide_menu(), fn()))
            menu.append(mi)

        menu.show_all()
        menu.popup_at_widget(btn, Gdk.Gravity.NORTH, Gdk.Gravity.SOUTH, None)

    @staticmethod
    def _act_lock():
        if shutil.which("okay-lock"):
            _spawn_detached("okay-lock")
        elif shutil.which("swaylock"):
            _spawn_detached("swaylock -f -c 11111b")
        else:
            _spawn_detached("loginctl lock-session")

    @staticmethod
    def _act_logout():
        _spawn_detached("pkill -TERM -x wayfire")

    @staticmethod
    def _act_reboot():
        _spawn_detached("systemctl reboot")

    @staticmethod
    def _act_shutdown():
        _spawn_detached("systemctl poweroff")
