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
    reexports::wayland_server::{backend::ObjectId, Resource},
    utils::{Logical, Physical, Point, Rectangle, Scale, Transform},
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};

use crate::{animation::MinimizeEffect, render::OkayRenderElements, state::OkayState};

pub const PANEL_HEIGHT: i32 = 42;
const MENU_WIDTH: i32 = 320;
const MENU_HEADER_HEIGHT: i32 = 74;
const MENU_ITEM_HEIGHT: i32 = 34;
const MENU_FOOTER_HEIGHT: i32 = 42;
const MENU_PADDING: i32 = 8;

/// Accent color presets: `(Name, [R, G, B], linear [f32; 4])`.
pub const ACCENT_PRESETS: [(&str, [u8; 3], [f32; 4]); 6] = [
    ("Lavender", [201, 160, 220], [0.788, 0.627, 0.863, 1.0]),
    ("Sky Blue", [137, 180, 250], [0.537, 0.706, 0.980, 1.0]),
    ("Emerald", [166, 227, 161], [0.651, 0.890, 0.631, 1.0]),
    ("Coral", [243, 139, 168], [0.953, 0.545, 0.659, 1.0]),
    ("Amber", [249, 226, 175], [0.976, 0.886, 0.686, 1.0]),
    ("Crimson", [235, 111, 146], [0.922, 0.435, 0.573, 1.0]),
];

#[derive(Debug, Clone)]
pub struct DesktopAppEntry {
    pub name: String,
    pub exec: String,
    pub category: String,
    pub badge: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherItem {
    Terminal,
    FileManager,
    Browser,
    SystemMonitor,
    Screenshot,
    OkaySettings,
    ReloadConfig,
}

impl LauncherItem {
    const BUILTIN: [Self; 7] = [
        Self::Terminal,
        Self::FileManager,
        Self::Browser,
        Self::SystemMonitor,
        Self::Screenshot,
        Self::OkaySettings,
        Self::ReloadConfig,
    ];

    fn badge(self) -> &'static str {
        match self {
            Self::Terminal => ">_",
            Self::FileManager => "FM",
            Self::Browser => "WB",
            Self::SystemMonitor => "SM",
            Self::Screenshot => "SC",
            Self::OkaySettings => "OS",
            Self::ReloadConfig => "RC",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::FileManager => "File Manager",
            Self::Browser => "Web Browser",
            Self::SystemMonitor => "System Monitor",
            Self::Screenshot => "Take Screenshot",
            Self::OkaySettings => "OKay Settings",
            Self::ReloadConfig => "Reload Config",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Terminal => "Super+Ret",
            Self::FileManager => "Files",
            Self::Browser => "Internet",
            Self::SystemMonitor => "Resources",
            Self::Screenshot => "Super+S",
            Self::OkaySettings => "Super+I",
            Self::ReloadConfig => "Config",
        }
    }
}

#[derive(Debug, Clone)]
enum ClickTarget {
    ToggleLauncher,
    ToggleSettings,
    ToggleCalendar,
    ToggleNotifDrawer,
    ToggleMute,
    ToggleTiling,
    PinnedApp(LauncherItem),
    Workspace(usize),
    FocusWindow(usize),
    LaunchItem(LauncherItem),
    LaunchDesktopApp(String),
    PowerLock,
    PowerSuspend,
    PowerReboot,
    PowerExit,
}

#[derive(Debug)]
pub struct PanelState {
    pub launcher_open: bool,
    pub show_workspaces: bool,
    pub dock_mode: bool,
    pub panel_top: bool,
    pub accent_preset: usize,
    pub window_gaps: i32,
    pub search_query: String,
    pub desktop_apps: Vec<DesktopAppEntry>,
    pub default_terminal: String,
    pub default_browser: String,
    pub default_file_manager: String,
    pub volume_percent: i32,
    pub volume_muted: bool,
    pub battery_percent: Option<i32>,
    hit_regions: Vec<(Rectangle<i32, Logical>, ClickTarget)>,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            launcher_open: false,
            show_workspaces: true,
            dock_mode: false,
            panel_top: false,
            accent_preset: 0,
            window_gaps: 8,
            search_query: String::new(),
            desktop_apps: scan_desktop_applications(),
            default_terminal: "foot".to_string(),
            default_browser: "firefox".to_string(),
            default_file_manager: "thunar".to_string(),
            volume_percent: 75,
            volume_muted: false,
            battery_percent: read_battery_percent(),
            hit_regions: Vec::new(),
        }
    }
}

/// Extra initial settings loaded from `~/.config/okay-de/config.toml`.
pub struct LoadedUserConfig {
    pub pointer_speed: f64,
    pub natural_scroll: bool,
    pub anim_enabled: bool,
    pub anim_speed: f32,
    pub minimize_effect: MinimizeEffect,
    pub xkb_layout: String,
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

    /// Reads `~/.config/okay-de/config.toml` and returns user input & animation settings.
    pub fn load_config_file(&mut self) -> LoadedUserConfig {
        let mut cfg = LoadedUserConfig {
            pointer_speed: 1.0,
            natural_scroll: false,
            anim_enabled: true,
            anim_speed: 1.0,
            minimize_effect: MinimizeEffect::MagicLamp,
            xkb_layout: "us".to_string(),
        };
        let Some(path) = config_file_path() else {
            return cfg;
        };
        let Ok(content) = fs::read_to_string(path) else {
            return cfg;
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
                    "panel_top" => self.panel_top = val == "true",
                    "show_workspaces" => self.show_workspaces = val != "false",
                    "accent_preset" => {
                        if let Ok(idx) = val.parse::<usize>() {
                            self.accent_preset = idx % ACCENT_PRESETS.len();
                        }
                    }
                    "window_gaps" => {
                        if let Ok(g) = val.parse::<i32>() {
                            self.window_gaps = g.clamp(0, 32);
                        }
                    }
                    "pointer_speed" => {
                        if let Ok(spd) = val.parse::<f64>() {
                            cfg.pointer_speed = spd.clamp(0.25, 2.5);
                        }
                    }
                    "natural_scroll" => cfg.natural_scroll = val == "true",
                    "animations_enabled" => cfg.anim_enabled = val != "false",
                    "animation_speed" => {
                        if let Ok(s) = val.parse::<f32>() {
                            cfg.anim_speed = s.clamp(0.4, 2.5);
                        }
                    }
                    "minimize_effect" => cfg.minimize_effect = MinimizeEffect::from_str(val),
                    "default_terminal" => {
                        if !val.is_empty() {
                            self.default_terminal = val.to_string();
                        }
                    }
                    "default_browser" => {
                        if !val.is_empty() {
                            self.default_browser = val.to_string();
                        }
                    }
                    "default_file_manager" => {
                        if !val.is_empty() {
                            self.default_file_manager = val.to_string();
                        }
                    }
                    "xkb_layout" => {
                        if !val.is_empty() {
                            cfg.xkb_layout = val.to_string();
                        }
                    }
                    _ => {}
                }
            }
        }
        cfg
    }

    /// Saves all user preferences to `~/.config/okay-de/config.toml`.
    pub fn save_full_config(
        &self,
        pointer_speed: f64,
        natural_scroll: bool,
        anim_enabled: bool,
        anim_speed: f32,
        minimize_effect: MinimizeEffect,
        xkb_layout: &str,
    ) {
        let Some(path) = config_file_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let body = format!(
            "# OKay DE Configuration\ndock_mode = {}\npanel_top = {}\nshow_workspaces = {}\naccent_preset = {}\nwindow_gaps = {}\npointer_speed = {:.2}\nnatural_scroll = {}\nanimations_enabled = {}\nanimation_speed = {:.2}\nminimize_effect = \"{}\"\ndefault_terminal = \"{}\"\ndefault_browser = \"{}\"\ndefault_file_manager = \"{}\"\nxkb_layout = \"{}\"\n",
            self.dock_mode,
            self.panel_top,
            self.show_workspaces,
            self.accent_preset,
            self.window_gaps,
            pointer_speed,
            natural_scroll,
            anim_enabled,
            anim_speed,
            minimize_effect.as_config_str(),
            self.default_terminal,
            self.default_browser,
            self.default_file_manager,
            xkb_layout,
        );
        let _ = fs::write(path, body);
    }

    pub fn set_volume(&mut self, new_vol: i32) {
        self.volume_percent = new_vol.clamp(0, 100);
        let pct_arg = format!("{}%", self.volume_percent);
        if Command::new("wpctl")
            .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &pct_arg])
            .spawn()
            .is_err()
        {
            let _ = Command::new("pactl")
                .args(["set-sink-volume", "@DEFAULT_SINK@", &pct_arg])
                .spawn();
        }
    }

    pub fn toggle_mute(&mut self) {
        self.volume_muted = !self.volume_muted;
        if Command::new("wpctl")
            .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
            .spawn()
            .is_err()
        {
            let _ = Command::new("pactl")
                .args(["set-sink-mute", "@DEFAULT_SINK@", "toggle"])
                .spawn();
        }
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

/// Scans `/usr/share/applications` and `~/.local/share/applications` for `.desktop` files.
pub fn scan_desktop_applications() -> Vec<DesktopAppEntry> {
    let mut dirs = vec![PathBuf::from("/usr/share/applications")];
    if let Ok(home) = std::env::var("HOME") {
        dirs.insert(0, PathBuf::from(home).join(".local/share/applications"));
    }

    let mut apps = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let mut name = None;
            let mut exec = None;
            let mut categories = String::new();
            let mut no_display = false;

            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('[') && line != "[Desktop Entry]" {
                    break;
                }
                if line == "NoDisplay=true" || line == "Hidden=true" {
                    no_display = true;
                } else if name.is_none() && line.starts_with("Name=") {
                    name = line.strip_prefix("Name=").map(|s| s.trim().to_string());
                } else if exec.is_none() && line.starts_with("Exec=") {
                    if let Some(raw_exec) = line.strip_prefix("Exec=") {
                        // Strip `%u`, `%F`, etc. field codes
                        let cleaned: Vec<&str> = raw_exec
                            .split_whitespace()
                            .filter(|tok| !tok.starts_with('%'))
                            .collect();
                        if !cleaned.is_empty() {
                            exec = Some(cleaned.join(" "));
                        }
                    }
                } else if line.starts_with("Categories=") {
                    categories = line.strip_prefix("Categories=").unwrap_or("").to_string();
                }
            }

            if no_display {
                continue;
            }
            if let (Some(n), Some(e)) = (name, exec) {
                if apps.iter().any(|a: &DesktopAppEntry| a.name == n) {
                    continue;
                }
                let badge: String = n
                    .chars()
                    .filter(|c| c.is_alphanumeric())
                    .take(2)
                    .collect::<String>()
                    .to_uppercase();
                let category = if categories.contains("Development") {
                    "Dev".to_string()
                } else if categories.contains("Network") {
                    "Web".to_string()
                } else if categories.contains("AudioVideo") {
                    "Media".to_string()
                } else if categories.contains("System") {
                    "System".to_string()
                } else {
                    "App".to_string()
                };
                apps.push(DesktopAppEntry {
                    name: n,
                    exec: e,
                    category,
                    badge: if badge.is_empty() { "AP".to_string() } else { badge },
                });
            }
        }
    }
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps
}

/// Spawns the user's preferred terminal or the first available terminal emulator installed on the system.
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

pub fn spawn_first_available(candidates: &[&str], socket_name: &OsStr, kind: &str) {
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

pub fn execute_launcher_item(state: &mut OkayState, item: LauncherItem) {
    state.panel.launcher_open = false;
    state.panel.search_query.clear();
    match item {
        LauncherItem::Terminal => {
            let preferred = state.panel.default_terminal.clone();
            let list = [
                preferred.as_str(),
                "foot",
                "kitty",
                "alacritty",
                "wezterm",
                "konsole",
                "gnome-terminal",
                "xterm",
            ];
            spawn_first_available(&list, &state.socket_name, "terminal");
        }
        LauncherItem::FileManager => {
            let preferred = state.panel.default_file_manager.clone();
            let list = [
                preferred.as_str(),
                "thunar",
                "pcmanfm",
                "nemo",
                "dolphin",
                "nautilus",
            ];
            spawn_first_available(&list, &state.socket_name, "file-manager");
        }
        LauncherItem::Browser => {
            let preferred = state.panel.default_browser.clone();
            let list = [
                preferred.as_str(),
                "firefox",
                "chromium",
                "brave",
                "floorp",
                "xdg-open",
            ];
            spawn_first_available(&list, &state.socket_name, "browser");
        }
        LauncherItem::SystemMonitor => spawn_system_monitor(&state.socket_name),
        LauncherItem::Screenshot => {
            crate::overlays::take_screenshot(state);
        }
        LauncherItem::OkaySettings => {
            state.settings.open = true;
            state.settings.refresh_system_info();
        }
        LauncherItem::ReloadConfig => {
            let cfg = state.panel.load_config_file();
            state.pointer_sensitivity = cfg.pointer_speed;
            state.natural_scroll = cfg.natural_scroll;
            state.animations.enabled = cfg.anim_enabled;
            state.animations.speed = cfg.anim_speed;
            state.animations.minimize_effect = cfg.minimize_effect;
            state.theme.active_border_color = state.panel.accent_f32();
            state.desktop.reload();
            state.retile_workspace();
            state
                .overlays
                .push_notification("Configuration Reloaded", "Applied ~/.config/okay-de/config.toml");
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
        if rect_contains(*rect, px, py) {
            matched = Some(target.clone());
            break;
        }
    }

    if let Some(target) = matched {
        match target {
            ClickTarget::ToggleLauncher => {
                state.panel.launcher_open = !state.panel.launcher_open;
                if state.panel.launcher_open {
                    state.panel.search_query.clear();
                }
            }
            ClickTarget::ToggleSettings => {
                state.panel.launcher_open = false;
                state.settings.open = !state.settings.open;
                if state.settings.open {
                    state.settings.refresh_system_info();
                }
            }
            ClickTarget::ToggleCalendar => {
                state.panel.launcher_open = false;
                state.overlays.calendar_open = !state.overlays.calendar_open;
                state.overlays.notif_drawer_open = false;
            }
            ClickTarget::ToggleNotifDrawer => {
                state.panel.launcher_open = false;
                state.overlays.notif_drawer_open = !state.overlays.notif_drawer_open;
                state.overlays.calendar_open = false;
            }
            ClickTarget::ToggleMute => {
                state.panel.toggle_mute();
            }
            ClickTarget::ToggleTiling => {
                state.panel.launcher_open = false;
                state.toggle_tiling();
            }
            ClickTarget::PinnedApp(item) => {
                execute_launcher_item(state, item);
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
            ClickTarget::LaunchDesktopApp(cmd) => {
                state.panel.launcher_open = false;
                state.panel.search_query.clear();
                let _ = Command::new("sh")
                    .arg("-c")
                    .arg(&cmd)
                    .env("WAYLAND_DISPLAY", &state.socket_name)
                    .spawn();
            }
            ClickTarget::PowerLock => {
                state.panel.launcher_open = false;
                state.overlays.locked = true;
                state.overlays.lock_input.clear();
            }
            ClickTarget::PowerSuspend => {
                state.panel.launcher_open = false;
                let _ = Command::new("systemctl").arg("suspend").spawn();
            }
            ClickTarget::PowerReboot => {
                state.panel.launcher_open = false;
                let _ = Command::new("systemctl").arg("reboot").spawn();
            }
            ClickTarget::PowerExit => {
                state.running = false;
                state.loop_signal.stop();
            }
        }
        return true;
    }

    if state.panel.launcher_open {
        state.panel.launcher_open = false;
        state.panel.search_query.clear();
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

    let ws_windows: Vec<(Option<ObjectId>, String, bool, bool)> = state
        .space
        .elements()
        .filter(|w| state.workspaces.is_on_active_workspace(w))
        .map(|w| {
            let id = w.toplevel().map(|t| t.wl_surface().id());
            let is_min = state.workspaces.is_minimized(w);
            let is_focused = !is_min && id.as_ref() == focused_id.as_ref();
            let title = window_title_str(w);
            (id, title, is_focused, is_min)
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

    let bar_y_local = if state.panel.panel_top {
        if state.panel.dock_mode { 8 } else { 0 }
    } else if state.panel.dock_mode {
        output_geo.size.h - PANEL_HEIGHT - 8
    } else {
        output_geo.size.h - PANEL_HEIGHT
    };

    let (bar_x_local, bar_w, bar_h) = if state.panel.dock_mode {
        let ws_section_w = if state.panel.show_workspaces { 9 * 27 + 10 } else { 0 };
        let win_section_w = (ws_windows.len() as i32 * 145).clamp(130, 560);
        let desired_w = (195 + ws_section_w + win_section_w + 260)
            .clamp(480, (output_geo.size.w - 32).max(360));
        let bx = (output_geo.size.w - desired_w) / 2;
        (bx, desired_w, PANEL_HEIGHT)
    } else {
        (0, output_geo.size.w, PANEL_HEIGHT)
    };

    let mut elements = Vec::new();
    let mut canvas = Canvas::new(bar_w, bar_h);

    if state.panel.dock_mode {
        canvas.fill_rounded_rect(0, 0, bar_w, bar_h, 11, [20, 20, 32, 238]);
        canvas.fill_rect(12, 0, bar_w - 24, 2, [accent[0], accent[1], accent[2], 185]);
    } else {
        canvas.fill_rect(0, 0, bar_w, bar_h, [18, 18, 28, 244]);
        let line_y = if state.panel.panel_top { bar_h - 1 } else { 0 };
        canvas.fill_rect(0, line_y, bar_w, 1, [accent[0], accent[1], accent[2], 145]);
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
    state
        .panel
        .hit_regions
        .push((start_global, ClickTarget::ToggleLauncher));
    cursor_x += start_w + 8;

    // 2. Quick Pinned App Launchers (`>_` Terminal, `FM` Files, `WB` Browser)
    let pinned = [
        (LauncherItem::Terminal, ">_"),
        (LauncherItem::FileManager, "FM"),
        (LauncherItem::Browser, "WB"),
    ];
    for (item, icon) in pinned {
        let pw = 30;
        let pin_global = Rectangle::new(
            (output_geo.loc.x + bar_x_local + cursor_x, output_geo.loc.y + bar_y_local + 6).into(),
            (pw, bar_h - 12).into(),
        );
        let hovered = rect_contains(pin_global, pointer_x, pointer_y);
        let bg = if hovered {
            [58, 54, 82, 255]
        } else {
            [32, 32, 48, 235]
        };
        let y_off = if hovered { 4 } else { 6 };
        canvas.fill_rounded_rect(cursor_x, y_off, pw, bar_h - 12, 6, bg);
        canvas.draw_text_px(
            cursor_x + 6,
            y_off + 6,
            icon,
            13.0,
            [accent[0], accent[1], accent[2], 255],
        );
        state
            .panel
            .hit_regions
            .push((pin_global, ClickTarget::PinnedApp(item)));
        cursor_x += pw + 4;
    }
    cursor_x += 6;

    // 3. Workspace Indicators (1..9) if enabled
    if state.panel.show_workspaces {
        for ws in 1..=9 {
            let ws_w = 23;
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
            canvas.draw_text_px(cursor_x + 7, 12, &label, 13.5, fg);
            state
                .panel
                .hit_regions
                .push((ws_global, ClickTarget::Workspace(ws)));
            cursor_x += ws_w + 4;
        }
        cursor_x += 8;
    }

    // 4. Right-Side System Tray: Clock/Calendar + Notifications + Settings + Audio Volume + Battery + Layout
    let clock_str = current_hh_mm();
    let clock_w = 68;
    let clock_x = (bar_w - clock_w - 10).max(cursor_x);
    let clock_global = Rectangle::new(
        (output_geo.loc.x + bar_x_local + clock_x, output_geo.loc.y + bar_y_local + 6).into(),
        (clock_w, bar_h - 12).into(),
    );
    let clock_bg = if state.overlays.calendar_open {
        [accent[0], accent[1], accent[2], 255]
    } else if rect_contains(clock_global, pointer_x, pointer_y) {
        [50, 50, 74, 255]
    } else {
        [32, 32, 48, 235]
    };
    let clock_fg = if state.overlays.calendar_open {
        [16, 16, 24, 255]
    } else {
        [235, 238, 250, 255]
    };
    canvas.fill_rounded_rect(clock_x, 6, clock_w, bar_h - 12, 6, clock_bg);
    canvas.draw_text_px(clock_x + 12, 12, &clock_str, 14.0, clock_fg);
    state
        .panel
        .hit_regions
        .push((clock_global, ClickTarget::ToggleCalendar));

    // Notification Center Bell [N]
    let notif_w = 32;
    let notif_x = (clock_x - notif_w - 6).max(cursor_x);
    if notif_x + notif_w <= clock_x - 4 {
        let notif_global = Rectangle::new(
            (output_geo.loc.x + bar_x_local + notif_x, output_geo.loc.y + bar_y_local + 6).into(),
            (notif_w, bar_h - 12).into(),
        );
        let n_bg = if state.overlays.notif_drawer_open {
            [accent[0], accent[1], accent[2], 255]
        } else {
            [34, 34, 50, 235]
        };
        let n_fg = if state.overlays.notif_drawer_open {
            [16, 16, 24, 255]
        } else {
            [225, 228, 245, 255]
        };
        canvas.fill_rounded_rect(notif_x, 6, notif_w, bar_h - 12, 6, n_bg);
        canvas.draw_text_px(notif_x + 10, 12, "N", 13.0, n_fg);
        state
            .panel
            .hit_regions
            .push((notif_global, ClickTarget::ToggleNotifDrawer));
    }

    // Settings [OS]
    let settings_w = 34;
    let settings_x = (notif_x - settings_w - 6).max(cursor_x);
    if settings_x + settings_w <= notif_x - 4 {
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
        state
            .panel
            .hit_regions
            .push((set_global, ClickTarget::ToggleSettings));
    }

    // Volume indicator
    let vol_w = 54;
    let vol_x = (settings_x - vol_w - 6).max(cursor_x);
    if vol_x + vol_w <= settings_x - 4 {
        let vol_global = Rectangle::new(
            (output_geo.loc.x + bar_x_local + vol_x, output_geo.loc.y + bar_y_local + 6).into(),
            (vol_w, bar_h - 12).into(),
        );
        let vol_txt = if state.panel.volume_muted {
            "MUTE".to_string()
        } else if let Some(bat) = state.panel.battery_percent {
            format!("{}% B{}", state.panel.volume_percent, bat)
        } else {
            format!("V:{}%", state.panel.volume_percent)
        };
        canvas.fill_rounded_rect(vol_x, 6, vol_w, bar_h - 12, 6, [34, 34, 50, 235]);
        canvas.draw_text_px(vol_x + 7, 12, &vol_txt, 12.5, [215, 220, 240, 255]);
        state
            .panel
            .hit_regions
            .push((vol_global, ClickTarget::ToggleMute));
    }

    // Layout mode badge (`TILE` / `DWND` / `FLT`)
    let mode_label = state.workspaces.layout_mode.short_label();
    let status_w = 52;
    let status_x = (vol_x - status_w - 6).max(cursor_x);
    if status_x + status_w <= vol_x - 4 {
        let status_global = Rectangle::new(
            (output_geo.loc.x + bar_x_local + status_x, output_geo.loc.y + bar_y_local + 6).into(),
            (status_w, bar_h - 12).into(),
        );
        let status_bg = if state.workspaces.tiling_enabled() {
            [56, 46, 78, 245]
        } else {
            [34, 34, 50, 235]
        };
        canvas.fill_rounded_rect(status_x, 6, status_w, bar_h - 12, 6, status_bg);
        canvas.draw_text_px(
            status_x + 9,
            12,
            mode_label,
            12.5,
            [accent[0], accent[1], accent[2], 255],
        );
        state
            .panel
            .hit_regions
            .push((status_global, ClickTarget::ToggleTiling));
    }

    // 5. Open Window Task Buttons (with exact button rect registration for Magic Lamp Genie!)
    let avail_w = (status_x - cursor_x - 8).max(0);
    if !ws_windows.is_empty() && avail_w > 48 {
        let max_btn_w = 170;
        let btn_w = ((avail_w / ws_windows.len() as i32) - 6).clamp(52, max_btn_w);
        for (idx, (win_id, title, is_focused, is_min)) in ws_windows.iter().enumerate() {
            let bx = cursor_x + idx as i32 * (btn_w + 6);
            if bx + btn_w > status_x - 4 {
                break;
            }
            let win_global = Rectangle::new(
                (output_geo.loc.x + bar_x_local + bx, output_geo.loc.y + bar_y_local + 6).into(),
                (btn_w, bar_h - 12).into(),
            );
            if let Some(id) = win_id {
                state
                    .animations
                    .taskbar_button_rects
                    .insert(id.clone(), win_global);
            }

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
            let y_top = if hovered { 4 } else { 6 };
            canvas.fill_rounded_rect(bx, y_top, btn_w, bar_h - 12, 6, bg);
            if *is_focused {
                canvas.fill_rect(
                    bx + 6,
                    bar_h - 8,
                    btn_w - 12,
                    2,
                    [accent[0], accent[1], accent[2], 255],
                );
            }
            let max_chars = ((btn_w - 18) / 8).max(2) as usize;
            let short: String = title.chars().take(max_chars).collect();
            canvas.draw_text_px(bx + 10, y_top + 6, &short, 13.5, fg);
            state
                .panel
                .hit_regions
                .push((win_global, ClickTarget::FocusWindow(idx)));
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

    // 6. Render Expanded Searchable Start Menu Popup (with smooth slide animation)
    let menu_anim = state.animations.launcher_open_anim;
    if (state.panel.launcher_open || menu_anim > 0.02) && is_primary {
        let query_lower = state.panel.search_query.to_lowercase();
        let matching_desktop: Vec<DesktopAppEntry> = state
            .panel
            .desktop_apps
            .iter()
            .filter(|a| query_lower.is_empty() || a.name.to_lowercase().contains(&query_lower))
            .take(4)
            .cloned()
            .collect();

        let total_rows = LauncherItem::BUILTIN.len() + matching_desktop.len();
        let menu_h = MENU_HEADER_HEIGHT
            + MENU_PADDING * 2
            + total_rows as i32 * MENU_ITEM_HEIGHT
            + MENU_FOOTER_HEIGHT;
        let menu_x = bar_x_local + 8;
        let slide_px = ((1.0 - menu_anim) * 18.0).round() as i32;
        let menu_y = if state.panel.panel_top {
            bar_y_local + PANEL_HEIGHT + 8 - slide_px
        } else {
            (bar_y_local - menu_h - 8 + slide_px).max(8)
        };

        let mut menu_canvas = Canvas::new(MENU_WIDTH, menu_h);
        menu_canvas.fill_rounded_rect(0, 0, MENU_WIDTH, menu_h, 10, [20, 20, 32, 250]);
        menu_canvas.stroke_rect(0, 0, MENU_WIDTH, menu_h, [accent[0], accent[1], accent[2], 195]);

        // Header + Search Box
        menu_canvas.fill_rect(2, 2, MENU_WIDTH - 4, MENU_HEADER_HEIGHT - 2, [28, 28, 44, 255]);
        menu_canvas.draw_text_px(
            14,
            8,
            "OKay DE",
            15.5,
            [accent[0], accent[1], accent[2], 255],
        );
        menu_canvas.draw_text_px(
            96,
            10,
            "Application Launcher",
            12.5,
            [160, 165, 190, 255],
        );

        menu_canvas.fill_rounded_rect(12, 36, MENU_WIDTH - 24, 28, 6, [16, 16, 26, 255]);
        let search_txt = if state.panel.search_query.is_empty() {
            "Type to search applications...".to_string()
        } else {
            format!("Search: {}_", state.panel.search_query)
        };
        let search_fg = if state.panel.search_query.is_empty() {
            [130, 135, 160, 255]
        } else {
            [240, 242, 252, 255]
        };
        menu_canvas.draw_text_px(22, 42, &search_txt, 12.5, search_fg);

        let mut row_idx = 0;

        // Built-in items
        for item in LauncherItem::BUILTIN {
            if !query_lower.is_empty() && !item.label().to_lowercase().contains(&query_lower) {
                continue;
            }
            let iy = MENU_HEADER_HEIGHT + MENU_PADDING + row_idx * MENU_ITEM_HEIGHT;
            let item_global = Rectangle::new(
                (
                    output_geo.loc.x + menu_x + MENU_PADDING,
                    output_geo.loc.y + menu_y + iy + 2,
                )
                    .into(),
                (MENU_WIDTH - MENU_PADDING * 2, MENU_ITEM_HEIGHT - 4).into(),
            );
            let hovered = rect_contains(item_global, pointer_x, pointer_y);
            let item_bg = if hovered {
                [54, 50, 78, 255]
            } else {
                [30, 30, 46, 235]
            };

            menu_canvas.fill_rounded_rect(
                MENU_PADDING,
                iy + 2,
                MENU_WIDTH - MENU_PADDING * 2,
                MENU_ITEM_HEIGHT - 4,
                6,
                item_bg,
            );
            menu_canvas.fill_rounded_rect(
                MENU_PADDING + 6,
                iy + 5,
                26,
                MENU_ITEM_HEIGHT - 10,
                5,
                [44, 44, 68, 255],
            );
            menu_canvas.draw_text_px(
                MENU_PADDING + 10,
                iy + 8,
                item.badge(),
                11.5,
                [accent[0], accent[1], accent[2], 255],
            );
            menu_canvas.draw_text_px(
                MENU_PADDING + 40,
                iy + 8,
                item.label(),
                13.5,
                [238, 240, 252, 255],
            );
            menu_canvas.draw_text_px(
                MENU_WIDTH - MENU_PADDING - 72,
                iy + 9,
                item.hint(),
                11.5,
                [145, 150, 175, 255],
            );

            state
                .panel
                .hit_regions
                .push((item_global, ClickTarget::LaunchItem(item)));
            row_idx += 1;
        }

        // Scanned `.desktop` system applications
        for app in &matching_desktop {
            let iy = MENU_HEADER_HEIGHT + MENU_PADDING + row_idx * MENU_ITEM_HEIGHT;
            let item_global = Rectangle::new(
                (
                    output_geo.loc.x + menu_x + MENU_PADDING,
                    output_geo.loc.y + menu_y + iy + 2,
                )
                    .into(),
                (MENU_WIDTH - MENU_PADDING * 2, MENU_ITEM_HEIGHT - 4).into(),
            );
            let hovered = rect_contains(item_global, pointer_x, pointer_y);
            let item_bg = if hovered {
                [54, 50, 78, 255]
            } else {
                [28, 28, 42, 235]
            };

            menu_canvas.fill_rounded_rect(
                MENU_PADDING,
                iy + 2,
                MENU_WIDTH - MENU_PADDING * 2,
                MENU_ITEM_HEIGHT - 4,
                6,
                item_bg,
            );
            menu_canvas.fill_rounded_rect(
                MENU_PADDING + 6,
                iy + 5,
                26,
                MENU_ITEM_HEIGHT - 10,
                5,
                [40, 44, 66, 255],
            );
            menu_canvas.draw_text_px(
                MENU_PADDING + 10,
                iy + 8,
                &app.badge,
                11.5,
                [accent[0], accent[1], accent[2], 255],
            );
            let short_name: String = app.name.chars().take(24).collect();
            menu_canvas.draw_text_px(
                MENU_PADDING + 40,
                iy + 8,
                &short_name,
                13.5,
                [235, 238, 250, 255],
            );
            menu_canvas.draw_text_px(
                MENU_WIDTH - MENU_PADDING - 52,
                iy + 9,
                &app.category,
                11.5,
                [145, 150, 175, 255],
            );

            state.panel.hit_regions.push((
                item_global,
                ClickTarget::LaunchDesktopApp(app.exec.clone()),
            ));
            row_idx += 1;
        }

        // Power Footer Bar (`Lock`, `Suspend`, `Reboot`, `Exit`)
        let fy = menu_h - MENU_FOOTER_HEIGHT + 4;
        menu_canvas.fill_rect(8, fy - 4, MENU_WIDTH - 16, 1, [48, 48, 70, 220]);
        let power_btns: [(&str, ClickTarget, [u8; 4]); 4] = [
            ("Lock", ClickTarget::PowerLock, [40, 40, 60, 255]),
            ("Sleep", ClickTarget::PowerSuspend, [40, 40, 60, 255]),
            ("Reboot", ClickTarget::PowerReboot, [48, 40, 60, 255]),
            ("Exit", ClickTarget::PowerExit, [145, 48, 72, 255]),
        ];
        for (pi, (plabel, ptarget, pbg)) in power_btns.into_iter().enumerate() {
            let pw = 68;
            let px = 12 + pi as i32 * 75;
            let p_global = Rectangle::new(
                (output_geo.loc.x + menu_x + px, output_geo.loc.y + menu_y + fy).into(),
                (pw, 30).into(),
            );
            menu_canvas.fill_rounded_rect(px, fy, pw, 30, 6, pbg);
            menu_canvas.draw_text_px(px + 14, fy + 7, plabel, 12.5, [242, 244, 252, 255]);
            state.panel.hit_regions.push((p_global, ptarget));
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
    is_pinned: bool,
    accent: [u8; 3],
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
        [accent[0], accent[1], accent[2], 255]
    } else {
        [56, 56, 78, 255]
    };
    canvas.fill_rect(0, 0, tb_w, tb_h, bg);
    canvas.fill_rect(0, 0, tb_w, 2, top_accent);

    let mut title = window_title_str(window);
    if is_pinned {
        title = format!("[PIN] {}", title);
    }
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

pub fn window_title_str(window: &Window) -> String {
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

fn read_battery_percent() -> Option<i32> {
    let entries = fs::read_dir("/sys/class/power_supply").ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("BAT") {
            let cap = fs::read_to_string(entry.path().join("capacity")).ok()?;
            return cap.trim().parse::<i32>().ok();
        }
    }
    None
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
