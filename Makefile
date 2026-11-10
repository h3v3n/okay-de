PREFIX ?= /usr
DESTDIR ?=

all:
	@echo "OKay DE - Gotowy do instalacji. Uruchom: make install"

install:
	# Instalacja skryptów wykonawczych i aplikacji
	install -d $(DESTDIR)$(PREFIX)/bin
	install -m 755 scripts/okay-session $(DESTDIR)$(PREFIX)/bin/okay-session
	install -m 755 scripts/okay-settings $(DESTDIR)$(PREFIX)/bin/okay-settings
	install -m 755 scripts/okay-powermenu $(DESTDIR)$(PREFIX)/bin/okay-powermenu
	install -m 755 scripts/okay-clipboard $(DESTDIR)$(PREFIX)/bin/okay-clipboard
	install -m 755 scripts/okay-screenshot $(DESTDIR)$(PREFIX)/bin/okay-screenshot
	install -m 755 scripts/okay-lock $(DESTDIR)$(PREFIX)/bin/okay-lock
	install -m 755 scripts/okay-volume $(DESTDIR)$(PREFIX)/bin/okay-volume
	install -m 755 scripts/okay-brightness $(DESTDIR)$(PREFIX)/bin/okay-brightness
	install -m 755 scripts/oksavi-ghost-runner $(DESTDIR)$(PREFIX)/bin/oksavi-ghost-runner

	# Rejestracja sesji Wayland dla Display Managerów
	install -d $(DESTDIR)$(PREFIX)/share/wayland-sessions
	install -m 644 sessions/okay-de.desktop $(DESTDIR)$(PREFIX)/share/wayland-sessions/okay-de.desktop

	# Rejestracja plików .desktop aplikacji
	install -d $(DESTDIR)$(PREFIX)/share/applications
	install -m 644 sessions/okay-settings.desktop $(DESTDIR)$(PREFIX)/share/applications/okay-settings.desktop
	install -m 644 sessions/foot.desktop $(DESTDIR)$(PREFIX)/share/applications/foot.desktop
	install -m 644 sessions/pcmanfm.desktop $(DESTDIR)$(PREFIX)/share/applications/pcmanfm.desktop

	# Domyślne ikony pulpitu dla nowych kont (/etc/skel/Desktop)
	install -d $(DESTDIR)/etc/skel/Desktop
	install -m 644 sessions/foot.desktop $(DESTDIR)/etc/skel/Desktop/foot.desktop
	install -m 644 sessions/pcmanfm.desktop $(DESTDIR)/etc/skel/Desktop/pcmanfm.desktop
	install -m 644 sessions/okay-settings.desktop $(DESTDIR)/etc/skel/Desktop/okay-settings.desktop

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
	install -m 644 config/waybar/dock-style.css $(DESTDIR)/etc/xdg/waybar/dock-style.css

	install -d $(DESTDIR)/etc/xdg/foot
	install -m 644 config/foot/foot.ini $(DESTDIR)/etc/xdg/foot/foot.ini

	install -d $(DESTDIR)/etc/xdg/fuzzel
	install -m 644 config/fuzzel/fuzzel.ini $(DESTDIR)/etc/xdg/fuzzel/fuzzel.ini

	install -d $(DESTDIR)/etc/xdg/mako
	install -m 644 config/mako/config $(DESTDIR)/etc/xdg/mako/config

	install -d $(DESTDIR)/etc/xdg/gtk-3.0
	install -m 644 config/gtk-3.0/settings.ini $(DESTDIR)/etc/xdg/gtk-3.0/settings.ini

	install -d $(DESTDIR)/etc/xdg/gtk-4.0
	install -m 644 config/gtk-4.0/settings.ini $(DESTDIR)/etc/xdg/gtk-4.0/settings.ini

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-session
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-settings
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-powermenu
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-clipboard
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-screenshot
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-lock
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-volume
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-brightness
	rm -f $(DESTDIR)$(PREFIX)/bin/oksavi-ghost-runner
	rm -f $(DESTDIR)$(PREFIX)/share/wayland-sessions/okay-de.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/okay-settings.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/foot.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/pcmanfm.desktop
	rm -rf $(DESTDIR)/etc/skel/Desktop
	rm -rf $(DESTDIR)/etc/xdg/labwc
	rm -rf $(DESTDIR)/etc/xdg/waybar
	rm -rf $(DESTDIR)/etc/xdg/foot
	rm -rf $(DESTDIR)/etc/xdg/fuzzel
	rm -rf $(DESTDIR)/etc/xdg/mako
	rm -f $(DESTDIR)/etc/xdg/gtk-3.0/settings.ini
	rm -f $(DESTDIR)/etc/xdg/gtk-4.0/settings.ini

.PHONY: all install uninstall
