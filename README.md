# OKay DE

Ultra-lekkie, nowoczesne środowisko graficzne Wayland dla dystrybucji **OKsavi Linux**, oparte na kompozytorze `labwc`.

---

## Stos technologiczny

| Rola | Komponent | Opis |
| :--- | :--- | :--- |
| **Kompozytor Wayland** | `labwc` | Stacking compositor zgodny z protokołem wlroots i składnią Openbox |
| **Panel / Pasek statusu** | `waybar` | Wysoce konfigurowalny pasek Wayland z obsługą modułów taskbar, pulseaudio, net, ram, bat |
| **Launcher aplikacji** | `fuzzel` | Błyskawiczny launcher aplikacji i menu dmenu z akceleracją Wayland |
| **Emulator terminala** | `foot` | Ultra-lekki terminal Wayland z natywnym renderowaniem |
| **Menedżer plików** | `pcmanfm` | Szybki menedżer plików |
| **Demon powiadomień** | `mako` | Lekki demon powiadomień OSD dla środowisk wlroots |
| **Tapeta pulpitu** | `swaybg` | Narzędzie do ustawiania tła pulpitu |
| **Audio** | `wpctl` / PipeWire | Nowoczesna kontrola dźwięku i multimediów |
| **Podświetlenie ekranu** | `brightnessctl` | Kontrola jasności matrycy laptopa |
| **Zrzuty ekranu** | `grim` + `slurp` | Narzędzia do przechwytywania zaznaczonego obszaru |
| **Menedżer schowka** | `wl-clipboard` + `cliphist` | Historia schowka z obsługą tekstu i obrazów |
| **Zarządzanie zasobami**| `oksavi-ghost-runner` | Optymalizacja pamięci (cgroups v2 + zRAM) |

---

## Paleta barw (Catppuccin Mocha)

Środowisko posiada zunifikowaną identyfikację wizualną we wszystkich komponentach:
- **Tło główne:** `#11111b` (Crust / Dark Slate)
- **Tło paneli i okien:** `#1e1e2e` (Base)
- **Kolor akcentu:** `#89b4fa` (Blue / Sapphire)
- **Tekst główny:** `#cdd6f4` (Text)
- **Obramowania i separatory:** `#313244` (Surface0)
- **Wskaźnik ostrzeżeń/błędów:** `#f38ba8` (Red)

---

## Skróty klawiszowe

| Skrót | Akcja | Polecenie / Opis |
| :--- | :--- | :--- |
| `Super + Return` | Uruchom terminal | `foot` |
| `Super + Spacja` | Wyszukiwarka aplikacji | `fuzzel` |
| `Super + E` | Menedżer plików | `pcmanfm` |
| `Super + V` | Historia schowka | `okay-clipboard` (cliphist + fuzzel) |
| `Super + Q` | Zamknij aktywne okno | Akcja `Close` |
| `Super + Shift + E` | Menu zasilania (Powermenu)| `okay-powermenu` |
| `PrintScreen` | Zrzut ekranu do schowka i pliku | `okay-screenshot` (`grim` + `slurp`) |
| `XF86AudioRaiseVolume` | Głośniej (+5%) | `wpctl set-volume -l 1.5 @DEFAULT_AUDIO_SINK@ 5%+` |
| `XF86AudioLowerVolume` | Ciszej (-5%) | `wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-` |
| `XF86AudioMute` | Wycisz / Odcisz | `wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle` |
| `XF86MonBrightnessUp` | Rozjaśnij ekran (+5%) | `brightnessctl set +5%` |
| `XF86MonBrightnessDown` | Przyciemnij ekran (-5%)| `brightnessctl set 5%-` |
| `Prawy Przycisk Myszy na pulpicie` | Menu podręczne | Wbudowane `menu.xml` |

---

## Instalacja

### Opcja 1: Instalacja systemowa (rekomendowana dla OKsavi Linux)
Wdraża konfiguracje do `/etc/xdg/` oraz skrypty do `/usr/bin/`:
```bash
sudo make install
```
lub:
```bash
sudo ./install.sh --system
```

### Opcja 2: Instalacja w profilu użytkownika
Kopiuje konfiguracje do `~/.config/` oraz skrypty do `~/.local/bin/`:
```bash
./install.sh --user
```
