# OKay DE

Ultra-lekkie, nowoczesne środowisko graficzne Wayland dla dystrybucji **OKsavi Linux**, oparte na kompozytorze `labwc` z minimalistycznym pływającym dockiem w stylu macOS / GNOME, interaktywnym pulpitem z ikonami plików (`pcmanfm --desktop`) oraz natywnym centrum sterowania **OKay Settings** (PyQt6).

---

## Interfejs i komponenty

- **Interaktywny pulpit (`pcmanfm --desktop`):** Obsługa ikon plików i skrótów aplikacji na pulpicie (`~/Desktop`), obsługa tapety w tle, menu kontekstowe oraz automatyczne generowanie katalogów użytkownika przez `xdg-user-dirs-update`.
- **Domyślne ikony na pulpicie:** Szablon w `/etc/skel/Desktop/` z gotowymi skrótami:
  - Terminal (`foot`)
  - Przeglądarka plików (`pcmanfm`)
  - Centrum sterowania (`okay-settings`)
- **Centrum Sterowania (OKay Settings):** Nowoczesna aplikacja w Pythonie 3 oparta o PyQt6 (Qt 6), umożliwiająca:
  - Zmianę tapety pulpitu i stylu paska zadań (pływająca wyspa vs pełna belka).
  - Zarządzanie przypiętymi aplikacjami w docku (Taskbar).
  - Dynamiczną zmianę kolorów prostokątnych przycisków okien (zamknij, maksymalizuj, minimalizuj) z natychmiastowym przeładowaniem kompozytora `labwc`.
  - Szybki dostęp do konfiguracji ekranów (`wdisplays`) i miksera (`pavucontrol`).
  - Sprawdzanie i instalację aktualizacji bezpośrednio z repozytorium GitHub (`h3v3n/okay-de`).
  - Szczegółowe informacje o systemie (CPU, RAM, kernel, dystrybucja, wersja commita).
- **Architektura Server-Side Decorations (SSD):** Belka kompozytora `labwc` o wysokości 30px (`titlebar.height: 30`) stanowi nadrzędną ramkę na zewnątrz każdego okna. Dzięki zmiennym sesyjnym `GTK_CSD=0` oraz `QT_WAYLAND_DISABLE_WINDOWDECORATION=0` aplikacje GTK3/GTK4 i Qt nie wstrzykują przycisków sterowania do pasków zakładek i nagłówków.
- **Dekoracje okien:** Trzy spłaszczone, panoramiczne prostokąty (48x10px) w układzie `:iconify,max,close` z zaokrągleniem narożników `8px` (`cornerRadius: 8`) bez wyświetlania nazwy okna (`showTitle: no`). Lewy przycisk minimalizuje okno, środkowy maksymalizuje, a prawy zamyka program.
- **Tapeta i tło pulpitu (`swaybg`):** Demon tapety Wayland z bezpieczną kaskadą: tapeta użytkownika (`~/.config/okay-de/wallpaper.png`) -> tapeta systemowa (`/usr/share/backgrounds/okay-de/default.png`) -> awaryjny kolor bazowy Catppuccin Mocha (`#1e1e2e`).
- **Pasek Zadań OKay (`sfwbar`):** Oficjalny, ultra-lekki pasek zadań w C z natywną obsługą `wlr-layer-shell` oraz `wlr-foreign-toplevel`. Zawiera menu programów (`󰣇`), dynamiczny taskbar z białą kreską statusu pod otwartymi oknami, powiadomienia, aplet głośności z regulacją rolką, dwuwierszowy zegar KDE Plasma oraz przycisk „Pokaż pulpit”.
- **Kafelkowanie (Snapping / Tiling):** Wbudowane przeciąganie okien myszą do krawędzi ekranu oraz skróty `Super + Strzałki`.
- **Motyw i ikony:** Catppuccin Mocha z akcentem Sapphire/Blue (`#89b4fa`), zestaw ikon `Papirus-Dark` oraz kursor `Adwaita (24px)`.

---

## Stos technologiczny

| Rola | Komponent | Opis |
| :--- | :--- | :--- |
| **Kompozytor Wayland** | `labwc` | Stacking compositor zgodny z protokołem wlroots i Openbox XML |
| **Pulpit z ikonami** | `pcmanfm --desktop` | Interaktywny pulpit z obsługą ikon plików i tła |
| **Centrum Sterowania** | `okay-settings` | Natywna aplikacja Pythona 3 (PyQt6) |
| **Pasek zadań (Taskbar)** | `sfwbar` | Pasek w C z obsługą wlr-layer-shell, Catppuccin Mocha i zegarem KDE |
| **Launcher aplikacji** | `fuzzel` | Błyskawiczny launcher aplikacji z akceleracją Wayland |
| **Emulator terminala** | `foot` | Ultra-lekki terminal Wayland z przezroczystością tła `0.95` |
| **Menedżer plików** | `pcmanfm` | Szybki menedżer plików z ciemnym motywem GTK |
| **Demon powiadomień** | `mako` | Lekki demon powiadomień OSD w prawym górnym rogu ekranu |
| **Dźwięk / OSD** | `wpctl` + `okay-volume` | Regulacja głośności z graficznym paskiem postępu |
| **Jasność matrycy** | `brightnessctl` + `okay-brightness` | Regulacja podświetlenia z paskiem OSD |
| **Zrzuty ekranu** | `okay-screenshot` | `grim` + `slurp` do schowka, pliku i z powiadomieniem |
| **Menedżer schowka** | `okay-clipboard` | `cliphist` + `fuzzel` + `wl-clipboard` (tekst i obrazy) |
| **Blokada ekranu** | `okay-lock` | Estetyczny `swaylock` w motywie Catppuccin Mocha |
| **Zarządzanie zasobami**| `oksavi-ghost-runner` | Optymalizacja pamięci (cgroups v2 + zRAM) |

---

## Skróty klawiszowe

| Skrót | Akcja | Polecenie / Opis |
| :--- | :--- | :--- |
| `Super + Return` | Uruchom terminal | `foot` |
| `Super + Spacja` | Wyszukiwarka aplikacji | `fuzzel` |
| `Super + M` | Menu główne środowiska | `ShowMenu root-menu` |
| `Super + I` | Centrum sterowania | `okay-settings` |
| `Super + E` | Menedżer plików | `pcmanfm` |
| `Super + V` | Historia schowka | `okay-clipboard` |
| `Super + L` | Zablokuj ekran | `okay-lock` |
| `Super + Q` | Zamknij aktywne okno | Akcja `Close` |
| `Super + Left` | Kafelkowanie do lewej | `SnapToEdge left` |
| `Super + Right` | Kafelkowanie do prawej | `SnapToEdge right` |
| `Super + Up` | Maksymalizuj okno | `ToggleMaximize` |
| `Super + Down` | Minimalizuj okno | `Iconify` |
| `Super + Shift + E` | Menu zasilania (Powermenu)| `okay-powermenu` |
| `PrintScreen` | Zrzut ekranu do pliku i schowka | `okay-screenshot` |
| `XF86AudioRaiseVolume` | Głośniej (+5%) z OSD | `okay-volume raise` |
| `XF86AudioLowerVolume` | Ciszej (-5%) z OSD | `okay-volume lower` |
| `XF86AudioMute` | Wycisz / Odcisz z OSD | `okay-volume mute` |
| `XF86MonBrightnessUp` | Rozjaśnij ekran (+5%) z OSD | `okay-brightness up` |
| `XF86MonBrightnessDown` | Przyciemnij ekran (-5%) z OSD | `okay-brightness down` |
| `Prawy Przycisk Myszy` | Menu podręczne pulpitu | `menu.xml` (Otwórz Pulpit, Ustawienia itp.) |

---

## Wymagane pakiety

### Pakiety bazowe (OKsavi / Arch Linux):
```bash
sudo pacman -S labwc pcmanfm fuzzel foot mako swaylock grim slurp wl-clipboard cliphist brightnessctl wireplumber papirus-icon-theme xdg-user-dirs python python-pyqt6 qt6-wayland
```

### Pakiety dla OKay Settings:
```bash
sudo pacman -S python python-pyqt6 qt6-wayland
```

### Sugerowane pakiety opcjonalne:
```bash
sudo pacman -S wdisplays pavucontrol networkmanager
```

---

## Instalacja

### Opcja 1: Instalacja systemowa (rekomendowana)
Wdraża konfiguracje do `/etc/xdg/`, skrypty do `/usr/bin/` i `/usr/local/bin/`, szablony do `/etc/skel/Desktop/` oraz wpisy `.desktop`:
```bash
sudo make install
```
lub:
```bash
sudo ./install.sh --system
```

### Opcja 2: Instalacja w profilu użytkownika
Kopiuje konfiguracje do `~/.config/`, skrypty do `~/.local/bin/`, ikony na `$HOME/Desktop/` oraz pliki `.desktop`:
```bash
./install.sh --user
```

---

## Aktualizacje

Środowisko OKay DE można aktualizować bezpośrednio z oficjalnego repozytorium GitHub:
- **Graficznie:** Otwórz `OKay Settings` (skrót `Super + I`), przejdź do zakładki **Aktualizacje** i kliknij **Sprawdź teraz**. Jeśli dostępna jest nowa wersja, pojawi się lista zmian oraz przycisk **Zainstaluj teraz** z wyborem trybu (użytkownik lub system).
- **Z wiersza poleceń:**
  ```bash
  git pull origin main
  ./install.sh --user   # lub: sudo ./install.sh --system
  labwc --reconfigure
  killall -SIGHUP sfwbar 2>/dev/null || sfwbar &
  ```
