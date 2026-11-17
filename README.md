# OKay DE

A standalone, modular Wayland Desktop Environment compositor written in **Rust** using the **Smithay** crate (`DRM/KMS + GBM + EGL/OpenGL ES + libseat + libinput + Wayland protocols`).

## Architecture

OKay DE is a 100% native Smithay-based Wayland compositor with zero external compositor or `wlroots` runtime dependencies:

- `src/main.rs`: Entry point, `calloop` event loop, Wayland socket setup (`WAYLAND_DISPLAY`), periodic 5s async panel status polling, native GLES screenshot trigger, and automatic backend selection:
  - **DRM/KMS + GBM + EGL/GLES + libseat + libinput** for bare-metal TTY sessions (`--drm`) with `udev` monitor hotplug (`Added` / `Changed` / `Removed`).
  - **Winit** nested backend (`--winit`) for rapid development and debugging inside an existing Wayland/X11 session.
- `src/state.rs`: Central `OkayState` holding Wayland protocol states, seat/input tracking, window registry, 9-workspace manager (`per_workspace_layouts`: `Floating`, `MasterStack` with adjustable `master_ratio`, `Dwindle`), snap zone detection (`SnapZone` halves, quarters, maximize), directional outer-border resize detection, titlebar actions, and `CompositorTheme`.
- `src/shell.rs`: `wl_compositor`, `wl_shm`, `xdg-shell` (toplevel, popup, maximize/fullscreen state preservation, server-side decoration negotiation), `wlr-layer-shell`, `wl_data_device`, and `wlr-data-control` clipboard handlers.
- `src/screencopy.rs`: Native `zwlr_screencopy_manager_v1` (`wlr-screencopy-unstable-v1`) implementation for `grim`, `slurp`, and `wf-recorder`, plus `capture_native_screenshot` (direct GLES framebuffer capture for full & area screenshots).
- `src/input.rs`: `libinput` / `winit` input handling (global keybindings, hardware `XF86Audio*` media keys, inline folder/rename editing, directional quadrant `Super+RMB` & outer-border resize, `Super+LMB` drag with top-bar boundary clamping and snap preview).
- `src/render.rs`: OpenGL ES rendering pipeline (`GlesRenderer` + `OutputDamageTracker`) with offscreen rounded-corner window clipping, window cache eviction on close, SDF drop shadows, and front-to-back layer composition.
- `src/desktop.rs`: Desktop wallpaper loader (`~/.config/okay-de/wallpaper.png`) with cover scaling and built-in aurora gradient fallback.
- `src/animation.rs`: 60+ FPS animations for window open/move, workspace sliding, parabolic Dock zoom (without overlapping neighbors), dock bounce, and `MagicLamp` / `ScaleGlide` minimize/restore targeting exact dock icons.
- `src/icons.rs`: XDG icon theme resolver (`Papirus`, `Adwaita`, `hicolor`, `pixmaps`) with non-blocking PNG/SVG (`rsvg-convert`) decoding and vector squircle fallbacks.
- `src/panel.rs`: Native top bar with interactive dropdown menus (`File`, `View`, `Window`, `Help`), floating Dock with hover tooltips and right-click Pin/Unpin context menu, Launchpad/macOS-style App Launcher (`Super+D`) with live search and power bar, server-side titlebars, and `~/.config/okay-de/config.toml` persistence (`autostart`, `lock_pin`, pinned apps).
- `src/settings.rs`: Built-in **OKay Settings** window with **9 tabs** (`Appearance & Anim`, `Displays`, `Wi-Fi & Network`, `Bluetooth`, `Sound`, `Mouse & Keyboard`, `Default Apps`, `System Info`, `Keybindings`) and smooth continuous drag sliders.
- `src/overlays.rs`: Notification toasts & drawer, leap-year calendar with month navigation, clipboard history (`Super+V`) with Delete/Clear All, expandable Control Center (Wi-Fi/Bluetooth lists & smooth 0–100% volume slider), live Window Overview (`Super+O`), OSD HUD pill, snap zone preview, interactive Area Screenshot selector (`Super+Shift+S`), `Alt+Tab` switcher with icons & workspace badges, PIN-aware Lock Screen, non-overlapping desktop grid (`New Folder`, `F2` inline rename, `Properties` modal), and Window Context Menu.
- `src/worker.rs`: Background worker thread connected via `calloop::channel::Channel` so icon rasterization, Wi-Fi/Bluetooth scans, system info, battery/audio/Wi-Fi polling, PNG screenshot encoding, clipboard pipes, and `grim` never block rendering or input.

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

Preferences are stored in `~/.config/okay-de/config.toml` (automatically updated by **OKay Settings**). You can also define background `autostart` commands, an optional Lock Screen PIN (`lock_pin`), and `~/.config/okay-de/autostart.sh` launched after `WAYLAND_DISPLAY` is initialized:

```toml
# ~/.config/okay-de/config.toml
autostart = "/usr/lib/polkit-gnome/polkit-gnome-authentication-agent-1"
autostart = "wl-paste --watch cliphist store"
lock_pin = ""
```

## Tiling Modes & Window Management

Each workspace remembers its own layout mode (`Super + T` cycles):
- **Floating (`FLT`)**: Freeform window placement with translucent drag-snap previews, outer-border/corner resize, `Super+LMB` move, `Super+RMB` directional resize, and double-click titlebar maximize.
- **Master-Stack (`TILE`)**: Primary master window on the left with stacked secondary windows on the right; adjust split with `Super + [ / ]` and swap windows with `Super + Shift + J / K`.
- **Dwindle (`DWND`)**: Binary space partitioning spiral layout.

## Default Keybindings & Pointer Controls

| Shortcut | Action |
| :--- | :--- |
| `Super + Return` | Spawn terminal (`foot`, `kitty`, `alacritty`, `wezterm`, `xterm`) |
| `Super + D` | Toggle App Launcher / Launchpad (type to filter, right-click to pin) |
| `Super + O` | Toggle live Window Overview (Exposé) |
| `Super + I` | Toggle **OKay Settings** window (9 tabs incl. Keybindings) |
| `Super + V` | Toggle Clipboard History Manager (`Delete` item / `Clear All`) |
| `Super + L` | Lock Screen (supports optional `lock_pin` + `Return` / `BackSpace`) |
| `Super + S` / `Print` | Native GLES Full Screenshot (`~/Pictures/Screenshots/`) |
| `Super + Shift + S` | Interactive Area Screenshot (drag region, `Esc` / RMB to cancel) |
| `Super + Q` | Close focused window |
| `Super + F` | Toggle fullscreen on focused window |
| `Super + T` | Cycle per-workspace layout (`Floating` / `MasterStack` / `Dwindle`) |
| `Super + A` | Toggle Always-on-Top for focused window |
| `Super + C` | Center focused window in work area |
| `Super + M` / `Super + Shift + M` | Minimize focused window / Restore last minimized |
| `Super + J / K` | Focus next / previous window |
| `Super + Shift + J / K` | Swap window order in tiled stack |
| `Super + [ / ]` (or `- / =`) | Decrease / increase Master-Stack split ratio |
| `Super + Left / Right / Up / Down` | Snap window (Left half / Right half / Maximize / Bottom half) |
| `Super + Tab` / `Alt + Tab` (+`Shift`) | Cycle window focus forward / backward with switcher overlay |
| `Super + 1..9` / `Super + Shift + Left / Right` | Switch workspace `1..9` or step previous / next workspace |
| `Super + Shift + 1..9` | Move focused window to workspace `1..9` |
| `XF86AudioRaise/Lower/Mute` | Adjust volume (`+/- 5%`) or mute with OSD feedback |
| `F2` | Rename currently selected Desktop icon inline |
| `Super + LMB` / `Super + RMB` | Drag window from anywhere / directional quadrant resize |
| `Titlebar 2xClick / MMB / RMB` | Maximize-Restore / Toggle Always-on-Top / Window Context Menu |
| `Super + Escape` | Exit compositor cleanly |
