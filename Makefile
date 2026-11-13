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
	install -m 755 scripts/okay-desktop $(DESTDIR)$(PREFIX)/bin/okay-desktop

	# Moduły centrum sterowania OKay Settings oraz aplety panelu OKay Panel (GTK3)
	install -d $(DESTDIR)$(PREFIX)/share/okay-de/settings $(DESTDIR)$(PREFIX)/share/okay-de/panel
	install -m 644 scripts/settings/*.py $(DESTDIR)$(PREFIX)/share/okay-de/settings/
	install -m 644 scripts/panel/*.py $(DESTDIR)$(PREFIX)/share/okay-de/panel/

	# Usunięcie pozostałości starego paska docka
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-dock
	rm -f $(DESTDIR)$(PREFIX)/share/applications/okay-dock.desktop

	# Rejestracja sesji Wayland dla Display Managerów
	install -d $(DESTDIR)$(PREFIX)/share/wayland-sessions
	install -m 644 sessions/okay-de.desktop $(DESTDIR)$(PREFIX)/share/wayland-sessions/okay-de.desktop

	# Rejestracja plików .desktop aplikacji (bez pliku sesji okay-de.desktop)
	install -d $(DESTDIR)$(PREFIX)/share/applications
	for f in sessions/*.desktop; do \
		if [ -f "$$f" ] && [ "$$(basename "$$f")" != "okay-de.desktop" ]; then \
			install -m 644 "$$f" $(DESTDIR)$(PREFIX)/share/applications/; \
		fi \
	done

	# Domyślne ikony pulpitu i konfiguracja dla nowych kont (/etc/skel)
	install -d $(DESTDIR)/etc/skel/Desktop \
		$(DESTDIR)/etc/skel/.config/okay-de \
		$(DESTDIR)/etc/skel/.config/foot \
		$(DESTDIR)/etc/skel/.config/mako \
		$(DESTDIR)/etc/skel/.config/gtk-3.0 \
		$(DESTDIR)/etc/skel/.config/gtk-4.0 \
		$(DESTDIR)/etc/skel/.config/pcmanfm/default
	install -m 644 sessions/foot.desktop $(DESTDIR)/etc/skel/Desktop/foot.desktop
	install -m 644 sessions/pcmanfm.desktop $(DESTDIR)/etc/skel/Desktop/pcmanfm.desktop
	install -m 644 sessions/okay-settings.desktop $(DESTDIR)/etc/skel/Desktop/okay-settings.desktop
	install -m 644 config/wayfire/wayfire.ini $(DESTDIR)/etc/skel/.config/wayfire.ini
	install -m 755 config/okay-de/autostart $(DESTDIR)/etc/skel/.config/okay-de/autostart
	install -m 644 config/okay-de/panel.json $(DESTDIR)/etc/skel/.config/okay-de/panel.json
	install -m 644 config/foot/foot.ini $(DESTDIR)/etc/skel/.config/foot/foot.ini
	install -m 644 config/mako/config $(DESTDIR)/etc/skel/.config/mako/config
	install -m 644 config/gtk-3.0/settings.ini $(DESTDIR)/etc/skel/.config/gtk-3.0/settings.ini
	install -m 644 config/gtk-4.0/settings.ini $(DESTDIR)/etc/skel/.config/gtk-4.0/settings.ini
	install -m 644 config/pcmanfm/default/desktop-items-0.conf $(DESTDIR)/etc/skel/.config/pcmanfm/default/desktop-items-0.conf
	install -m 644 config/pcmanfm/default/pcmanfm.conf $(DESTDIR)/etc/skel/.config/pcmanfm/default/pcmanfm.conf

	# Domyślna tapeta Catppuccin Mocha
	install -d $(DESTDIR)$(PREFIX)/share/backgrounds/okay-de
	install -m 644 backgrounds/default.png $(DESTDIR)$(PREFIX)/share/backgrounds/okay-de/default.png

	# Usunięcie pozostałości konfiguracji przestarzałego kompozytora labwc
	rm -rf $(DESTDIR)/etc/xdg/labwc \
		$(DESTDIR)$(PREFIX)/share/okay-de/defaults/labwc \
		$(DESTDIR)$(PREFIX)/share/themes/OKay/labwc \
		$(DESTDIR)$(PREFIX)/share/themes/OKay/openbox-3

	# Domyślne konfiguracje systemowe XDG i kompozytora Wayfire
	install -d $(DESTDIR)/etc/xdg
	install -m 644 config/wayfire/wayfire.ini $(DESTDIR)/etc/xdg/wayfire.ini

	install -d $(DESTDIR)/etc/xdg/foot
	install -m 644 config/foot/foot.ini $(DESTDIR)/etc/xdg/foot/foot.ini

	install -d $(DESTDIR)/etc/xdg/mako
	install -m 644 config/mako/config $(DESTDIR)/etc/xdg/mako/config

	install -d $(DESTDIR)/etc/xdg/gtk-3.0
	install -m 644 config/gtk-3.0/settings.ini $(DESTDIR)/etc/xdg/gtk-3.0/settings.ini

	install -d $(DESTDIR)/etc/xdg/gtk-4.0
	install -m 644 config/gtk-4.0/settings.ini $(DESTDIR)/etc/xdg/gtk-4.0/settings.ini

	install -d $(DESTDIR)/etc/xdg/pcmanfm/default
	install -m 644 config/pcmanfm/default/desktop-items-0.conf $(DESTDIR)/etc/xdg/pcmanfm/default/desktop-items-0.conf
	install -m 644 config/pcmanfm/default/pcmanfm.conf $(DESTDIR)/etc/xdg/pcmanfm/default/pcmanfm.conf

	# Konfiguracja OKay DE, kompozytora Wayfire oraz szablony read-only
	install -d $(DESTDIR)/etc/xdg/okay-de $(DESTDIR)/etc/okay-de \
		$(DESTDIR)$(PREFIX)/share/okay-de/defaults/okay-de \
		$(DESTDIR)$(PREFIX)/share/okay-de/defaults/wayfire \
		$(DESTDIR)$(PREFIX)/share/okay-de/defaults/backgrounds
	install -m 644 config/okay-de/panel.json $(DESTDIR)/etc/xdg/okay-de/panel.json
	install -m 644 config/okay-de/panel.json $(DESTDIR)/etc/okay-de/panel.json
	install -m 644 config/okay-de/panel.json $(DESTDIR)$(PREFIX)/share/okay-de/defaults/okay-de/panel.json
	install -m 755 config/okay-de/autostart $(DESTDIR)/etc/xdg/okay-de/autostart
	install -m 755 config/okay-de/autostart $(DESTDIR)/etc/okay-de/autostart
	install -m 755 config/okay-de/autostart $(DESTDIR)$(PREFIX)/share/okay-de/defaults/okay-de/autostart
	install -m 644 config/wayfire/wayfire.ini $(DESTDIR)/etc/xdg/okay-de/wayfire.ini
	install -m 644 config/wayfire/wayfire.ini $(DESTDIR)/etc/okay-de/wayfire.ini
	install -m 644 config/wayfire/wayfire.ini $(DESTDIR)$(PREFIX)/share/okay-de/defaults/wayfire/wayfire.ini
	install -m 644 backgrounds/default.png $(DESTDIR)$(PREFIX)/share/okay-de/defaults/backgrounds/default.png

	# Reguły uprawnień administracyjnych (Polkit & Sudoers)
	install -d $(DESTDIR)$(PREFIX)/share/polkit-1/rules.d
	install -m 644 config/polkit/99-okay-de.rules $(DESTDIR)$(PREFIX)/share/polkit-1/rules.d/99-okay-de.rules
	install -d $(DESTDIR)/etc/polkit-1/rules.d
	install -m 644 config/polkit/99-okay-de.rules $(DESTDIR)/etc/polkit-1/rules.d/99-okay-de.rules
	install -d $(DESTDIR)/etc/sudoers.d
	install -m 440 config/sudoers/99-okay-de $(DESTDIR)/etc/sudoers.d/99-okay-de

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-session
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-settings
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-panel
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-desktop
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-dock
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-powermenu
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-clipboard
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-screenshot
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-lock
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-volume
	rm -f $(DESTDIR)$(PREFIX)/bin/okay-brightness
	rm -f $(DESTDIR)$(PREFIX)/share/wayland-sessions/okay-de.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/okay-settings.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/okay-dock.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/foot.desktop
	rm -f $(DESTDIR)$(PREFIX)/share/applications/pcmanfm.desktop
	rm -rf $(DESTDIR)$(PREFIX)/share/backgrounds/okay-de
	rm -rf $(DESTDIR)$(PREFIX)/share/okay-de
	rm -rf $(DESTDIR)$(PREFIX)/share/themes/OKay
	rm -rf $(DESTDIR)/etc/skel/Desktop
	rm -rf $(DESTDIR)/etc/skel/.config/okay-de
	rm -f $(DESTDIR)/etc/skel/.config/wayfire.ini
	rm -f $(DESTDIR)/etc/xdg/wayfire.ini
	rm -rf $(DESTDIR)/etc/xdg/labwc
	rm -rf $(DESTDIR)/etc/xdg/foot
	rm -rf $(DESTDIR)/etc/xdg/mako
	rm -f $(DESTDIR)/etc/xdg/gtk-3.0/settings.ini
	rm -f $(DESTDIR)/etc/xdg/gtk-4.0/settings.ini
	rm -rf $(DESTDIR)/etc/xdg/pcmanfm
	rm -rf $(DESTDIR)/etc/xdg/okay-de
	rm -rf $(DESTDIR)/etc/okay-de
	rm -rf $(DESTDIR)/etc/xdg/sfwbar
	rm -rf $(DESTDIR)/etc/xdg/waybar

.PHONY: all install uninstall
