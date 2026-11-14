use std::{
    collections::HashMap,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
};

use fontdue::{Font, FontSettings, Metrics};
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                Kind,
            },
            gles::GlesRenderer,
        },
    },
    desktop::Window,
    output::Output,
    reexports::wayland_server::Resource,
    utils::{Logical, Physical, Point, Rectangle, Scale, Transform},
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};

use crate::{render::OkayRenderElements, state::OkayState};

pub const PANEL_HEIGHT: i32 = 42;
const MENU_WIDTH: i32 = 290;
const MENU_HEADER_HEIGHT: i32 = 44;
const MENU_ITEM_HEIGHT: i32 = 36;
const MENU_PADDING: i32 = 8;

/// Accent color presets: `(Name, [R, G, B], linear [f32; 4])`.
pub const ACCENT_PRESETS: [(&str, [u8; 3], [f32; 4]); 5] = [
    ("Lavender", [201, 160, 220], [0.788, 0.627, 0.863, 1.0]),
    ("Sky Blue", [137, 180, 250], [0.537, 0.706, 0.980, 1.0]),
    ("Emerald", [166, 227, 161], [0.651, 0.890, 0.631, 1.0]),
    ("Coral", [243, 139, 168], [0.953, 0.545, 0.659, 1.0]),
    ("Amber", [249, 226, 175], [0.976, 0.886, 0.686, 1.0]),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherItem {
    Terminal,
    FileManager,
    Browser,
    SystemMonitor,
    OkaySettings,
    ReloadConfig,
    ExitSession,
}

impl LauncherItem {
    const ALL: [Self; 7] = [
        Self::Terminal,
        Self::FileManager,
        Self::Browser,
        Self::SystemMonitor,
        Self::OkaySettings,
        Self::ReloadConfig,
        Self::ExitSession,
    ];

    fn badge(self) -> &'static str {
        match self {
            Self::Terminal => ">_",
            Self::FileManager => "FM",
            Self::Browser => "WB",
            Self::SystemMonitor => "SM",
            Self::OkaySettings => "OS",
            Self::ReloadConfig => "RC",
            Self::ExitSession => "EX",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::FileManager => "File Manager",
            Self::Browser => "Web Browser",
            Self::SystemMonitor => "System Monitor",
            Self::OkaySettings => "OKay Settings",
            Self::ReloadConfig => "Reload Config",
            Self::ExitSession => "Exit Session",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Terminal => "Super+Ret",
            Self::FileManager => "Files",
            Self::Browser => "Internet",
            Self::SystemMonitor => "Resources",
            Self::OkaySettings => "Super+I",
            Self::ReloadConfig => "Wallpaper",
            Self::ExitSession => "Super+Esc",
        }
    }
}

#[derive(Debug, Clone)]
enum ClickTarget {
    ToggleLauncher,
    ToggleSettings,
    ToggleTiling,
    Workspace(usize),
    FocusWindow(usize),
    LaunchItem(LauncherItem),
}

#[derive(Debug)]
pub struct PanelState {
    pub launcher_open: bool,
    pub show_workspaces: bool,
    pub dock_mode: bool,
    pub accent_preset: usize,
    hit_regions: Vec<(Rectangle<i32, Logical>, ClickTarget)>,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            launcher_open: false,
            show_workspaces: true,
            dock_mode: false,
            accent_preset: 0,
            hit_regions: Vec::new(),
        }
    }
}

impl PanelState {
    pub fn new() -> Self {
        let mut s = Self::default();
        let _ = s.load_config_file();
        s
    }

    pub fn accent_rgb(&self) -> [u8; 3] {
        ACCENT_PRESETS[self.accent_preset % ACCENT_PRESETS.len()].1
    }

    pub fn accent_f32(&self) -> [f32; 4] {
        ACCENT_PRESETS[self.accent_preset % ACCENT_PRESETS.len()].2
    }

    /// Reads `~/.config/okay-de/config.toml` and returns `(pointer_speed, natural_scroll)` if set.
    pub fn load_config_file(&mut self) -> (f64, bool) {
        let mut pointer_speed = 1.0_f64;
        let mut natural_scroll = false;
        let Some(path) = config_file_path() else {
            return (pointer_speed, natural_scroll);
        };
        let Ok(content) = fs::read_to_string(path) else {
            return (pointer_speed, natural_scroll);
        };
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let key = k.trim();
                let val = v.trim().trim_matches('"');
                match key {
                    "dock_mode" => self.dock_mode = val == "true",
                    "show_workspaces" => self.show_workspaces = val != "false",
                    "accent_preset" => {
                        if let Ok(idx) = val.parse::<usize>() {
                            self.accent_preset = idx % ACCENT_PRESETS.len();
                        }
                    }
                    "pointer_speed" => {
                        if let Ok(spd) = val.parse::<f64>() {
                            pointer_speed = spd.clamp(0.25, 2.5);
                        }
                    }
                    "natural_scroll" => natural_scroll = val == "true",
                    _ => {}
                }
            }
        }
        (pointer_speed, natural_scroll)
    }

    /// Saves current panel & input preferences to `~/.config/okay-de/config.toml`.
    pub fn save_config_file(&self, pointer_speed: f64, natural_scroll: bool) {
        let Some(path) = config_file_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let body = format!(
            "# OKay DE Configuration\ndock_mode = {}\nshow_workspaces = {}\naccent_preset = {}\npointer_speed = {:.2}\nnatural_scroll = {}\n",
            self.dock_mode,
            self.show_workspaces,
            self.accent_preset,
            pointer_speed,
            natural_scroll
        );
        let _ = fs::write(path, body);
    }
}

fn config_file_path() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("okay-de/config.toml"));
        }
    }
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".config/okay-de/config.toml"))
}

/// Spawns the first available terminal emulator installed on the system.
pub fn spawn_terminal(socket_name: &OsStr) {
    let candidates = [
        "foot",
        "kitty",
        "alacritty",
        "wezterm",
        "konsole",
        "gnome-terminal",
        "xterm",
    ];
    spawn_first_available(&candidates, socket_name, "terminal");
}

fn spawn_first_available(candidates: &[&str], socket_name: &OsStr, kind: &str) {
    for bin in candidates {
        match Command::new(bin).env("WAYLAND_DISPLAY", socket_name).spawn() {
            Ok(_) => {
                tracing::info!(app = bin, kind, "Spawned application");
                return;
            }
            Err(_) => continue,
        }
    }
    tracing::warn!(?candidates, kind, "No matching application found in PATH");
}

fn spawn_system_monitor(socket_name: &OsStr) {
    let gui_monitors = ["gnome-system-monitor", "plasma-systemmonitor", "ksysguard", "missioncenter"];
    for bin in gui_monitors {
        if Command::new(bin).env("WAYLAND_DISPLAY", socket_name).spawn().is_ok() {
            return;
        }
    }
    for term in ["foot", "kitty", "alacritty", "xterm"] {
        for tui in ["btop", "htop", "top"] {
            if Command::new(term)
                .arg("-e")
                .arg(tui)
                .env("WAYLAND_DISPLAY", socket_name)
                .spawn()
                .is_ok()
            {
                return;
            }
        }
    }
}

fn execute_launcher_item(state: &mut OkayState, item: LauncherItem) {
    state.panel.launcher_open = false;
    match item {
        LauncherItem::Terminal => spawn_terminal(&state.socket_name),
        LauncherItem::FileManager => spawn_first_available(
            &["pcmanfm", "thunar", "nemo", "dolphin", "nautilus"],
            &state.socket_name,
            "file-manager",
        ),
        LauncherItem::Browser => spawn_first_available(
            &["firefox", "chromium", "brave", "floorp", "xdg-open"],
            &state.socket_name,
            "browser",
        ),
        LauncherItem::SystemMonitor => spawn_system_monitor(&state.socket_name),
        LauncherItem::OkaySettings => {
            state.settings.open = true;
            state.settings.refresh_system_info();
        }
        LauncherItem::ReloadConfig => {
            let (spd, nat) = state.panel.load_config_file();
            state.pointer_sensitivity = spd;
            state.natural_scroll = nat;
            state.theme.active_border_color = state.panel.accent_f32();
            state.desktop.reload();
            state.retile_workspace();
        }
        LauncherItem::ExitSession => {
            tracing::info!("Exit Session selected from taskbar menu");
            state.running = false;
            state.loop_signal.stop();
        }
    }
}

/// Handles a left-click at global logical coordinates. Returns `true` if the panel or
/// launcher menu consumed the click.
pub fn handle_panel_click(state: &mut OkayState, global_pos: Point<f64, Logical>) -> bool {
    let px = global_pos.x.round() as i32;
    let py = global_pos.y.round() as i32;

    let mut matched: Option<ClickTarget> = None;
    for (rect, target) in state.panel.hit_regions.iter().rev() {
        if px >= rect.loc.x
            && px < rect.loc.x + rect.size.w
            && py >= rect.loc.y
            && py < rect.loc.y + rect.size.h
        {
            matched = Some(target.clone());
            break;
        }
    }

    if let Some(target) = matched {
        match target {
            ClickTarget::ToggleLauncher => {
                state.panel.launcher_open = !state.panel.launcher_open;
            }
            ClickTarget::ToggleSettings => {
                state.panel.launcher_open = false;
                state.settings.open = !state.settings.open;
                if state.settings.open {
                    state.settings.refresh_system_info();
                }
            }
            ClickTarget::ToggleTiling => {
                state.panel.launcher_open = false;
                state.toggle_tiling();
            }
            ClickTarget::Workspace(ws) => {
                state.panel.launcher_open = false;
                state.switch_workspace(ws);
            }
            ClickTarget::FocusWindow(win_idx) => {
                state.panel.launcher_open = false;
                let ws_windows: Vec<Window> = state
                    .space
                    .elements()
                    .filter(|w| state.workspaces.is_on_active_workspace(w))
                    .cloned()
                    .collect();
                if let Some(win) = ws_windows.get(win_idx) {
                    let is_focused = state.focused_window().as_ref() == Some(win);
                    let is_min = state.workspaces.is_minimized(win);
                    if is_min || is_focused {
                        state.toggle_minimize_window(win);
                    } else {
                        state.focus_window(win);
                    }
                }
            }
            ClickTarget::LaunchItem(item) => {
                execute_launcher_item(state, item);
            }
        }
        return true;
    }

    if state.panel.launcher_open {
        state.panel.launcher_open = false;
    }

    false
}

/// Rebuilds interactive hit-regions and renders the native taskbar/dock (+ launcher popup if open)
/// for the given `output`.
pub fn render_panel_elements(
    state: &mut OkayState,
    renderer: &mut GlesRenderer,
    output: &Output,
    scale: Scale<f64>,
) -> Vec<OkayRenderElements<GlesRenderer>> {
    let Some(output_geo) = state.space.output_geometry(output) else {
        return Vec::new();
    };
    if output_geo.size.w <= 0 || output_geo.size.h <= PANEL_HEIGHT + 12 {
        return Vec::new();
    }

    let is_primary = state
        .space
        .outputs()
        .next()
        .map(|o| o == output)
        .unwrap_or(false);
    if is_primary {
        state.panel.hit_regions.clear();
    }

    let accent = state.panel.accent_rgb();
    let pointer_x = state.pointer_location.x.round() as i32;
    let pointer_y = state.pointer_location.y.round() as i32;

    let focused_id = state
        .seat
        .get_keyboard()
        .and_then(|k| k.current_focus())
        .map(|s| s.id());

    let ws_windows: Vec<(String, bool, bool)> = state
        .space
        .elements()
        .filter(|w| state.workspaces.is_on_active_workspace(w))
        .map(|w| {
            let is_min = state.workspaces.is_minimized(w);
            let is_focused = !is_min
                && w.toplevel()
                    .map(|t| Some(t.wl_surface().id()) == focused_id)
                    .unwrap_or(false);
            let title = window_title(w);
            (title, is_focused, is_min)
        })
        .collect();

    let occupied_workspaces: Vec<bool> = (1..=9)
        .map(|ws| {
            state.space.elements().any(|w| {
                w.toplevel()
                    .map(|t| state.workspaces.window_workspace(&t.wl_surface().id()) == ws)
                    .unwrap_or(false)
            })
        })
        .collect();

    let (bar_x_local, bar_y_local, bar_w, bar_h) = if state.panel.dock_mode {
        let ws_section_w = if state.panel.show_workspaces { 9 * 28 + 10 } else { 0 };
        let win_section_w = (ws_windows.len() as i32 * 148).clamp(120, 560);
        let desired_w = (84 + ws_section_w + win_section_w + 190).clamp(420, (output_geo.size.w - 32).max(320));
        let bx = (output_geo.size.w - desired_w) / 2;
        let by = output_geo.size.h - PANEL_HEIGHT - 8;
        (bx, by, desired_w, PANEL_HEIGHT)
    } else {
        (0, output_geo.size.h - PANEL_HEIGHT, output_geo.size.w, PANEL_HEIGHT)
    };

    let mut elements = Vec::new();
    let mut canvas = Canvas::new(bar_w, bar_h);

    if state.panel.dock_mode {
        canvas.fill_rounded_rect(0, 0, bar_w, bar_h, 11, [20, 20, 32, 238]);
        canvas.fill_rect(12, 0, bar_w - 24, 2, [accent[0], accent[1], accent[2], 180]);
    } else {
        canvas.fill_rect(0, 0, bar_w, bar_h, [18, 18, 28, 244]);
        canvas.fill_rect(0, 0, bar_w, 1, [accent[0], accent[1], accent[2], 140]);
    }

    let mut cursor_x = 10;

    // 1. "OKay" Start Button
    let start_w = 72;
    let start_global = Rectangle::new(
        (output_geo.loc.x + bar_x_local + cursor_x, output_geo.loc.y + bar_y_local + 6).into(),
        (start_w, bar_h - 12).into(),
    );
    let start_hovered = rect_contains(start_global, pointer_x, pointer_y);
    let start_bg = if state.panel.launcher_open {
        [accent[0], accent[1], accent[2], 255]
    } else if start_hovered {
        [62, 58, 86, 255]
    } else {
        [40, 40, 58, 255]
    };
    let start_fg = if state.panel.launcher_open {
        [16, 16, 24, 255]
    } else {
        [240, 242, 252, 255]
    };
    canvas.fill_rounded_rect(cursor_x, 6, start_w, bar_h - 12, 6, start_bg);
    canvas.draw_text_px(cursor_x + 15, 12, "OKay", 15.0, start_fg);
    state.panel.hit_regions.push((start_global, ClickTarget::ToggleLauncher));
    cursor_x += start_w + 10;

    // 2. Workspace Indicators (1..9) if enabled
    if state.panel.show_workspaces {
        for ws in 1..=9 {
            let ws_w = 24;
            let is_active = state.workspaces.active == ws;
            let is_occupied = occupied_workspaces[ws - 1];
            let ws_global = Rectangle::new(
                (output_geo.loc.x + bar_x_local + cursor_x, output_geo.loc.y + bar_y_local + 7).into(),
                (ws_w, bar_h - 14).into(),
            );
            let hovered = rect_contains(ws_global, pointer_x, pointer_y);

            let bg = if is_active {
                [accent[0], accent[1], accent[2], 255]
            } else if hovered {
                [58, 58, 82, 255]
            } else if is_occupied {
                [42, 42, 62, 255]
            } else {
                [28, 28, 42, 220]
            };
            let fg = if is_active {
                [16, 16, 24, 255]
            } else if is_occupied || hovered {
                [235, 236, 248, 255]
            } else {
                [130, 134, 160, 255]
            };

            canvas.fill_rounded_rect(cursor_x, 7, ws_w, bar_h - 14, 5, bg);
            let label = ws.to_string();
            canvas.draw_text_px(cursor_x + 8, 12, &label, 14.0, fg);
            state.panel.hit_regions.push((ws_global, ClickTarget::Workspace(ws)));
            cursor_x += ws_w + 4;
        }
        cursor_x += 8;
    }

    // 3. Right-side System Tray: Clock + Layout Mode + Settings button
    let clock_str = current_hh_mm();
    let clock_w = 68;
    let clock_x = (bar_w - clock_w - 10).max(cursor_x);
    canvas.fill_rounded_rect(clock_x, 6, clock_w, bar_h - 12, 6, [32, 32, 48, 235]);
    canvas.draw_text_px(clock_x + 12, 12, &clock_str, 14.0, [235, 238, 250, 255]);

    let settings_w = 34;
    let settings_x = (clock_x - settings_w - 6).max(cursor_x);
    if settings_x + settings_w <= clock_x - 4 {
        let set_global = Rectangle::new(
            (output_geo.loc.x + bar_x_local + settings_x, output_geo.loc.y + bar_y_local + 6).into(),
            (settings_w, bar_h - 12).into(),
        );
        let set_bg = if state.settings.open {
            [accent[0], accent[1], accent[2], 255]
        } else if rect_contains(set_global, pointer_x, pointer_y) {
            [56, 56, 80, 255]
        } else {
            [34, 34, 50, 235]
        };
        let set_fg = if state.settings.open {
            [16, 16, 24, 255]
        } else {
            [225, 228, 245, 255]
        };
        canvas.fill_rounded_rect(settings_x, 6, settings_w, bar_h - 12, 6, set_bg);
        canvas.draw_text_px(settings_x + 8, 12, "OS", 13.0, set_fg);
        state.panel.hit_regions.push((set_global, ClickTarget::ToggleSettings));
    }

    let mode_label = if state.workspaces.tiling_enabled { "TILE" } else { "FLT" };
    let status_w = 52;
    let status_x = (settings_x - status_w - 6).max(cursor_x);
    if status_x + status_w <= settings_x - 4 {
        let status_global = Rectangle::new(
            (output_geo.loc.x + bar_x_local + status_x, output_geo.loc.y + bar_y_local + 6).into(),
            (status_w, bar_h - 12).into(),
        );
        let status_bg = if state.workspaces.tiling_enabled {
            [56, 46, 78, 245]
        } else {
            [34, 34, 50, 235]
        };
        canvas.fill_rounded_rect(status_x, 6, status_w, bar_h - 12, 6, status_bg);
        canvas.draw_text_px(status_x + 9, 12, mode_label, 13.0, [accent[0], accent[1], accent[2], 255]);
        state.panel.hit_regions.push((status_global, ClickTarget::ToggleTiling));
    }

    // 4. Open Window Task Buttons
    let avail_w = (status_x - cursor_x - 8).max(0);
    if !ws_windows.is_empty() && avail_w > 48 {
        let max_btn_w = 175;
        let btn_w = ((avail_w / ws_windows.len() as i32) - 6).clamp(54, max_btn_w);
        for (idx, (title, is_focused, is_min)) in ws_windows.iter().enumerate() {
            let bx = cursor_x + idx as i32 * (btn_w + 6);
            if bx + btn_w > status_x - 4 {
                break;
            }
            let win_global = Rectangle::new(
                (output_geo.loc.x + bar_x_local + bx, output_geo.loc.y + bar_y_local + 6).into(),
                (btn_w, bar_h - 12).into(),
            );
            let hovered = rect_contains(win_global, pointer_x, pointer_y);
            let bg = if *is_focused {
                [56, 50, 78, 255]
            } else if hovered {
                [46, 46, 68, 245]
            } else if *is_min {
                [24, 24, 36, 200]
            } else {
                [32, 32, 48, 235]
            };
            let fg = if *is_min {
                [145, 148, 172, 255]
            } else {
                [238, 240, 252, 255]
            };
            canvas.fill_rounded_rect(bx, 6, btn_w, bar_h - 12, 6, bg);
            if *is_focused {
                canvas.fill_rect(bx + 6, bar_h - 8, btn_w - 12, 2, [accent[0], accent[1], accent[2], 255]);
            }
            let max_chars = ((btn_w - 18) / 8).max(2) as usize;
            let short: String = title.chars().take(max_chars).collect();
            canvas.draw_text_px(bx + 10, 12, &short, 13.5, fg);
            state.panel.hit_regions.push((win_global, ClickTarget::FocusWindow(idx)));
        }
    }

    let bar_buf = MemoryRenderBuffer::from_slice(
        &canvas.pixels,
        Fourcc::Abgr8888,
        (bar_w, bar_h),
        1,
        Transform::Normal,
        None,
    );
    let bar_phys_loc: Point<f64, Physical> =
        Point::<i32, Logical>::from((bar_x_local, bar_y_local))
            .to_f64()
            .to_physical(scale);
    if let Ok(elem) = MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        bar_phys_loc,
        &bar_buf,
        None,
        None,
        Some((bar_w, bar_h).into()),
        Kind::Unspecified,
    ) {
        elements.push(OkayRenderElements::Memory(elem));
    }

    // 5. Render Expanded Start Menu Popup
    if state.panel.launcher_open && is_primary {
        let menu_h = MENU_HEADER_HEIGHT
            + MENU_PADDING * 2
            + LauncherItem::ALL.len() as i32 * MENU_ITEM_HEIGHT;
        let menu_x = bar_x_local + 8;
        let menu_y = (bar_y_local - menu_h - 8).max(8);

        let mut menu_canvas = Canvas::new(MENU_WIDTH, menu_h);
        menu_canvas.fill_rounded_rect(0, 0, MENU_WIDTH, menu_h, 10, [20, 20, 32, 250]);
        menu_canvas.stroke_rect(0, 0, MENU_WIDTH, menu_h, [accent[0], accent[1], accent[2], 190]);

        // Header
        menu_canvas.fill_rect(2, 2, MENU_WIDTH - 4, MENU_HEADER_HEIGHT - 2, [28, 28, 44, 255]);
        menu_canvas.draw_text_px(14, 10, "OKay DE", 15.5, [accent[0], accent[1], accent[2], 255]);
        menu_canvas.draw_text_px(96, 12, "Application Launcher", 12.5, [160, 165, 190, 255]);
        menu_canvas.fill_rect(8, MENU_HEADER_HEIGHT - 1, MENU_WIDTH - 16, 1, [50, 50, 72, 220]);

        for (idx, item) in LauncherItem::ALL.iter().enumerate() {
            let iy = MENU_HEADER_HEIGHT + MENU_PADDING + idx as i32 * MENU_ITEM_HEIGHT;
            let item_global = Rectangle::new(
                (
                    output_geo.loc.x + menu_x + MENU_PADDING,
                    output_geo.loc.y + menu_y + iy + 2,
                )
                    .into(),
                (MENU_WIDTH - MENU_PADDING * 2, MENU_ITEM_HEIGHT - 4).into(),
            );
            let hovered = rect_contains(item_global, pointer_x, pointer_y);
            let is_exit = *item == LauncherItem::ExitSession;

            let item_bg = if hovered {
                if is_exit {
                    [92, 38, 56, 255]
                } else {
                    [54, 50, 78, 255]
                }
            } else if is_exit {
                [46, 26, 38, 235]
            } else {
                [30, 30, 46, 235]
            };
            let item_fg = if is_exit {
                [245, 148, 174, 255]
            } else {
                [238, 240, 252, 255]
            };

            menu_canvas.fill_rounded_rect(
                MENU_PADDING,
                iy + 2,
                MENU_WIDTH - MENU_PADDING * 2,
                MENU_ITEM_HEIGHT - 4,
                6,
                item_bg,
            );

            // Icon badge
            let badge_bg = if is_exit {
                [120, 44, 66, 255]
            } else {
                [44, 44, 68, 255]
            };
            menu_canvas.fill_rounded_rect(MENU_PADDING + 6, iy + 6, 28, MENU_ITEM_HEIGHT - 12, 5, badge_bg);
            menu_canvas.draw_text_px(
                MENU_PADDING + 11,
                iy + 9,
                item.badge(),
                12.0,
                [accent[0], accent[1], accent[2], 255],
            );

            menu_canvas.draw_text_px(MENU_PADDING + 42, iy + 9, item.label(), 14.0, item_fg);
            menu_canvas.draw_text_px(
                MENU_WIDTH - MENU_PADDING - 72,
                iy + 10,
                item.hint(),
                11.5,
                [145, 150, 175, 255],
            );

            state.panel.hit_regions.push((item_global, ClickTarget::LaunchItem(*item)));
        }

        let menu_buf = MemoryRenderBuffer::from_slice(
            &menu_canvas.pixels,
            Fourcc::Abgr8888,
            (MENU_WIDTH, menu_h),
            1,
            Transform::Normal,
            None,
        );
        let menu_phys_loc: Point<f64, Physical> =
            Point::<i32, Logical>::from((menu_x, menu_y))
                .to_f64()
                .to_physical(scale);
        if let Ok(elem) = MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            menu_phys_loc,
            &menu_buf,
            None,
            None,
            Some((MENU_WIDTH, menu_h).into()),
            Kind::Unspecified,
        ) {
            elements.push(OkayRenderElements::Memory(elem));
        }
    }

    elements
}

/// Renders a server-side titlebar (with smooth Maple Mono title text and `[-]`, `[+]`, `[X]` buttons)
/// directly above `window` at output-local coordinates.
pub fn render_titlebar_element(
    window: &Window,
    local_win_loc: Point<i32, Logical>,
    is_focused: bool,
    renderer: &mut GlesRenderer,
    scale: Scale<f64>,
) -> Option<OkayRenderElements<GlesRenderer>> {
    let win_geo = window.geometry();
    let tb_w = win_geo.size.w.max(120);
    let tb_h = crate::state::TITLEBAR_HEIGHT;
    let tb_x = local_win_loc.x + win_geo.loc.x;
    let tb_y = local_win_loc.y + win_geo.loc.y - tb_h;

    let mut canvas = Canvas::new(tb_w, tb_h);
    let bg = if is_focused {
        [32, 28, 48, 250]
    } else {
        [22, 22, 34, 240]
    };
    let top_accent = if is_focused {
        [201, 160, 220, 255]
    } else {
        [56, 56, 78, 255]
    };
    canvas.fill_rect(0, 0, tb_w, tb_h, bg);
    canvas.fill_rect(0, 0, tb_w, 2, top_accent);

    let title = window_title(window);
    let max_chars = ((tb_w - 96) / 8).max(1) as usize;
    let short_title: String = title.chars().take(max_chars).collect();
    let title_fg = if is_focused {
        [238, 240, 252, 255]
    } else {
        [165, 168, 190, 255]
    };
    canvas.draw_text_px(10, 5, &short_title, 13.0, title_fg);

    if tb_w >= 120 {
        // Minimize button [-]: [tb_w - 80 .. tb_w - 58]
        canvas.fill_rounded_rect(tb_w - 80, 4, 22, tb_h - 8, 4, [48, 48, 68, 255]);
        canvas.draw_text_px(tb_w - 72, 4, "-", 13.0, [225, 228, 245, 255]);

        // Maximize button [+]: [tb_w - 54 .. tb_w - 32]
        canvas.fill_rounded_rect(tb_w - 54, 4, 22, tb_h - 8, 4, [48, 48, 68, 255]);
        canvas.draw_text_px(tb_w - 46, 4, "+", 13.0, [225, 228, 245, 255]);

        // Close button [X]: [tb_w - 28 .. tb_w - 6]
        canvas.fill_rounded_rect(tb_w - 28, 4, 22, tb_h - 8, 4, [190, 66, 92, 255]);
        canvas.draw_text_px(tb_w - 21, 4, "x", 13.0, [255, 245, 248, 255]);
    }

    let buf = MemoryRenderBuffer::from_slice(
        &canvas.pixels,
        Fourcc::Abgr8888,
        (tb_w, tb_h),
        1,
        Transform::Normal,
        None,
    );
    let phys_loc: Point<f64, Physical> = Point::<i32, Logical>::from((tb_x, tb_y))
        .to_f64()
        .to_physical(scale);

    MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        phys_loc,
        &buf,
        None,
        None,
        Some((tb_w, tb_h).into()),
        Kind::Unspecified,
    )
    .ok()
    .map(OkayRenderElements::Memory)
}

pub fn rect_contains(rect: Rectangle<i32, Logical>, px: i32, py: i32) -> bool {
    px >= rect.loc.x
        && px < rect.loc.x + rect.size.w
        && py >= rect.loc.y
        && py < rect.loc.y + rect.size.h
}

fn window_title(window: &Window) -> String {
    if let Some(toplevel) = window.toplevel() {
        let title = with_states(toplevel.wl_surface(), |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .and_then(|d| {
                    let guard = d.lock().unwrap();
                    guard.title.clone().or_else(|| guard.app_id.clone())
                })
        });
        if let Some(t) = title {
            if !t.is_empty() {
                return t;
            }
        }
    }
    "Window".to_string()
}

fn current_hh_mm() -> String {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    }
}

// ============================================================================
// TrueType Font Engine (`fontdue`: Maple Mono -> JetBrainsMono -> NotoSans)
// ============================================================================

struct FontEngine {
    font: Option<Font>,
    font_name: String,
    cache: HashMap<(char, u16), (Metrics, Vec<u8>)>,
}

static FONT_ENGINE: OnceLock<Mutex<FontEngine>> = OnceLock::new();

fn with_font_engine<R>(f: impl FnOnce(&mut FontEngine) -> R) -> R {
    let lock = FONT_ENGINE.get_or_init(|| Mutex::new(FontEngine::load()));
    let mut guard = lock.lock().unwrap();
    f(&mut guard)
}

/// Returns the human-readable name of the active TrueType font.
pub fn active_font_name() -> String {
    with_font_engine(|eng| eng.font_name.clone())
}

impl FontEngine {
    fn load() -> Self {
        let mut dirs = Vec::new();
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(&home).join(".local/share/fonts"));
            dirs.push(PathBuf::from(&home).join(".fonts"));
        }
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        dirs.push(PathBuf::from("/usr/share/fonts"));

        let mut all_fonts = Vec::new();
        for dir in &dirs {
            collect_font_files(dir, 0, &mut all_fonts);
        }

        // Priority tiers:
        // 1. Maple Mono (Regular preferred)
        // 2. JetBrains Mono (Regular preferred)
        // 3. Noto Sans / DejaVu Sans / Liberation Sans
        let tiers: [(&str, fn(&str) -> bool); 4] = [
            ("Maple Mono", |name| {
                name.contains("maplemono") || (name.contains("maple") && name.contains("mono"))
            }),
            ("JetBrains Mono", |name| name.contains("jetbrainsmono") || name.contains("jetbrains")),
            ("Noto Sans", |name| {
                name.contains("notosans-")
                    || name.contains("notosansregular")
                    || name.contains("dejavusans")
                    || name.contains("liberationsans")
            }),
            ("System TrueType", |name| name.ends_with(".ttf")),
        ];

        for (label, matcher) in tiers {
            let mut candidates: Vec<&PathBuf> = all_fonts
                .iter()
                .filter(|p| {
                    p.file_name()
                        .and_then(|s| s.to_str())
                        .map(|s| matcher(&s.to_lowercase()))
                        .unwrap_or(false)
                })
                .collect();

            // Prefer Regular over Italic/Bold variants
            candidates.sort_by_key(|p| {
                let s = p
                    .file_name()
                    .and_then(|x| x.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if s.contains("regular") {
                    0
                } else if !s.contains("italic") && !s.contains("bold") && !s.contains("light") {
                    1
                } else {
                    2
                }
            });

            for path in candidates {
                if let Ok(bytes) = fs::read(path) {
                    if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                        let file_label = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or(label)
                            .to_string();
                        tracing::info!(?path, font = %file_label, "Loaded TrueType UI font");
                        return Self {
                            font: Some(font),
                            font_name: file_label,
                            cache: HashMap::new(),
                        };
                    }
                }
            }
        }

        tracing::warn!("No system TrueType font found; using embedded fallback rasterizer");
        Self {
            font: None,
            font_name: "Built-in Sans".to_string(),
            cache: HashMap::new(),
        }
    }

    fn rasterize_cached(&mut self, ch: char, px_size: f32) -> Option<(Metrics, &[u8])> {
        let font = self.font.as_ref()?;
        let key = (ch, (px_size * 10.0).round() as u16);
        if !self.cache.contains_key(&key) {
            let (metrics, bitmap) = font.rasterize(ch, px_size);
            self.cache.insert(key, (metrics, bitmap));
        }
        self.cache.get(&key).map(|(m, b)| (*m, b.as_slice()))
    }
}

fn collect_font_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 5 || !dir.exists() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_font_files(&path, depth + 1, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_lowercase();
            if ext_lower == "ttf" || ext_lower == "otf" {
                out.push(path);
            }
        }
    }
}

// ============================================================================
// Software RGBA Canvas with Anti-Aliased Font & Rounded Rectangles
// ============================================================================

pub struct Canvas {
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<u8>,
}

impl Canvas {
    pub fn new(width: i32, height: i32) -> Self {
        let len = (width.max(1) as usize) * (height.max(1) as usize) * 4;
        Self {
            width,
            height,
            pixels: vec![0u8; len],
        }
    }

    #[inline]
    pub fn blend_pixel(&mut self, x: i32, y: i32, color: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let alpha = color[3] as u32;
        if alpha == 0 {
            return;
        }
        let idx = ((y * self.width + x) as usize) * 4;
        if alpha == 255 || self.pixels[idx + 3] == 0 {
            self.pixels[idx] = color[0];
            self.pixels[idx + 1] = color[1];
            self.pixels[idx + 2] = color[2];
            self.pixels[idx + 3] = color[3];
        } else {
            let inv = 255 - alpha;
            self.pixels[idx] = ((color[0] as u32 * alpha + self.pixels[idx] as u32 * inv) / 255) as u8;
            self.pixels[idx + 1] = ((color[1] as u32 * alpha + self.pixels[idx + 1] as u32 * inv) / 255) as u8;
            self.pixels[idx + 2] = ((color[2] as u32 * alpha + self.pixels[idx + 2] as u32 * inv) / 255) as u8;
            self.pixels[idx + 3] = (self.pixels[idx + 3] as u32 + ((255 - self.pixels[idx + 3] as u32) * alpha) / 255) as u8;
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: [u8; 4]) {
        let x0 = x.clamp(0, self.width);
        let y0 = y.clamp(0, self.height);
        let x1 = (x + w).clamp(0, self.width);
        let y1 = (y + h).clamp(0, self.height);

        for py in y0..y1 {
            for px in x0..x1 {
                let idx = ((py * self.width + px) as usize) * 4;
                self.pixels[idx] = color[0];
                self.pixels[idx + 1] = color[1];
                self.pixels[idx + 2] = color[2];
                self.pixels[idx + 3] = color[3];
            }
        }
    }

    pub fn fill_rounded_rect(&mut self, x: i32, y: i32, w: i32, h: i32, radius: i32, color: [u8; 4]) {
        let r = radius.clamp(0, (w.min(h)) / 2);
        if r <= 1 {
            self.fill_rect(x, y, w, h, color);
            return;
        }
        let x0 = x.clamp(0, self.width);
        let y0 = y.clamp(0, self.height);
        let x1 = (x + w).clamp(0, self.width);
        let y1 = (y + h).clamp(0, self.height);
        let r_sq = r * r;

        for py in y0..y1 {
            for px in x0..x1 {
                let lx = px - x;
                let ly = py - y;
                let cx = if lx < r {
                    r - 1 - lx
                } else if lx >= w - r {
                    lx - (w - r)
                } else {
                    0
                };
                let cy = if ly < r {
                    r - 1 - ly
                } else if ly >= h - r {
                    ly - (h - r)
                } else {
                    0
                };
                if cx > 0 && cy > 0 && (cx * cx + cy * cy) > r_sq {
                    continue;
                }
                let idx = ((py * self.width + px) as usize) * 4;
                self.pixels[idx] = color[0];
                self.pixels[idx + 1] = color[1];
                self.pixels[idx + 2] = color[2];
                self.pixels[idx + 3] = color[3];
            }
        }
    }

    pub fn stroke_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: [u8; 4]) {
        self.fill_rect(x, y, w, 1, color);
        self.fill_rect(x, y + h - 1, w, 1, color);
        self.fill_rect(x, y, 1, h, color);
        self.fill_rect(x + w - 1, y, 1, h, color);
    }

    /// Renders smooth anti-aliased TrueType text using `fontdue` (with cached glyphs).
    pub fn draw_text_px(&mut self, start_x: i32, start_y: i32, text: &str, px_size: f32, color: [u8; 4]) {
        let baseline_y = start_y + (px_size * 0.85).round() as i32;
        let mut cursor_x = start_x as f32;

        with_font_engine(|engine| {
            if engine.font.is_some() {
                for ch in text.chars() {
                    if let Some((metrics, bitmap)) = engine.rasterize_cached(ch, px_size) {
                        let gx = cursor_x.round() as i32 + metrics.xmin;
                        let gy = baseline_y - metrics.height as i32 - metrics.ymin;

                        for row in 0..metrics.height {
                            for col in 0..metrics.width {
                                let cov = bitmap[row * metrics.width + col];
                                if cov > 8 {
                                    let a = ((color[3] as u32 * cov as u32) / 255) as u8;
                                    self.blend_pixel(
                                        gx + col as i32,
                                        gy + row as i32,
                                        [color[0], color[1], color[2], a],
                                    );
                                }
                            }
                        }
                        cursor_x += metrics.advance_width;
                    }
                }
            }
        });
    }
}
