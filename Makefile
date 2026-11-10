PREFIX ?= /usr
DESTDIR ?=

all:
	@echo "OKay DE - Gotowy do instalacji. Uruchom: make install"

install:
	# Instalacja skryptów wykonawczych
	install -d $(DESTDIR)$(PREFIX)/bin
	install -m 755 scripts/okay-session $(DESTDIR)$(PREFIX)/bin/okay-session
	install -m 755 scripts/okay-powermenu $(DESTDIR)$(PREFIX)/bin/okay-powermenu
	install -m 755 scripts/okay-clipboard $(DESTDIR)$(PREFIX)/bin/okay-clipboard
	install -m 755 scripts/okay-screenshot $(DESTDIR)$(PREFIX)/bin/okay-screenshot

	# Rejestracja sesji Wayland dla Display Managerów (greetd, sddm, gdm itp.)
	install -d $(DESTDIR)$(PREFIX)/share/wayland-sessions
	install -m 644 sessions/okay-de.desktop $(DESTDIR)$(PREFIX)/share/wayland-sessions/okay-de.desktop

	# Domyślne konfiguracje systemowe XDG
	install -d $(DESTDIR)/etc/xdg/labwc
	install -m 644 config/labwc/rc.xml $(DESTDIR)/etc/xdg/labwc/rc.xml
	install -m 644 config/labwc/menu.xml $(DESTDIR)/etc/xdg/labwc/menu.xml
	install -m 644 config/labwc/themerc $(DESTDIR)/etc/xdg/labwc/themerc
	install -m 644 config/labwc/themerc-override $(DESTDIR)/etc/xdg/labwc/themerc-override
	install -m 755 config/labwc/autostart $(DESTDIR)/etc/xdg/labwc/autostart

	install -d $(DESTDIR)/etc/xdg/waybar
	install -m 644 config/waybar/config.jsonc $(DESTDIR)/etc/xdg/waybar/config.jsonc
	install -m 644 config/waybar/style.css $(DESTDIR)/etc/xdg/waybar/style.css

	install -d $(DESTDIR)/etc/xdg/foot
	install -m 644 config/foot/foot.ini $(DESTDIR)/etc/xdg/foot/foot.ini

	install -d $(DESTDIR)/etc/xdg/fuzzel
	install -m 644 config/fuzzel/fuzzel.ini $(DESTDIR)/etc/xdg/fuzzel/fuzzel.ini

	install -d $(DESTDIR)/etc/xdg/mako
	install -m 644 config/mako/config $(DESTDIR)/etc/xdg/mako/config

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-session
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-powermenu
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-clipboard
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-screenshot
	rm -f $(DESTDIR)$(PREFIX)/share/wayland-sessions/okay-de.desktop
	rm -rf $(DESTDIR)/etc/xdg/labwc
	rm -rf $(DESTDIR)/etc/xdg/waybar
	rm -rf $(DESTDIR)/etc/xdg/foot
	rm -rf $(DESTDIR)/etc/xdg/fuzzel
	rm -rf $(DESTDIR)/etc/xdg/mako

.PHONY: all install uninstall
