# OKay DE

A standalone, modular Wayland Desktop Environment compositor written in **Rust** using the **Smithay** crate (`DRM/KMS + GBM + EGL/OpenGL ES + libseat + libinput + Wayland protocols`).

## Architecture

OKay DE is a 100% native Smithay-based Wayland compositor with zero external compositor or `wlroots` runtime dependencies:

- `src/main.rs`: Entry point, `calloop` event loop, Wayland socket setup (`WAYLAND_DISPLAY`), and automatic backend selection:
  - **DRM/KMS + GBM + EGL/GLES + libseat + libinput** for bare-metal TTY sessions (`--drm`) with `udev` monitor hotplug (`Added` / `Changed` / `Removed`).
  - **Winit** nested backend (`--winit`) for rapid development and debugging inside an existing Wayland/X11 session.
- `src/state.rs`: Central `OkayState` holding Wayland protocol states, seat/input tracking, window registry, focus stack, 9-workspace manager (`Floating`, `MasterStack`, `Dwindle`), window snapping (`SnapZone`), and `CompositorTheme`.
- `src/shell.rs`: `wl_compositor`, `wl_shm`, `xdg-shell` (toplevel, popup, server-side decoration negotiation), `wlr-layer-shell`, `wl_data_device`, and `wlr-data-control` clipboard handlers.
- `src/screencopy.rs`: Native `zwlr_screencopy_manager_v1` (`wlr-screencopy-unstable-v1`) implementation for `grim`, `slurp`, and `wf-recorder`.
- `src/input.rs`: `libinput` / `winit` input handling (global keybindings, relative/absolute pointer motion, click-to-focus, window move/resize grabs, scroll axis forwarding).
- `src/render.rs`: OpenGL ES rendering pipeline (`GlesRenderer` + `OutputDamageTracker`) with offscreen rounded-corner window clipping, SDF drop shadows, and front-to-back layer composition.
- `src/desktop.rs`: Desktop wallpaper loader (`~/.config/okay-de/wallpaper.png`) with cover scaling and built-in aurora gradient fallback.
- `src/animation.rs`: 60+ FPS animations for window open/move, workspace sliding, dock icon bounce, and `MagicLamp` / `ScaleGlide` minimize/restore effects.
- `src/icons.rs`: XDG icon theme resolver (`Papirus`, `Adwaita`, `hicolor`, `pixmaps`) with non-blocking PNG/SVG (`rsvg-convert`) decoding and vector squircle fallbacks.
- `src/panel.rs`: Native top/bottom panel, floating dock, Start Menu app launcher with live search, server-side titlebars, and `~/.config/okay-de/config.toml` persistence (including `autostart`).
- `src/settings.rs`: Built-in **OKay Settings** window with 8 tabs (`Appearance & Anim`, `Displays`, `Wi-Fi & Network`, `Bluetooth`, `Sound`, `Mouse & Keyboard`, `Default Apps`, `System Info`).
- `src/overlays.rs`: Notification toasts & drawer, calendar popup, clipboard history (`Super+V`), control center, `Alt+Tab` switcher, lockscreen, desktop icons grid, and context menus.
- `src/worker.rs`: Background worker thread connected via `calloop::channel::Channel` so icon rasterization, Wi-Fi/Bluetooth scans, system info, clipboard pipes, and `grim` never block rendering or input.

## Dependencies (Arch Linux)

```bash
# Build & core runtime dependencies
sudo pacman -S --needed rust cargo systemd-libs libinput seatd mesa libdrm pixman libxkbcommon wayland

# Optional runtime tools for full DE integration
sudo pacman -S --needed librsvg grim slurp wf-recorder wl-clipboard networkmanager bluez-utils wireplumber foot thunar
```

## Build & Install

```bash
# Build in debug or release mode
cargo build --release

# Run nested inside an existing Wayland/X11 session (auto-detected or explicit)
cargo run -- --winit

# Run on bare-metal TTY via DRM/KMS + libseat
./target/release/okay-de --drm

# Install binary and Wayland display-manager session entry (SDDM / GDM / greetd)
sudo install -Dm755 target/release/okay-de /usr/local/bin/okay-de
sudo install -Dm644 okay-de.desktop /usr/share/wayland-sessions/okay-de.desktop
```

## Configuration & Autostart

Preferences are stored in `~/.config/okay-de/config.toml` (automatically updated by **OKay Settings**). You can also define background `autostart` commands launched after `WAYLAND_DISPLAY` is initialized:

```toml
# ~/.config/okay-de/config.toml
autostart = "/usr/lib/polkit-gnome/polkit-gnome-authentication-agent-1"
autostart = "wl-paste --watch cliphist store"
```

## Tiling Modes & Window Management

Press `Super + T` to cycle the workspace layout mode:
- **Floating (`FLT`)**: Freeform window placement with interactive drag/resize and edge snapping.
- **Master-Stack (`TILE`)**: Primary master window on the left with stacked secondary windows on the right.
- **Dwindle (`DWND`)**: Binary space partitioning spiral layout.

## Default Keybindings

| Shortcut | Action |
| :--- | :--- |
| `Super + Return` | Spawn terminal (`foot`, `kitty`, `alacritty`, `wezterm`, `xterm`) |
| `Super + D` | Toggle Start Menu / App Launcher (type to filter) |
| `Super + I` | Toggle **OKay Settings** window |
| `Super + V` | Toggle Clipboard History Manager |
| `Super + L` | Lock Screen |
| `Super + S` / `Print` | Take Screenshot (`~/Pictures/Screenshots/`) |
| `Super + Q` | Close focused window |
| `Super + F` | Toggle fullscreen on focused window |
| `Super + T` | Cycle layout mode (`Floating` / `MasterStack` / `Dwindle`) |
| `Super + A` | Toggle Always-on-Top for focused window |
| `Super + Left / Right / Up / Down` | Snap window (Left half / Right half / Maximize / Bottom half) |
| `Super + Tab` / `Alt + Tab` | Cycle window focus with live switcher overlay |
| `Super + 1..9` | Switch to workspace `1..9` |
| `Super + Shift + 1..9` | Move focused window to workspace `1..9` |
| `Super + Escape` | Exit compositor cleanly |
