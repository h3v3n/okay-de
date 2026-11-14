# OKay DE

A standalone, modular Wayland Desktop Environment compositor written in **Rust** using the **Smithay** crate (`DRM/KMS + GBM + EGL/OpenGL ES + libinput + Wayland protocols`).

## Architecture

OKay DE is a 100% native Smithay-based Wayland compositor with zero external compositor or `wlroots` dependencies:

- `src/main.rs`: Entry point, `calloop` event loop, Wayland socket setup (`WAYLAND_DISPLAY`), and automatic backend selection:
  - **DRM/KMS + GBM + EGL/GLES + libseat + libinput** for bare-metal TTY sessions.
  - **Winit** nested backend for rapid development and debugging inside an existing Wayland/X11 session.
- `src/state.rs`: Central `OkayState` holding `DisplayHandle`, `SeatState`, `XdgShellState`, `XdgDecorationState`, `WlrLayerShellState`, `ShmState`, window registry, focus stack, 9-workspace manager, and decoupled `CompositorTheme`.
- `src/input.rs`: `libinput` / seat input handling (keyboard shortcuts, relative/absolute pointer motion, click-to-focus, scroll axis forwarding).
- `src/render.rs`: Damage-tracked OpenGL ES rendering pipeline (`GlesRenderer` + `OutputDamageTracker`) rendering background clear color, `wlr-layer-shell` layers, mapped `xdg_toplevel` / `xdg_popup` surfaces, and server-side window borders.
- `src/shell.rs`: Full `xdg-shell` (toplevel, popup, decoration negotiation) and `wlr-layer-shell` protocol handlers.

## Dependencies (Arch Linux)

```bash
sudo pacman -S --needed rust cargo systemd-libs libinput seatd mesa libdrm pixman libxkbcommon wayland
```

## Build & Run

```bash
# Build in debug or release mode
cargo build --release

# Run nested inside an existing Wayland/X11 session (auto-detected or explicit)
cargo run -- --winit

# Run on bare-metal TTY via DRM/KMS + libseat
./target/release/okay-de
```

## Default Keybindings

| Shortcut | Action |
| :--- | :--- |
| `Super + Return` | Spawn terminal (`foot`, `kitty`, `alacritty`, `wezterm`, `xterm`) |
| `Super + Q` | Close focused window |
| `Super + 1..9` | Switch to workspace `1..9` |
| `Super + Shift + 1..9` | Move focused window to workspace `1..9` |
| `Super + Tab` / `Alt + Tab` | Cycle window focus |
| `Super + Escape` | Exit compositor cleanly |
