# -*- coding: utf-8 -*-

"""
Natywny System Tray (StatusNotifierWatcher + StatusNotifierItem + DBusMenu)
dla okay-panel oparty wyłącznie na standardowym Gio.DBus (zero zależności z AUR).
"""

import os
import time
import shutil
from gi.repository import Gtk, Gdk, GLib, Gio, GdkPixbuf

from .animations import apply_animation_class, prepare_fade_in, start_fade_in

WATCHER_BUS_NAME = "org.kde.StatusNotifierWatcher"
WATCHER_OBJ_PATH = "/StatusNotifierWatcher"
WATCHER_XML = """
<node>
  <interface name="org.kde.StatusNotifierWatcher">
    <method name="RegisterStatusNotifierItem">
      <arg type="s" name="service" direction="in"/>
    </method>
    <method name="RegisterStatusNotifierHost">
      <arg type="s" name="service" direction="in"/>
    </method>
    <property type="as" name="RegisteredStatusNotifierItems" access="read"/>
    <property type="b" name="IsStatusNotifierHostRegistered" access="read"/>
    <property type="i" name="ProtocolVersion" access="read"/>
    <signal name="StatusNotifierItemRegistered">
      <arg type="s" name="service"/>
    </signal>
    <signal name="StatusNotifierItemUnregistered">
      <arg type="s" name="service"/>
    </signal>
    <signal name="StatusNotifierHostRegistered"/>
  </interface>
</node>
"""


def _decode_sni_pixmap(pixmaps, target_size: int = 18):
    """Konwertuje tablicę a(iiay) ARGB32 (big-endian) na GdkPixbuf.Pixbuf RGBA."""
    if not pixmaps:
        return None
    best = None
    best_diff = 10**9
    for entry in pixmaps:
        try:
            w, h, raw = entry
            w, h = int(w), int(h)
            if w <= 0 or h <= 0 or not raw:
                continue
            diff = abs(w - target_size) + (0 if w >= target_size else 4)
            if diff < best_diff:
                best_diff = diff
                best = (w, h, raw)
        except Exception:
            continue
    if best is None:
        return None
    w, h, raw = best
    try:
        raw_bytes = bytes(raw)
        n_bytes = w * h * 4
        if len(raw_bytes) < n_bytes:
            return None
        argb = raw_bytes[:n_bytes]
        rgba = bytearray(n_bytes)
        rgba[0::4] = argb[1::4]
        rgba[1::4] = argb[2::4]
        rgba[2::4] = argb[3::4]
        rgba[3::4] = argb[0::4]
        pb = GdkPixbuf.Pixbuf.new_from_bytes(
            GLib.Bytes.new(bytes(rgba)),
            GdkPixbuf.Colorspace.RGB,
            True,
            8,
            w,
            h,
            w * 4
        )
        if w != target_size or h != target_size:
            pb = pb.scale_simple(target_size, target_size, GdkPixbuf.InterpType.BILINEAR)
        return pb
    except Exception:
        return None


def _resolve_icon_from_theme_path(icon_name: str, theme_path: str, size: int = 18):
    if not icon_name or not theme_path or not os.path.isdir(theme_path):
        return None
    for ext in ("", ".png", ".svg", ".xpm"):
        p = os.path.join(theme_path, f"{icon_name}{ext}")
        if os.path.isfile(p):
            try:
                return GdkPixbuf.Pixbuf.new_from_file_at_scale(p, size, size, True)
            except Exception:
                pass
    return None


class StatusNotifierTray(Gtk.Box):
    """Kontener tacy systemowej SNI umieszczany po prawej stronie okay-panel."""

    def __init__(self, get_panel_pos_cb=None, animations_enabled_cb=None):
        super().__init__(orientation=Gtk.Orientation.HORIZONTAL, spacing=2)
        self._get_panel_pos = get_panel_pos_cb or (lambda: "bottom")
        self._animations_enabled = animations_enabled_cb or (lambda: True)
        self._items = {}        # item_id -> {"bus": ..., "path": ..., "btn": ..., "props": ...}
        self._conn = None
        self._reg_id = 0
        self._own_id = 0
        self._host_own_id = 0
        self._sub_ids = []
        self._icon_theme = Gtk.IconTheme.get_default()
        self.set_no_show_all(True)
        self._init_dbus()

    def _init_dbus(self):
        try:
            self._conn = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        except Exception:
            return

        node_info = Gio.DBusNodeInfo.new_for_xml(WATCHER_XML)
        iface_info = node_info.lookup_interface(WATCHER_BUS_NAME)
        try:
            self._reg_id = self._conn.register_object(
                WATCHER_OBJ_PATH,
                iface_info,
                self._on_watcher_method,
                self._on_watcher_get_prop,
                None,
            )
        except Exception:
            self._reg_id = 0

        self._own_id = Gio.bus_own_name_on_connection(
            self._conn,
            WATCHER_BUS_NAME,
            Gio.BusNameOwnerFlags.REPLACE | Gio.BusNameOwnerFlags.ALLOW_REPLACEMENT,
            None,
            self._on_watcher_name_lost,
        )

        host_name = f"org.kde.StatusNotifierHost-{os.getpid()}-1"
        self._host_own_id = Gio.bus_own_name_on_connection(
            self._conn,
            host_name,
            Gio.BusNameOwnerFlags.NONE,
            None,
            None,
        )

        # Nasłuch zniknięć właścicieli usług oraz aktualizacji ikon/statusów SNI
        self._sub_ids.append(
            self._conn.signal_subscribe(
                "org.freedesktop.DBus",
                "org.freedesktop.DBus",
                "NameOwnerChanged",
                "/org/freedesktop/DBus",
                None,
                Gio.DBusSignalFlags.NONE,
                self._on_name_owner_changed,
            )
        )
        for sig_name in ("NewIcon", "NewAttentionIcon", "NewStatus", "NewTitle", "NewToolTip"):
            self._sub_ids.append(
                self._conn.signal_subscribe(
                    None,
                    "org.kde.StatusNotifierItem",
                    sig_name,
                    None,
                    None,
                    Gio.DBusSignalFlags.NONE,
                    self._on_item_signal,
                )
            )

    def _on_watcher_name_lost(self, _conn, _name):
        # Jeśli istnieje zewnętrzny Watcher, podepnij się jako Host i wczytaj aktualne pozycje
        if not self._conn:
            return
        host_name = f"org.kde.StatusNotifierHost-{os.getpid()}-1"
        self._conn.call(
            WATCHER_BUS_NAME,
            WATCHER_OBJ_PATH,
            WATCHER_BUS_NAME,
            "RegisterStatusNotifierHost",
            GLib.Variant("(s)", (host_name,)),
            None,
            Gio.DBusCallFlags.NONE,
            2000,
            None,
            None,
        )
        self._conn.call(
            WATCHER_BUS_NAME,
            WATCHER_OBJ_PATH,
            "org.freedesktop.DBus.Properties",
            "Get",
            GLib.Variant("(ss)", (WATCHER_BUS_NAME, "RegisteredStatusNotifierItems")),
            GLib.VariantType("(v)"),
            Gio.DBusCallFlags.NONE,
            2000,
            None,
            self._on_external_items_fetched,
        )

    def _on_external_items_fetched(self, conn, res):
        try:
            (val,) = conn.call_finish(res).unpack()
            for service in val or []:
                self._add_sni_item(service, sender=None)
        except Exception:
            pass

    def _on_watcher_method(self, _conn, sender, _obj_path, _iface, method_name, params, invocation):
        if method_name == "RegisterStatusNotifierItem":
            (service,) = params.unpack()
            self._add_sni_item(service, sender=sender)
            invocation.return_value(None)
        elif method_name == "RegisterStatusNotifierHost":
            try:
                self._conn.emit_signal(
                    None, WATCHER_OBJ_PATH, WATCHER_BUS_NAME,
                    "StatusNotifierHostRegistered", None
                )
            except Exception:
                pass
            invocation.return_value(None)

    def _on_watcher_get_prop(self, _conn, _sender, _obj_path, _iface, prop_name):
        if prop_name == "RegisteredStatusNotifierItems":
            return GLib.Variant("as", list(self._items.keys()))
        if prop_name == "IsStatusNotifierHostRegistered":
            return GLib.Variant("b", True)
        if prop_name == "ProtocolVersion":
            return GLib.Variant("i", 0)
        return None

    def _add_sni_item(self, service: str, sender: str = None):
        if not service:
            return
        if service.startswith("/"):
            if not sender:
                return
            bus_name, obj_path = sender, service
        else:
            if "/" in service:
                bus_name, rest = service.split("/", 1)
                obj_path = "/" + rest
            else:
                bus_name, obj_path = service, "/StatusNotifierItem"

        item_id = f"{bus_name}{obj_path}"
        if item_id not in self._items:
            btn = Gtk.Button()
            btn.get_style_context().add_class("tray-item")
            img = Gtk.Image.new_from_icon_name("application-x-executable-symbolic", Gtk.IconSize.MENU)
            btn.add(img)
            btn.show_all()

            rec = {
                "id": item_id,
                "bus": bus_name,
                "path": obj_path,
                "btn": btn,
                "img": img,
                "props": {},
            }
            btn.connect("button-press-event", self._on_tray_button_press, rec)
            self._items[item_id] = rec
            self.pack_start(btn, False, False, 0)
            self.show()

            try:
                self._conn.emit_signal(
                    None, WATCHER_OBJ_PATH, WATCHER_BUS_NAME,
                    "StatusNotifierItemRegistered",
                    GLib.Variant("(s)", (item_id,))
                )
            except Exception:
                pass

        self._fetch_item_props(item_id)

    def _remove_sni_item(self, item_id: str):
        rec = self._items.pop(item_id, None)
        if rec is None:
            return
        btn = rec["btn"]
        self.remove(btn)
        btn.destroy()
        if not self._items:
            self.hide()
        try:
            if self._conn:
                self._conn.emit_signal(
                    None, WATCHER_OBJ_PATH, WATCHER_BUS_NAME,
                    "StatusNotifierItemUnregistered",
                    GLib.Variant("(s)", (item_id,))
                )
        except Exception:
            pass

    def _on_name_owner_changed(self, _conn, _sender, _path, _iface, _sig, params):
        name, _old_owner, new_owner = params.unpack()
        if new_owner:
            return
        stale = [k for k, v in self._items.items() if v["bus"] == name]
        for k in stale:
            self._remove_sni_item(k)

    def _on_item_signal(self, _conn, sender, path, _iface, _sig, _params):
        item_id = f"{sender}{path}"
        if item_id in self._items:
            self._fetch_item_props(item_id)

    def _fetch_item_props(self, item_id: str):
        rec = self._items.get(item_id)
        if not rec or not self._conn:
            return
        self._conn.call(
            rec["bus"],
            rec["path"],
            "org.freedesktop.DBus.Properties",
            "GetAll",
            GLib.Variant("(s)", ("org.kde.StatusNotifierItem",)),
            GLib.VariantType("(a{sv})"),
            Gio.DBusCallFlags.NONE,
            1500,
            None,
            lambda conn, res, iid=item_id: self._on_props_ready(conn, res, iid),
        )

    def _on_props_ready(self, conn, res, item_id: str):
        rec = self._items.get(item_id)
        if not rec:
            return
        try:
            (props,) = conn.call_finish(res).unpack()
        except Exception:
            return
        rec["props"] = props

        status = str(props.get("Status", "Active"))
        btn = rec["btn"]
        if status == "Passive":
            btn.hide()
        else:
            btn.show()
            self.show()

        # Tooltip: Title lub ToolTip[2]
        tooltip = str(props.get("Title") or props.get("Id") or "Aplikacja")
        tt = props.get("ToolTip")
        if isinstance(tt, (tuple, list)) and len(tt) >= 3 and tt[2]:
            tooltip = str(tt[2])
        btn.set_tooltip_text(tooltip)

        # Ikona: nazwa z motywu -> IconThemePath -> AttentionIconPixmap -> IconPixmap
        icon_name = ""
        pixmap_data = None
        if status == "NeedsAttention":
            icon_name = str(props.get("AttentionIconName") or "")
            pixmap_data = props.get("AttentionIconPixmap")
        if not icon_name and not pixmap_data:
            icon_name = str(props.get("IconName") or "")
            pixmap_data = props.get("IconPixmap")

        theme_path = str(props.get("IconThemePath") or "")
        img = rec["img"]

        if icon_name:
            if os.path.isabs(icon_name) and os.path.isfile(icon_name):
                try:
                    pb = GdkPixbuf.Pixbuf.new_from_file_at_scale(icon_name, 18, 18, True)
                    img.set_from_pixbuf(pb)
                    return
                except Exception:
                    pass
            pb = _resolve_icon_from_theme_path(icon_name, theme_path, 18)
            if pb is not None:
                img.set_from_pixbuf(pb)
                return
            if self._icon_theme and self._icon_theme.has_icon(icon_name):
                img.set_from_icon_name(icon_name, Gtk.IconSize.MENU)
                return

        if pixmap_data:
            pb = _decode_sni_pixmap(pixmap_data, 18)
            if pb is not None:
                img.set_from_pixbuf(pb)
                return

        img.set_from_icon_name("application-x-executable-symbolic", Gtk.IconSize.MENU)

    def _on_tray_button_press(self, widget, event, rec: dict):
        if not self._conn:
            return True
        props = rec.get("props", {})
        x = int(getattr(event, "x_root", 0))
        y = int(getattr(event, "y_root", 0))

        if event.button == 1:
            if props.get("ItemIsMenu", False):
                self._open_context_or_dbusmenu(widget, rec, x, y)
            else:
                self._call_sni_method(
                    rec, "Activate", GLib.Variant("(ii)", (x, y)),
                    fallback_cb=lambda: self._open_context_or_dbusmenu(widget, rec, x, y)
                )
        elif event.button == 2:
            self._call_sni_method(rec, "SecondaryActivate", GLib.Variant("(ii)", (x, y)))
        elif event.button == 3:
            self._open_context_or_dbusmenu(widget, rec, x, y)
        return True

    def _call_sni_method(self, rec: dict, method: str, variant_args, fallback_cb=None):
        self._conn.call(
            rec["bus"],
            rec["path"],
            "org.kde.StatusNotifierItem",
            method,
            variant_args,
            None,
            Gio.DBusCallFlags.NONE,
            1500,
            None,
            lambda conn, res, fb=fallback_cb: self._on_sni_call_finish(conn, res, fb),
        )

    @staticmethod
    def _on_sni_call_finish(conn, res, fallback_cb):
        try:
            conn.call_finish(res)
        except Exception:
            if fallback_cb is not None:
                fallback_cb()

    def _open_context_or_dbusmenu(self, widget, rec: dict, x: int, y: int):
        menu_path = rec.get("props", {}).get("Menu", "")
        if menu_path and self._conn:
            self._conn.call(
                rec["bus"],
                str(menu_path),
                "com.canonical.dbusmenu",
                "GetLayout",
                GLib.Variant("(iias)", (0, 3, [])),
                GLib.VariantType("(u(ia{sv}av))"),
                Gio.DBusCallFlags.NONE,
                1500,
                None,
                lambda conn, res, r=rec, w=widget, mp=str(menu_path), px=x, py=y: self._on_dbusmenu_layout(conn, res, r, w, mp, px, py),
            )
        else:
            self._call_sni_method(rec, "ContextMenu", GLib.Variant("(ii)", (x, y)))

    def _on_dbusmenu_layout(self, conn, res, rec: dict, widget, menu_path: str, x: int, y: int):
        try:
            (_rev, root_node) = conn.call_finish(res).unpack()
            menu = self._build_gtk_dbusmenu(rec["bus"], menu_path, root_node)
            if menu is not None and len(menu.get_children()) > 0:
                anim = bool(self._animations_enabled())
                apply_animation_class(menu, anim)
                menu.show_all()
                prepare_fade_in(menu, anim)
                if self._get_panel_pos() == "top":
                    menu.popup_at_widget(widget, Gdk.Gravity.SOUTH, Gdk.Gravity.NORTH, None)
                else:
                    menu.popup_at_widget(widget, Gdk.Gravity.NORTH, Gdk.Gravity.SOUTH, None)
                start_fade_in(menu, anim)
                return
        except Exception:
            pass
        self._call_sni_method(rec, "ContextMenu", GLib.Variant("(ii)", (x, y)))

    def _build_gtk_dbusmenu(self, bus_name: str, menu_path: str, node) -> Gtk.Menu:
        menu = Gtk.Menu()
        _node_id, _props, children = node
        for child in children or []:
            self._append_dbusmenu_child(menu, bus_name, menu_path, child)
        return menu

    def _append_dbusmenu_child(self, menu: Gtk.Menu, bus_name: str, menu_path: str, child):
        cid, props, subchildren = child
        if props.get("visible", True) is False:
            return
        if props.get("type") == "separator":
            menu.append(Gtk.SeparatorMenuItem())
            return

        label = str(props.get("label", "")).replace("_", "")
        mi = Gtk.MenuItem(label=label or "...")
        mi.set_sensitive(bool(props.get("enabled", True)))

        if props.get("children-display") == "submenu" or subchildren:
            sub = Gtk.Menu()
            for sc in subchildren or []:
                self._append_dbusmenu_child(sub, bus_name, menu_path, sc)
            mi.set_submenu(sub)
        else:
            mi.connect("activate", lambda _m, nid=int(cid): self._send_dbusmenu_click(bus_name, menu_path, nid))
        menu.append(mi)

    def _send_dbusmenu_click(self, bus_name: str, menu_path: str, item_id: int):
        if not self._conn:
            return
        self._conn.call(
            bus_name,
            menu_path,
            "com.canonical.dbusmenu",
            "Event",
            GLib.Variant("(isvu)", (item_id, "clicked", GLib.Variant("i", 0), int(time.time()))),
            None,
            Gio.DBusCallFlags.NONE,
            1500,
            None,
            None,
        )
