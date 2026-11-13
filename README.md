# OKay DE

Ultra-lekkie, nowoczesne środowisko graficzne Wayland dla dystrybucji **OKsavi Linux**, oparte na kompozytorze `Wayfire`, natywnym zunifikowanym pasku zadań `okay-panel` (GTK3 + GtkLayerShell), pulpicie z ikonami (`okay-desktop`) oraz modularnym centrum sterowania **OKay Settings** (GTK3).

---

## Interfejs i komponenty

- **Interaktywny pulpit (`okay-desktop`):** Natywny pulpit Wayland (`GtkLayerShell.Layer.BACKGROUND`) obsługujący ikony plików, skróty aplikacji (`.desktop`) w `~/Desktop`, przeciąganie (Drag & Drop z menedżera plików, np. Dolphin / PCManFM), menu kontekstowe oraz animacje fade bez zasłaniania tapety `swaybg`.
- **Domyślne ikony na pulpicie:** Szablon w `/etc/skel/Desktop/` z gotowymi skrótami:
  - Terminal (`foot`)
  - Przeglądarka plików (`pcmanfm`)
  - Centrum sterowania (`okay-settings`)
- **Centrum Sterowania (OKay Settings):** Modularna aplikacja GTK3 (`Gtk.StackSidebar` + `Gtk.Stack`) z natywnymi zakładkami:
  - **Wi-Fi i Sieć:** Skanowanie i łączenie z sieciami bezprzewodowymi (`nmcli`, nieblokujący wątek roboczy).
  - **Monitory:** Konfiguracja ekranów, rozdzielczości, odświeżania (Hz) i skalowania (`wlr-randr`).
  - **Mysz i Touchpad:** Prędkość wskaźnika, tap-to-click i naturalne przewijanie zapisywane w `~/.config/wayfire.ini` (`[input]`, automatyczne przeładowanie na żywo).
  - **Dźwięk:** Regulacja głośności i wybór urządzeń wejścia/wyjścia PipeWire (`wpctl`).
  - **Pulpit i Panel:** Tryb „Tylko ikony”, tryb Dock/Panel, pozycja panelu (`~/.config/okay-de/panel.json`), animacje (`animate` + GTK), kolor akcentu, styl/zaokrąglenia okien (`pixdecor` + GTK) oraz tapeta pulpitu (`swaybg`).
  - **System i Aktualizacje:** Specyfikacja sprzętu i aktualizacje OKay DE bezpośrednio z GitHub (`h3v3n/okay-de`).
- **Architektura Server-Side Decorations (SSD) i animacje (`Wayfire`):** Belki kompozytora `Wayfire` (`[decoration]` / opcjonalnie `[pixdecor]` z zaokrąglonymi narożnikami) oraz wbudowane płynne animacje fade podczas otwierania, zamykania i minimalizowania okien (`[animate]`). Dzięki zmiennym sesyjnym `GTK_CSD=0` oraz `QT_WAYLAND_DISABLE_WINDOWDECORATION=0` aplikacje GTK3/GTK4 i Qt zachowują spójne obramowanie systemowe.
- **Tapeta i tło pulpitu (`swaybg`):** Demon tapety Wayland z bezpieczną kaskadą: tapeta użytkownika (`~/.config/okay-de/wallpaper.png`) -> tapeta systemowa (`/usr/share/backgrounds/okay-de/default.png`) -> awaryjny kolor bazowy Catppuccin Mocha (`#1e1e2e`).
- **Pasek Zadań OKay (`okay-panel`):** Zunifikowany, natywny pasek zadań GTK 3 + GtkLayerShell z wbudowanym klientem protokołów `wlr-layer-shell` oraz `wlr-foreign-toplevel-management-v1` (zero zewnętrznych zależności z AUR). Łączy przypięte ikony oraz dynamiczne okna w jednym kontenerze (bez duplikacji ikon), oferuje menu kontekstowe PPM („Przypnij / Odepnij od paska”), tryb Panel lub Dock, przełączany tryb „Tylko ikony” / „Ikony + nazwy”, szybkie ustawienia (Wi-Fi, dźwięk, podświetlenie), zasobnik systemowy SNI, zegar oraz przycisk „Pokaż pulpit”.
- **Kafelkowanie (Snapping / Tiling):** Wtyczka `grid` — skróty `Super + Strzałki` (`slot_l`, `slot_r`) oraz przenoszenie (`Super + LPM`) i zmiana rozmiaru (`Super + PPM`).
- **Motyw i ikony:** Ciemny motyw Catppuccin Mocha + OKsavi Dark Neon z akcentem zieleni (`#00e676`), zestaw ikon `Papirus-Dark` oraz kursor `Adwaita (24px)`.

---

## Stos technologiczny

| Rola | Komponent | Opis |
| :--- | :--- | :--- |
| **Kompozytor Wayland** | `wayfire` | Lekki kompozytor 3D/Wayland (wlroots) z wbudowanymi animacjami okien (`animate`), `grid` i `xdg-activation` |
| **Pulpit z ikonami** | `okay-desktop` | Natywny pulpit Wayland (GTK3 + GtkLayerShell) z Drag & Drop dla `~/Desktop` |
| **Centrum Sterowania** | `okay-settings` | Modularna aplikacja Python 3 (GTK3 + PyGObject) |
| **Pasek zadań (Taskbar)** | `okay-panel` | Zunifikowany panel/dock GTK 3 + GtkLayerShell z przypinaniem aplikacji i kontrolą okien (`panel.json`) |
| **Launcher aplikacji** | `okay-panel --toggle-menu` | Natywne menu startowe OKay DE (kategorie, wyszukiwarka, przypięte, zasilanie) |
| **Emulator terminala** | `foot` | Ultra-lekki terminal Wayland z przezroczystością tła `0.95` |
| **Menedżer plików** | `pcmanfm` | Szybki menedżer plików z ciemnym motywem GTK |
| **Demon powiadomień** | `mako` | Lekki demon powiadomień OSD w prawym górnym rogu ekranu |
| **Dźwięk / OSD** | `wpctl` + `okay-volume` | Regulacja głośności z graficznym paskiem postępu |
| **Jasność matrycy** | `brightnessctl` + `okay-brightness` | Regulacja podświetlenia z paskiem OSD |
| **Zrzuty ekranu** | `okay-screenshot` | `grim` + `slurp` do schowka, pliku i z powiadomieniem |
| **Menedżer schowka** | `okay-clipboard` | Natywny picker historii schowka (`cliphist` + `wl-clipboard`, GTK3 + GtkLayerShell) |
| **Blokada ekranu** | `okay-lock` | Estetyczny `swaylock` w motywie Catppuccin Mocha |

---

## Skróty klawiszowe (`~/.config/wayfire.ini`)

| Skrót | Akcja | Polecenie / Wtyczka Wayfire |
| :--- | :--- | :--- |
| `Super + Return` | Uruchom terminal | `foot` |
| `Super + Spacja` | Menu startowe / wyszukiwarka aplikacji | `okay-panel --toggle-menu` |
| `Super + I` | Centrum sterowania | `okay-settings` |
| `Super + E` | Menedżer plików | `pcmanfm` |
| `Super + V` | Historia schowka | `okay-clipboard` |
| `Super + L` | Zablokuj ekran | `okay-lock` |
| `Super + Q` / `Alt + F4` | Zamknij aktywne okno | `close_top_view` |
| `Alt + Tab` | Przełączanie okien | `switcher` |
| `Super + Left` | Kafelkowanie do lewej | `grid` (`slot_l`) |
| `Super + Right` | Kafelkowanie do prawej | `grid` (`slot_r`) |
| `Super + Up` | Maksymalizuj okno | `wm-actions` (`toggle_maximize`) |
| `Super + Down` | Minimalizuj okno | `wm-actions` (`minimize`) |
| `Super + Shift + E` | Menu zasilania (Powermenu)| `okay-powermenu` |
| `PrintScreen` | Zrzut ekranu do pliku i schowka | `okay-screenshot` |
| `KEY_VOLUMEUP` | Głośniej (+5%) z OSD | `okay-volume raise` |
| `KEY_VOLUMEDOWN` | Ciszej (-5%) z OSD | `okay-volume lower` |
| `KEY_MUTE` | Wycisz / Odcisz z OSD | `okay-volume mute` |
| `KEY_BRIGHTNESSUP` | Rozjaśnij ekran (+5%) z OSD | `okay-brightness up` |
| `KEY_BRIGHTNESSDOWN` | Przyciemnij ekran (-5%) z OSD | `okay-brightness down` |

---

## Wymagane pakiety

### Pakiety bazowe (OKsavi / Arch Linux):
```bash
sudo pacman -S wayfire swaybg pcmanfm foot mako swaylock grim slurp wl-clipboard cliphist brightnessctl wireplumber papirus-icon-theme adwaita-icon-theme xdg-user-dirs gtk3 gtk-layer-shell python python-gobject wlr-randr
```

### Sugerowane pakiety opcjonalne:
```bash
# networkmanager, pavucontrol (oficjalne repo)
sudo pacman -S networkmanager pavucontrol

# Opcjonalnie z AUR: pixdecor (zaokrąglone narożniki SSD w Wayfire)
```

---

## Instalacja

### Opcja 1: Instalacja w profilu użytkownika (rekomendowana, bez uprawnień root)
Instaluje binaria do `~/.local/bin`, moduły oraz szablony read-only do `~/.local/share/okay-de/`, a konfiguracje do `~/.config/wayfire.ini` i `~/.config/okay-de/` (z zachowaniem istniejących ustawień użytkownika):
```bash
./install.sh --user
```

### Opcja 2: Instalacja systemowa / budowa pakietu Arch (`PKGBUILD`)
Wdraża pakiety bazowe (w tym `wayfire`) i szablony domyślne do `/etc/xdg/`, `/etc/okay-de/`, `/etc/skel/` oraz `/usr/share/okay-de/defaults/`:
```bash
makepkg -si
```
lub:
```bash
sudo make install
```
lub:
```bash
sudo ./install.sh --system
```

---

## Aktualizacje

Środowisko OKay DE aktualizuje się w 100% w przestrzeni użytkownika (`~/.local/share/okay-de/repo` -> `./install.sh --user`) bez użycia `sudo` czy `pkexec`:
- **Graficznie:** Otwórz `OKay Settings` (skrót `Super + I`), przejdź do zakładki **System i Aktualizacje** i kliknij **Sprawdź aktualizacje** -> **Pobierz i zainstaluj** (lub **Z lokalnego folderu...** dla lokalnych źródeł). Istniejąca konfiguracja w `~/.config/wayfire.ini` i `~/.config/okay-de/` zostaje zachowana, Wayfire przeładowuje konfigurację automatycznie, a `okay-panel` i `okay-desktop` restartują się w tle.
- **Z wiersza poleceń:**
  ```bash
  git pull origin main
  ./install.sh --user
  ```
