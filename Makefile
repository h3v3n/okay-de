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
	install -m 755 scripts/okay-panel $(DESTDIR)$(PREFIX)/bin/okay-panel
	install -m 755 scripts/oksavi-ghost-runner $(DESTDIR)$(PREFIX)/bin/oksavi-ghost-runner

	# Usunięcie pozostałości starego paska docka
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-dock
	rm -f $(DESTDIR)$(PREFIX)/share/applications/okay-dock.desktop

	# Rejestracja sesji Wayland dla Display Managerów
	install -d $(DESTDIR)$(PREFIX)/share/wayland-sessions
	install -m 644 sessions/okay-de.desktop $(DESTDIR)$(PREFIX)/share/wayland-sessions/okay-de.desktop

	# Rejestracja plików .desktop aplikacji
	install -d $(DESTDIR)$(PREFIX)/share/applications
	for f in sessions/*.desktop; do \
		if [ -f "$$f" ]; then \
			install -m 644 "$$f" $(DESTDIR)$(PREFIX)/share/applications/; \
		fi \
	done

	# Domyślne ikony pulpitu dla nowych kont (/etc/skel/Desktop)
	install -d $(DESTDIR)/etc/skel/Desktop
	install -m 644 sessions/foot.desktop $(DESTDIR)/etc/skel/Desktop/foot.desktop
	install -m 644 sessions/pcmanfm.desktop $(DESTDIR)/etc/skel/Desktop/pcmanfm.desktop
	install -m 644 sessions/okay-settings.desktop $(DESTDIR)/etc/skel/Desktop/okay-settings.desktop

	# Domyślna tapeta Catppuccin Mocha
	install -d $(DESTDIR)$(PREFIX)/share/backgrounds/okay-de
	install -m 644 backgrounds/default.png $(DESTDIR)$(PREFIX)/share/backgrounds/okay-de/default.png

	# Domyślny motyw graficzny OKay dla kompozytora labwc / Openbox (belki i przyciski SSD)
	install -d $(DESTDIR)$(PREFIX)/share/themes/OKay/labwc
	install -m 644 themes/OKay/labwc/* $(DESTDIR)$(PREFIX)/share/themes/OKay/labwc/
	install -d $(DESTDIR)$(PREFIX)/share/themes/OKay/openbox-3
	install -m 644 themes/OKay/openbox-3/* $(DESTDIR)$(PREFIX)/share/themes/OKay/openbox-3/

	# Domyślne konfiguracje systemowe XDG i maski przycisków XBM / SVG
	install -d $(DESTDIR)/etc/xdg/labwc
	install -m 644 config/labwc/environment $(DESTDIR)/etc/xdg/labwc/environment
	install -m 644 config/labwc/rc.xml $(DESTDIR)/etc/xdg/labwc/rc.xml
	install -m 644 config/labwc/menu.xml $(DESTDIR)/etc/xdg/labwc/menu.xml
	install -m 644 config/labwc/themerc $(DESTDIR)/etc/xdg/labwc/themerc
	install -m 644 config/labwc/themerc-override $(DESTDIR)/etc/xdg/labwc/themerc-override
	install -m 644 config/labwc/*.xbm $(DESTDIR)/etc/xdg/labwc/
	install -m 644 config/labwc/*.svg $(DESTDIR)/etc/xdg/labwc/
	install -m 755 config/labwc/autostart $(DESTDIR)/etc/xdg/labwc/autostart

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

	install -d $(DESTDIR)/etc/xdg/pcmanfm/default
	install -m 644 config/pcmanfm/default/desktop-items-0.conf $(DESTDIR)/etc/xdg/pcmanfm/default/desktop-items-0.conf
	install -m 644 config/pcmanfm/default/pcmanfm.conf $(DESTDIR)/etc/xdg/pcmanfm/default/pcmanfm.conf

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-session
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-settings
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-panel
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-dock
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-powermenu
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-clipboard
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-screenshot
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-lock
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-volume
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-brightness
	rm -f $(DESTDIR)$(PREFIX)/bin/oksavi-ghost-runner
	rm -f $(DESTDIR)$(PREFIX)/share/wayland-sessions/okay-de.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/okay-settings.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/okay-dock.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/foot.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/pcmanfm.desktop
	rm -rf $(DESTDIR)$(PREFIX)/share/backgrounds/okay-de
	rm -rf $(DESTDIR)$(PREFIX)/share/themes/OKay
	rm -rf $(DESTDIR)/etc/skel/Desktop
	rm -rf $(DESTDIR)/etc/xdg/labwc
	rm -rf $(DESTDIR)/etc/xdg/foot
	rm -rf $(DESTDIR)/etc/xdg/fuzzel
	rm -rf $(DESTDIR)/etc/xdg/mako
	rm -f $(DESTDIR)/etc/xdg/gtk-3.0/settings.ini
	rm -f $(DESTDIR)/etc/xdg/gtk-4.0/settings.ini
	rm -rf $(DESTDIR)/etc/xdg/pcmanfm
	rm -rf $(DESTDIR)/etc/xdg/sfwbar
	rm -rf $(DESTDIR)/etc/xdg/waybar

.PHONY: all install uninstall
