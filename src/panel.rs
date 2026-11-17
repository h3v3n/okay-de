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

pub const PANEL_HEIGHT: i32 = 28;
pub const DOCK_HEIGHT: i32 = 62;
pub const DOCK_MARGIN: i32 = 10;
const MENU_WIDTH: i32 = 340;
const MENU_HEADER_HEIGHT: i32 = 76;
const MENU_ITEM_HEIGHT: i32 = 34;
const MENU_FOOTER_HEIGHT: i32 = 44;
const MENU_PADDING: i32 = 10;

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
    pub icon_name: Option<String>,
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
    ToggleControlCenter,
    ToggleMute,
    ToggleTiling,
    MenuFile,
    MenuWindow,
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
    pub custom_pinned: Vec<DesktopAppEntry>,
    pub default_terminal: String,
    pub default_browser: String,
    pub default_file_manager: String,
    pub volume_percent: i32,
    pub volume_muted: bool,
    pub battery_percent: Option<i32>,
    pub autostart_cmds: Vec<String>,
    hit_regions: Vec<(Rectangle<i32, Logical>, ClickTarget)>,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            launcher_open: false,
            show_workspaces: true,
            dock_mode: true,
            panel_top: true,
            accent_preset: 1,
            window_gaps: 10,
            search_query: String::new(),
            desktop_apps: scan_desktop_applications(),
            custom_pinned: Vec::new(),
            default_terminal: "foot".to_string(),
            default_browser: "firefox".to_string(),
            default_file_manager: "thunar".to_string(),
            volume_percent: 75,
            volume_muted: false,
            battery_percent: read_battery_percent(),
            autostart_cmds: Vec::new(),
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
        let mut in_autostart_section = false;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                in_autostart_section = line.eq_ignore_ascii_case("[autostart]");
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let key = k.trim();
                let val = v.trim().trim_matches('"');
                if in_autostart_section && (key == "exec" || key == "cmd" || key == "autostart") {
                    if !val.is_empty() && !self.autostart_cmds.iter().any(|c| c == val) {
                        self.autostart_cmds.push(val.to_string());
                    }
                    continue;
                }
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
                    "pinned_app" => {
                        let parts: Vec<&str> = val.split('|').collect();
                        if parts.len() >= 2 && !parts[0].is_empty() && !parts[1].is_empty() {
                            let icon_name = parts.get(2).filter(|s| !s.is_empty()).map(|s| (*s).to_string());
                            if !self.custom_pinned.iter().any(|p| p.exec == parts[1]) {
                                self.custom_pinned.push(DesktopAppEntry {
                                    name: parts[0].to_string(),
                                    exec: parts[1].to_string(),
                                    category: "Pinned".to_string(),
                                    icon_name,
                                });
                            }
                        }
                    }
                    "autostart" => {
                        if !val.is_empty() && !self.autostart_cmds.iter().any(|c| c == val) {
                            self.autostart_cmds.push(val.to_string());
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
        let mut body = format!(
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
        for p in &self.custom_pinned {
            body.push_str(&format!(
                "pinned_app = \"{}|{}|{}\"\n",
                p.name,
                p.exec,
                p.icon_name.as_deref().unwrap_or("")
            ));
        }
        for cmd in &self.autostart_cmds {
            body.push_str(&format!("autostart = \"{}\"\n", cmd));
        }
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
            let mut icon_name = None;
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
                } else if icon_name.is_none() && line.starts_with("Icon=") {
                    icon_name = line.strip_prefix("Icon=").map(|s| s.trim().to_string());
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
                    icon_name,
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
                state.overlays.control_center_open = false;
            }
            ClickTarget::ToggleControlCenter => {
                state.panel.launcher_open = false;
                state.overlays.control_center_open = !state.overlays.control_center_open;
                state.overlays.calendar_open = false;
                state.overlays.notif_drawer_open = false;
            }
            ClickTarget::MenuFile => {
                state.panel.launcher_open = false;
                execute_launcher_item(state, LauncherItem::Terminal);
            }
            ClickTarget::MenuWindow => {
                state.panel.launcher_open = false;
                state.cycle_focus();
            }
            ClickTarget::ToggleMute => {
                state.panel.toggle_mute();
            }
            ClickTarget::ToggleTiling => {
                state.panel.launcher_open = false;
                state.toggle_tiling();
            }
            ClickTarget::PinnedApp(item) => {
                let bounce_key = format!("pin_{}", item.badge());
                state.animations.trigger_dock_bounce(&bounce_key);
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
) -> Vec<OkayRenderElements> {
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

    let mut elements = Vec::new();
    let focused_app_name = ws_windows
        .iter()
        .find(|(_, _, f, _)| *f)
        .map(|(_, t, _, _)| {
            let short: String = t.chars().take(20).collect();
            short
        })
        .unwrap_or_else(|| "OKay Desktop".to_string());

    if state.panel.dock_mode {
        // ====================================================================
        // 1:1 macOS Sequoia / Sonoma Layout:
        //   A. Top Menu Bar (28px height, full width, translucent dark glass)
        //   B. Bottom Floating Dock (18px rounded glass pill + squircles + parabolic zoom)
        // ====================================================================
        let mb_w = output_geo.size.w;
        let mb_h = PANEL_HEIGHT;
        let mut mb = Canvas::new(mb_w, mb_h);

        // Translucent dark glass: rgba(24, 26, 36, 0.86) + subtle 1px bottom border
        mb.fill_rect(0, 0, mb_w, mb_h, [24, 26, 36, 219]);
        mb.fill_rect_blend(0, 0, mb_w, 1, [255, 255, 255, 26]);
        mb.fill_rect_blend(0, mb_h - 1, mb_w, 1, [255, 255, 255, 18]);

        let mut mx = 12;

        // Crisp `OKay` logo button (toggles Spotlight / Launcher)
        let logo_w = 50;
        let logo_global = Rectangle::new(
            (output_geo.loc.x + mx, output_geo.loc.y + 2).into(),
            (logo_w, mb_h - 4).into(),
        );
        if state.panel.launcher_open || rect_contains(logo_global, pointer_x, pointer_y) {
            mb.fill_rounded_rect_aa(mx, 3, logo_w, mb_h - 6, 6, [255, 255, 255, 32]);
        }
        mb.draw_text_px(mx + 8, 6, "OKay", 13.5, [246, 248, 255, 255]);
        state
            .panel
            .hit_regions
            .push((logo_global, ClickTarget::ToggleLauncher));
        mx += logo_w + 10;

        // Focused App Name (bold-style bright white)
        let app_w = (mb.measure_text_px(&focused_app_name, 13.5) + 12).clamp(64, 180);
        mb.draw_text_px(mx, 6, &focused_app_name, 13.5, [255, 255, 255, 255]);
        mx += app_w + 8;

        // Clean macOS Menu Items: File, Edit, View, Window, Help
        let menu_items: [(&str, Option<ClickTarget>); 5] = [
            ("File", Some(ClickTarget::MenuFile)),
            ("Edit", None),
            ("View", Some(ClickTarget::ToggleTiling)),
            ("Window", Some(ClickTarget::MenuWindow)),
            ("Help", Some(ClickTarget::ToggleSettings)),
        ];
        for (label, target) in menu_items {
            let iw = mb.measure_text_px(label, 12.5) + 14;
            if mx + iw > mb_w - 380 {
                break;
            }
            let item_rect = Rectangle::new(
                (output_geo.loc.x + mx, output_geo.loc.y + 2).into(),
                (iw, mb_h - 4).into(),
            );
            if rect_contains(item_rect, pointer_x, pointer_y) {
                mb.fill_rounded_rect_aa(mx, 3, iw, mb_h - 6, 5, [255, 255, 255, 26]);
            }
            mb.draw_text_px(mx + 7, 7, label, 12.5, [224, 228, 240, 245]);
            if let Some(t) = target {
                state.panel.hit_regions.push((item_rect, t));
            }
            mx += iw + 2;
        }

        // Optional compact Workspace pills in menu bar (1..9) if enabled
        if state.panel.show_workspaces && mx + 200 < mb_w - 340 {
            mx += 10;
            for ws in 1..=9 {
                let ww = 18;
                let is_active = state.workspaces.active == ws;
                let is_occ = occupied_workspaces[ws - 1];
                let ws_rect = Rectangle::new(
                    (output_geo.loc.x + mx, output_geo.loc.y + 4).into(),
                    (ww, mb_h - 8).into(),
                );
                if is_active {
                    mb.fill_rounded_rect_aa(mx, 5, ww, mb_h - 10, 5, [255, 255, 255, 42]);
                } else if rect_contains(ws_rect, pointer_x, pointer_y) {
                    mb.fill_rounded_rect_aa(mx, 5, ww, mb_h - 10, 5, [255, 255, 255, 20]);
                }
                let wfg = if is_active {
                    [255, 255, 255, 255]
                } else if is_occ {
                    [198, 204, 224, 235]
                } else {
                    [118, 124, 146, 180]
                };
                mb.draw_text_px(mx + 5, 7, &ws.to_string(), 11.5, wfg);
                state.panel.hit_regions.push((ws_rect, ClickTarget::Workspace(ws)));
                mx += ww + 2;
            }
        }

        // Right-side macOS Status Icons & Live Clock (`Day DD Mon HH:MM`)
        let clock_str = current_macos_clock();
        let clock_w = (mb.measure_text_px(&clock_str, 12.5) + 16).max(118);
        let clock_x = (mb_w - clock_w - 10).max(mx + 8);
        let clock_global = Rectangle::new(
            (output_geo.loc.x + clock_x, output_geo.loc.y + 2).into(),
            (clock_w, mb_h - 4).into(),
        );
        if state.overlays.calendar_open || rect_contains(clock_global, pointer_x, pointer_y) {
            mb.fill_rounded_rect_aa(clock_x, 3, clock_w, mb_h - 6, 5, [255, 255, 255, 32]);
        }
        mb.draw_text_px(clock_x + 8, 7, &clock_str, 12.5, [242, 244, 252, 255]);
        state
            .panel
            .hit_regions
            .push((clock_global, ClickTarget::ToggleCalendar));

        // Notification Bell (`N`)
        let notif_w = 24;
        let notif_x = clock_x - notif_w - 4;
        let notif_global = Rectangle::new(
            (output_geo.loc.x + notif_x, output_geo.loc.y + 2).into(),
            (notif_w, mb_h - 4).into(),
        );
        if state.overlays.notif_drawer_open || rect_contains(notif_global, pointer_x, pointer_y) {
            mb.fill_rounded_rect_aa(notif_x, 3, notif_w, mb_h - 6, 5, [255, 255, 255, 32]);
        }
        mb.draw_text_px(notif_x + 7, 7, "N", 12.0, [224, 228, 242, 245]);
        state
            .panel
            .hit_regions
            .push((notif_global, ClickTarget::ToggleNotifDrawer));

        // Control Center Icon (`CC`)
        let cc_w = 28;
        let cc_x = notif_x - cc_w - 4;
        let cc_global = Rectangle::new(
            (output_geo.loc.x + cc_x, output_geo.loc.y + 2).into(),
            (cc_w, mb_h - 4).into(),
        );
        if state.overlays.control_center_open || rect_contains(cc_global, pointer_x, pointer_y) {
            mb.fill_rounded_rect_aa(cc_x, 3, cc_w, mb_h - 6, 5, [255, 255, 255, 34]);
        }
        // Two mini pill-switch glyphs representing macOS Control Center
        mb.fill_rounded_rect_aa(cc_x + 6, 8, 16, 5, 2, [232, 236, 248, 235]);
        mb.fill_rounded_rect_aa(cc_x + 6, 15, 16, 5, 2, [180, 188, 212, 210]);
        state
            .panel
            .hit_regions
            .push((cc_global, ClickTarget::ToggleControlCenter));

        // Volume / Battery status pill
        let vol_lbl = if state.panel.volume_muted {
            "Mute".to_string()
        } else if let Some(bat) = state.panel.battery_percent {
            format!("{}% B{}", state.panel.volume_percent, bat)
        } else {
            format!("{}%", state.panel.volume_percent)
        };
        let vol_w = mb.measure_text_px(&vol_lbl, 11.5) + 14;
        let vol_x = cc_x - vol_w - 4;
        let vol_global = Rectangle::new(
            (output_geo.loc.x + vol_x, output_geo.loc.y + 2).into(),
            (vol_w, mb_h - 4).into(),
        );
        if rect_contains(vol_global, pointer_x, pointer_y) {
            mb.fill_rounded_rect_aa(vol_x, 3, vol_w, mb_h - 6, 5, [255, 255, 255, 28]);
        }
        mb.draw_text_px(vol_x + 7, 7, &vol_lbl, 11.5, [215, 220, 236, 240]);
        state
            .panel
            .hit_regions
            .push((vol_global, ClickTarget::ToggleMute));

        let wifi_w = 34;
        let wifi_x = vol_x - wifi_w - 4;
        let wifi_global = Rectangle::new(
            (output_geo.loc.x + wifi_x, output_geo.loc.y + 2).into(),
            (wifi_w, mb_h - 4).into(),
        );
        if rect_contains(wifi_global, pointer_x, pointer_y) {
            mb.fill_rounded_rect_aa(wifi_x, 3, wifi_w, mb_h - 6, 5, [255, 255, 255, 28]);
        }
        mb.draw_text_px(wifi_x + 5, 7, "WiFi", 11.5, [215, 220, 236, 240]);
        state
            .panel
            .hit_regions
            .push((wifi_global, ClickTarget::ToggleControlCenter));

        let mb_buf = MemoryRenderBuffer::from_slice(
            &mb.pixels,
            Fourcc::Abgr8888,
            (mb_w, mb_h),
            1,
            Transform::Normal,
            None,
        );
        if let Ok(elem) = MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            Point::<f64, Physical>::from((0.0, 0.0)),
            &mb_buf,
            None,
            None,
            Some((mb_w, mb_h).into()),
            Kind::Unspecified,
        ) {
            elements.push(OkayRenderElements::Memory(elem));
        }

        // ====================================================================
        // B. Bottom Floating Dock (Centered 18px Glass Pill + Squircle Icons + Zoom)
        // ====================================================================
        let pinned_apps: [(LauncherItem, &str, crate::icons::VectorIconKind); 5] = [
            (LauncherItem::FileManager, "FM", crate::icons::VectorIconKind::Files),
            (LauncherItem::Terminal, ">_", crate::icons::VectorIconKind::Terminal),
            (LauncherItem::Browser, "WB", crate::icons::VectorIconKind::Browser),
            (LauncherItem::OkaySettings, "OS", crate::icons::VectorIconKind::Settings),
            (LauncherItem::SystemMonitor, "SM", crate::icons::VectorIconKind::SystemMonitor),
        ];

        let custom_pinned = state.panel.custom_pinned.clone();
        let base_icon = 42;
        let icon_gap = 10;
        let divider_w = if ws_windows.is_empty() { 0 } else { 18 };
        let dock_pad_x = 16;
        let total_icons = pinned_apps.len() + custom_pinned.len() + ws_windows.len();
        let dock_pill_w = (dock_pad_x * 2
            + total_icons as i32 * base_icon
            + (total_icons as i32 - 1).max(0) * icon_gap
            + divider_w)
            .clamp(280, output_geo.size.w - 32);

        // Extra canvas headroom above the pill so parabolic hover magnification & bounce never clip
        let dock_headroom = 26;
        let dock_canvas_w = dock_pill_w + 40;
        let dock_canvas_h = DOCK_HEIGHT + dock_headroom;
        let dock_pill_x_in_canvas = 20;
        let dock_pill_y_in_canvas = dock_headroom;

        let dock_canvas_x_local = (output_geo.size.w - dock_canvas_w) / 2;
        let dock_pill_y_local = output_geo.size.h - DOCK_HEIGHT - DOCK_MARGIN;
        let dock_canvas_y_local = dock_pill_y_local - dock_headroom;

        let mut dock = Canvas::new(dock_canvas_w, dock_canvas_h);
        dock.fill_glass_panel(
            dock_pill_x_in_canvas,
            dock_pill_y_in_canvas,
            dock_pill_w,
            DOCK_HEIGHT,
            18,
        );

        let dock_zone_y = output_geo.loc.y + dock_pill_y_local - 24;
        let pointer_near_dock = pointer_y >= dock_zone_y
            && pointer_x >= output_geo.loc.x + dock_canvas_x_local
            && pointer_x <= output_geo.loc.x + dock_canvas_x_local + dock_canvas_w;

        let mut dx = dock_pill_x_in_canvas + dock_pad_x;

        for (item, glyph, kind) in pinned_apps {
            let center_global_x = output_geo.loc.x + dock_canvas_x_local + dx + base_icon / 2;
            let zoom = if pointer_near_dock {
                let dist = ((pointer_x - center_global_x) as f32).abs();
                if dist < 96.0 {
                    let norm = 1.0 - dist / 96.0;
                    1.0 + 0.38 * norm * norm * (3.0 - 2.0 * norm)
                } else {
                    1.0
                }
            } else {
                1.0
            };
            let sz = (base_icon as f32 * zoom).round() as i32;
            let bounce_key = format!("pin_{}", glyph);
            let bounce_y = state.animations.dock_bounce_offset(&bounce_key);
            let draw_x = dx - (sz - base_icon) / 2;
            let draw_y = dock_pill_y_in_canvas + 8 - (sz - base_icon) - bounce_y;

            let sys_icon_hint = match item {
                LauncherItem::FileManager => Some(state.panel.default_file_manager.as_str()),
                LauncherItem::Terminal => Some(state.panel.default_terminal.as_str()),
                LauncherItem::Browser => Some(state.panel.default_browser.as_str()),
                _ => None,
            };
            crate::icons::draw_app_icon(&mut dock, draw_x, draw_y, sz, sys_icon_hint, kind);

            // Active indicator dot underneath Settings if open
            if item == LauncherItem::OkaySettings && state.settings.open {
                dock.fill_circle_aa(
                    (dx + base_icon / 2) as f32,
                    (dock_pill_y_in_canvas + DOCK_HEIGHT - 6) as f32,
                    2.5,
                    [235, 240, 255, 235],
                );
            }

            let hit_global = Rectangle::new(
                (
                    output_geo.loc.x + dock_canvas_x_local + draw_x,
                    output_geo.loc.y + dock_canvas_y_local + draw_y,
                )
                    .into(),
                (sz, sz + 10).into(),
            );
            state
                .panel
                .hit_regions
                .push((hit_global, ClickTarget::PinnedApp(item)));

            dx += base_icon + icon_gap;
        }

        // User-pinned custom apps from Desktop Icon Context Menu
        for app in &custom_pinned {
            let center_global_x = output_geo.loc.x + dock_canvas_x_local + dx + base_icon / 2;
            let zoom = if pointer_near_dock {
                let dist = ((pointer_x - center_global_x) as f32).abs();
                if dist < 96.0 {
                    let norm = 1.0 - dist / 96.0;
                    1.0 + 0.36 * norm * norm * (3.0 - 2.0 * norm)
                } else {
                    1.0
                }
            } else {
                1.0
            };
            let sz = (base_icon as f32 * zoom).round() as i32;
            let draw_x = dx - (sz - base_icon) / 2;
            let draw_y = dock_pill_y_in_canvas + 8 - (sz - base_icon);
            let kind = crate::icons::VectorIconKind::from_hint(&app.name, &app.exec, false);
            crate::icons::draw_app_icon(&mut dock, draw_x, draw_y, sz, app.icon_name.as_deref(), kind);

            let hit_global = Rectangle::new(
                (
                    output_geo.loc.x + dock_canvas_x_local + draw_x,
                    output_geo.loc.y + dock_canvas_y_local + draw_y,
                )
                    .into(),
                (sz, sz + 10).into(),
            );
            state
                .panel
                .hit_regions
                .push((hit_global, ClickTarget::LaunchDesktopApp(app.exec.clone())));
            dx += base_icon + icon_gap;
        }

        // Vertical glass divider before open windows
        if !ws_windows.is_empty() {
            dock.fill_rect_blend(
                dx + 2,
                dock_pill_y_in_canvas + 12,
                1,
                DOCK_HEIGHT - 24,
                [255, 255, 255, 42],
            );
            dx += divider_w;

            for (idx, (win_id, title, is_focused, is_min)) in ws_windows.iter().enumerate() {
                if dx + base_icon > dock_pill_x_in_canvas + dock_pill_w - 8 {
                    break;
                }
                let center_global_x = output_geo.loc.x + dock_canvas_x_local + dx + base_icon / 2;
                let zoom = if pointer_near_dock {
                    let dist = ((pointer_x - center_global_x) as f32).abs();
                    if dist < 96.0 {
                        let norm = 1.0 - dist / 96.0;
                        1.0 + 0.36 * norm * norm * (3.0 - 2.0 * norm)
                    } else {
                        1.0
                    }
                } else {
                    1.0
                };
                let sz = (base_icon as f32 * zoom).round() as i32;
                let draw_x = dx - (sz - base_icon) / 2;
                let draw_y = dock_pill_y_in_canvas + 8 - (sz - base_icon);

                let win_global = Rectangle::new(
                    (
                        output_geo.loc.x + dock_canvas_x_local + draw_x,
                        output_geo.loc.y + dock_canvas_y_local + draw_y,
                    )
                        .into(),
                    (sz, sz + 10).into(),
                );
                if let Some(id) = win_id {
                    state
                        .animations
                        .taskbar_button_rects
                        .insert(id.clone(), win_global);
                }

                let kind = crate::icons::VectorIconKind::from_hint(title, title, false);
                crate::icons::draw_app_icon(&mut dock, draw_x, draw_y, sz, Some(title), kind);
                if *is_focused {
                    dock.fill_rounded_rect_aa(draw_x, draw_y, sz, 3, 2, [accent[0], accent[1], accent[2], 220]);
                }

                // Glowing active indicator dot beneath every open window in the Dock
                let dot_col = if *is_focused {
                    [255, 255, 255, 250]
                } else if *is_min {
                    [160, 168, 192, 160]
                } else {
                    [210, 218, 238, 215]
                };
                dock.fill_circle_aa(
                    (dx + base_icon / 2) as f32,
                    (dock_pill_y_in_canvas + DOCK_HEIGHT - 6) as f32,
                    if *is_focused { 2.8 } else { 2.2 },
                    dot_col,
                );

                state
                    .panel
                    .hit_regions
                    .push((win_global, ClickTarget::FocusWindow(idx)));
                dx += base_icon + icon_gap;
            }
        }

        let dock_buf = MemoryRenderBuffer::from_slice(
            &dock.pixels,
            Fourcc::Abgr8888,
            (dock_canvas_w, dock_canvas_h),
            1,
            Transform::Normal,
            None,
        );
        let dock_phys: Point<f64, Physical> =
            Point::<i32, Logical>::from((dock_canvas_x_local, dock_canvas_y_local))
                .to_f64()
                .to_physical(scale);
        if let Ok(elem) = MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            dock_phys,
            &dock_buf,
            None,
            None,
            Some((dock_canvas_w, dock_canvas_h).into()),
            Kind::Unspecified,
        ) {
            elements.push(OkayRenderElements::Memory(elem));
        }
    } else {
        // ====================================================================
        // Classic Full-Width Taskbar (when Dock Mode is toggled OFF in Settings)
        // ====================================================================
        let bar_y_local = if state.panel.panel_top {
            0
        } else {
            output_geo.size.h - 36
        };
        let bar_w = output_geo.size.w;
        let bar_h = 36;
        let mut canvas = Canvas::new(bar_w, bar_h);
        canvas.fill_rect(0, 0, bar_w, bar_h, [24, 26, 36, 219]);
        let line_y = if state.panel.panel_top { bar_h - 1 } else { 0 };
        canvas.fill_rect_blend(0, line_y, bar_w, 1, [255, 255, 255, 36]);

        let mut cursor_x = 10;
        let start_w = 66;
        let start_global = Rectangle::new(
            (output_geo.loc.x + cursor_x, output_geo.loc.y + bar_y_local + 4).into(),
            (start_w, bar_h - 8).into(),
        );
        let start_bg = if state.panel.launcher_open {
            [accent[0], accent[1], accent[2], 255]
        } else if rect_contains(start_global, pointer_x, pointer_y) {
            [62, 64, 84, 255]
        } else {
            [42, 44, 60, 235]
        };
        let start_fg = if state.panel.launcher_open {
            contrast_fg(accent)
        } else {
            [244, 246, 254, 255]
        };
        canvas.fill_rounded_rect_aa(cursor_x, 4, start_w, bar_h - 8, 7, start_bg);
        canvas.draw_text_px(cursor_x + 14, 9, "OKay", 13.5, start_fg);
        state
            .panel
            .hit_regions
            .push((start_global, ClickTarget::ToggleLauncher));
        cursor_x += start_w + 10;

        if state.panel.show_workspaces {
            for ws in 1..=9 {
                let ws_w = 22;
                let is_active = state.workspaces.active == ws;
                let ws_global = Rectangle::new(
                    (output_geo.loc.x + cursor_x, output_geo.loc.y + bar_y_local + 5).into(),
                    (ws_w, bar_h - 10).into(),
                );
                let bg = if is_active {
                    [accent[0], accent[1], accent[2], 255]
                } else if occupied_workspaces[ws - 1] {
                    [48, 50, 68, 235]
                } else {
                    [32, 34, 48, 200]
                };
                let fg = if is_active {
                    contrast_fg(accent)
                } else {
                    [225, 228, 242, 245]
                };
                canvas.fill_rounded_rect_aa(cursor_x, 5, ws_w, bar_h - 10, 5, bg);
                canvas.draw_text_px(cursor_x + 7, 9, &ws.to_string(), 12.5, fg);
                state
                    .panel
                    .hit_regions
                    .push((ws_global, ClickTarget::Workspace(ws)));
                cursor_x += ws_w + 4;
            }
            cursor_x += 8;
        }

        let clock_str = current_macos_clock();
        let clock_w = (canvas.measure_text_px(&clock_str, 12.5) + 16).max(118);
        let clock_x = (bar_w - clock_w - 10).max(cursor_x);
        let clock_global = Rectangle::new(
            (output_geo.loc.x + clock_x, output_geo.loc.y + bar_y_local + 4).into(),
            (clock_w, bar_h - 8).into(),
        );
        canvas.fill_rounded_rect_aa(clock_x, 4, clock_w, bar_h - 8, 6, [38, 40, 56, 235]);
        canvas.draw_text_px(clock_x + 8, 9, &clock_str, 12.5, [240, 242, 252, 255]);
        state
            .panel
            .hit_regions
            .push((clock_global, ClickTarget::ToggleCalendar));

        let avail_w = (clock_x - cursor_x - 10).max(0);
        if !ws_windows.is_empty() && avail_w > 56 {
            let btn_w = ((avail_w / ws_windows.len() as i32) - 6).clamp(52, 165);
            for (idx, (win_id, title, is_focused, _is_min)) in ws_windows.iter().enumerate() {
                let bx = cursor_x + idx as i32 * (btn_w + 6);
                if bx + btn_w > clock_x - 6 {
                    break;
                }
                let win_global = Rectangle::new(
                    (output_geo.loc.x + bx, output_geo.loc.y + bar_y_local + 4).into(),
                    (btn_w, bar_h - 8).into(),
                );
                if let Some(id) = win_id {
                    state
                        .animations
                        .taskbar_button_rects
                        .insert(id.clone(), win_global);
                }
                let bg = if *is_focused {
                    [58, 62, 84, 245]
                } else {
                    [36, 38, 52, 225]
                };
                canvas.fill_rounded_rect_aa(bx, 4, btn_w, bar_h - 8, 6, bg);
                let max_chars = ((btn_w - 16) / 8).max(2) as usize;
                let short: String = title.chars().take(max_chars).collect();
                canvas.draw_text_px(bx + 8, 9, &short, 12.5, [238, 240, 252, 255]);
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
        let bar_phys: Point<f64, Physical> =
            Point::<i32, Logical>::from((0, bar_y_local))
                .to_f64()
                .to_physical(scale);
        if let Ok(elem) = MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            bar_phys,
            &bar_buf,
            None,
            None,
            Some((bar_w, bar_h).into()),
            Kind::Unspecified,
        ) {
            elements.push(OkayRenderElements::Memory(elem));
        }
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
        let menu_x = 12;
        let slide_px = ((1.0 - menu_anim) * 14.0).round() as i32;
        let menu_y = if state.panel.dock_mode || state.panel.panel_top {
            PANEL_HEIGHT + 8 - slide_px
        } else {
            (output_geo.size.h - 36 - menu_h - 8 + slide_px).max(8)
        };

        let mut menu_canvas = Canvas::new(MENU_WIDTH, menu_h);
        menu_canvas.fill_glass_panel(0, 0, MENU_WIDTH, menu_h, 14);

        // Header + Search Box
        menu_canvas.draw_text_px(14, 10, "OKay Spotlight", 14.5, [246, 248, 255, 255]);
        menu_canvas.draw_text_px(142, 11, &focused_app_name, 12.0, [156, 164, 188, 230]);

        menu_canvas.fill_rounded_rect_aa(12, 38, MENU_WIDTH - 24, 28, 8, [255, 255, 255, 18]);
        let search_txt = if state.panel.search_query.is_empty() {
            "Search apps & commands...".to_string()
        } else {
            format!("{}|", state.panel.search_query)
        };
        let search_fg = if state.panel.search_query.is_empty() {
            [142, 148, 172, 235]
        } else {
            [248, 250, 255, 255]
        };
        menu_canvas.draw_text_px(22, 44, &search_txt, 12.5, search_fg);

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

            menu_canvas.fill_rounded_rect_aa(
                MENU_PADDING,
                iy + 2,
                MENU_WIDTH - MENU_PADDING * 2,
                MENU_ITEM_HEIGHT - 4,
                6,
                item_bg,
            );
            let item_kind = match item {
                LauncherItem::Terminal => crate::icons::VectorIconKind::Terminal,
                LauncherItem::FileManager => crate::icons::VectorIconKind::Files,
                LauncherItem::Browser => crate::icons::VectorIconKind::Browser,
                LauncherItem::SystemMonitor => crate::icons::VectorIconKind::SystemMonitor,
                LauncherItem::Screenshot => crate::icons::VectorIconKind::Screenshot,
                LauncherItem::OkaySettings | LauncherItem::ReloadConfig => crate::icons::VectorIconKind::Settings,
            };
            crate::icons::draw_app_icon(
                &mut menu_canvas,
                MENU_PADDING + 8,
                iy + 6,
                22,
                None,
                item_kind,
            );
            menu_canvas.draw_text_px(
                MENU_PADDING + 38,
                iy + 8,
                item.label(),
                13.5,
                [238, 240, 252, 255],
            );
            let hint_w = menu_canvas.measure_text_px(item.hint(), 11.5);
            menu_canvas.draw_text_px(
                MENU_WIDTH - MENU_PADDING - hint_w - 10,
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

            menu_canvas.fill_rounded_rect_aa(
                MENU_PADDING,
                iy + 2,
                MENU_WIDTH - MENU_PADDING * 2,
                MENU_ITEM_HEIGHT - 4,
                6,
                item_bg,
            );
            let kind = crate::icons::VectorIconKind::from_hint(&app.name, &app.exec, false);
            crate::icons::draw_app_icon(
                &mut menu_canvas,
                MENU_PADDING + 8,
                iy + 6,
                22,
                app.icon_name.as_deref(),
                kind,
            );
            let short_name: String = app.name.chars().take(24).collect();
            menu_canvas.draw_text_px(
                MENU_PADDING + 38,
                iy + 8,
                &short_name,
                13.5,
                [235, 238, 250, 255],
            );
            let cat_w = menu_canvas.measure_text_px(&app.category, 11.5);
            menu_canvas.draw_text_px(
                MENU_WIDTH - MENU_PADDING - cat_w - 10,
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

/// Renders a 1:1 macOS-style server-side titlebar with SDF rounded top corners (`12px`),
/// top-left Traffic Light buttons (`#ff5f57`, `#febc2e`, `#28c840` with `x`, `-`, `+` on hover),
/// and centered Maple Mono window title.
pub fn render_titlebar_element(
    window: &Window,
    local_win_loc: Point<i32, Logical>,
    pointer_local: Point<f64, Logical>,
    is_focused: bool,
    is_pinned: bool,
    _accent: [u8; 3],
    renderer: &mut GlesRenderer,
    scale: Scale<f64>,
) -> Option<OkayRenderElements> {
    let win_geo = window.geometry();
    let tb_w = win_geo.size.w.max(120);
    let tb_h = crate::state::TITLEBAR_HEIGHT;
    let tb_x = local_win_loc.x + win_geo.loc.x;
    let tb_y = local_win_loc.y + win_geo.loc.y - tb_h;

    let mut canvas = Canvas::new(tb_w, tb_h);
    let bg = if is_focused {
        [30, 32, 44, 240]
    } else {
        [22, 24, 34, 224]
    };
    // SDF anti-aliased rounded top corners (12px radius), square bottom edge meeting the client surface
    canvas.fill_rounded_rect_aa(0, 0, tb_w, tb_h + 12, 12, bg);
    // Subtle 1px rgba(255, 255, 255, 0.14) inner top highlight
    if tb_w > 24 {
        canvas.fill_rect_blend(12, 1, tb_w - 24, 1, [255, 255, 255, 36]);
    }
    // Subtle 1px separator line at bottom of titlebar
    canvas.fill_rect_blend(0, tb_h - 1, tb_w, 1, [255, 255, 255, 18]);

    // Check if pointer is hovering over the Traffic Lights cluster on the top-left
    let rel_px = pointer_local.x.round() as i32 - tb_x;
    let rel_py = pointer_local.y.round() as i32 - tb_y;
    let lights_hovered = (6..=66).contains(&rel_px) && (4..=tb_h - 4).contains(&rel_py);

    // Top-left macOS Traffic Light buttons: Close (#ff5f57), Minimize (#febc2e), Maximize (#28c840)
    let cy = tb_h as f32 * 0.5;
    let lights = [
        (16.0f32, [255, 95, 87, 255], "x"),
        (36.0f32, [254, 188, 46, 255], "-"),
        (56.0f32, [40, 200, 64, 255], "+"),
    ];
    for (cx, active_col, sym) in lights {
        let col = if is_focused || lights_hovered {
            active_col
        } else {
            [72, 74, 88, 220]
        };
        canvas.fill_circle_aa(cx, cy, 6.0, col);
        if lights_hovered {
            canvas.draw_text_px((cx - 3.5).round() as i32, (cy - 6.5).round() as i32, sym, 11.0, [24, 18, 18, 220]);
        }
    }

    // Centered window title
    let mut title = window_title_str(window);
    if is_pinned {
        title = format!("[PIN] {}", title);
    }
    let avail_title_w = (tb_w - 144).max(32);
    let max_chars = (avail_title_w / 8).max(1) as usize;
    let short_title: String = title.chars().take(max_chars).collect();
    let title_w = canvas.measure_text_px(&short_title, 13.0);
    let title_x = ((tb_w - title_w) / 2).max(74);
    let title_fg = if is_focused {
        [238, 240, 250, 255]
    } else {
        [152, 156, 174, 255]
    };
    canvas.draw_text_px(title_x, 7, &short_title, 13.0, title_fg);

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

/// Formats local time as macOS-style `Day DD Mon HH:MM` (e.g. `Sat 14 Nov 22:14`).
pub fn current_macos_clock() -> String {
    unsafe {
        let mut t: libc::time_t = 0;
        libc::time(&mut t);
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let day = DAYS[(tm.tm_wday as usize).min(6)];
        let mon = MONTHS[(tm.tm_mon as usize).min(11)];
        format!(
            "{} {:02} {} {:02}:{:02}",
            day, tm.tm_mday, mon, tm.tm_hour, tm.tm_min
        )
    }
}

/// Returns high-contrast foreground text color (`dark` or `white`) for any background RGB.
pub fn contrast_fg(bg: [u8; 3]) -> [u8; 4] {
    let luma = 0.2126 * bg[0] as f32 + 0.7152 * bg[1] as f32 + 0.0722 * bg[2] as f32;
    if luma > 152.0 {
        [18, 20, 28, 255]
    } else {
        [244, 246, 252, 255]
    }
}

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

    /// Smooth SDF anti-aliased rounded rectangle.
    pub fn fill_rounded_rect_aa(&mut self, x: i32, y: i32, w: i32, h: i32, radius: i32, color: [u8; 4]) {
        if w <= 0 || h <= 0 || color[3] == 0 {
            return;
        }
        let r = (radius as f32).clamp(0.0, (w.min(h) as f32) * 0.5);
        let x0 = (x - 1).clamp(0, self.width);
        let y0 = (y - 1).clamp(0, self.height);
        let x1 = (x + w + 1).clamp(0, self.width);
        let y1 = (y + h + 1).clamp(0, self.height);
        let cx = x as f32 + w as f32 * 0.5;
        let cy = y as f32 + h as f32 * 0.5;
        let hw = w as f32 * 0.5 - r;
        let hh = h as f32 * 0.5 - r;

        for py in y0..y1 {
            for px in x0..x1 {
                let dx = (px as f32 + 0.5 - cx).abs() - hw;
                let dy = (py as f32 + 0.5 - cy).abs() - hh;
                let dist = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.max(dy).min(0.0) - r;
                let cov = (0.5 - dist).clamp(0.0, 1.0);
                if cov > 0.01 {
                    let a = ((color[3] as f32 * cov).round() as u32).min(255) as u8;
                    self.blend_pixel(px, py, [color[0], color[1], color[2], a]);
                }
            }
        }
    }

    /// Smooth SDF anti-aliased filled circle.
    pub fn fill_circle_aa(&mut self, cx: f32, cy: f32, radius: f32, color: [u8; 4]) {
        if radius <= 0.0 || color[3] == 0 {
            return;
        }
        let x0 = ((cx - radius - 1.5).floor() as i32).clamp(0, self.width);
        let y0 = ((cy - radius - 1.5).floor() as i32).clamp(0, self.height);
        let x1 = ((cx + radius + 1.5).ceil() as i32).clamp(0, self.width);
        let y1 = ((cy + radius + 1.5).ceil() as i32).clamp(0, self.height);

        for py in y0..y1 {
            for px in x0..x1 {
                let dx = px as f32 + 0.5 - cx;
                let dy = py as f32 + 0.5 - cy;
                let dist = (dx * dx + dy * dy).sqrt() - radius;
                let cov = (0.5 - dist).clamp(0.0, 1.0);
                if cov > 0.01 {
                    let a = ((color[3] as f32 * cov).round() as u32).min(255) as u8;
                    self.blend_pixel(px, py, [color[0], color[1], color[2], a]);
                }
            }
        }
    }

    /// Draws a macOS-style gradient squircle icon with subtle top specular highlight.
    pub fn fill_squircle_gradient(
        &mut self,
        x: i32,
        y: i32,
        size: i32,
        top_rgb: [u8; 3],
        bot_rgb: [u8; 3],
    ) {
        if size <= 4 {
            return;
        }
        let r = (size as f32 * 0.24).max(4.0);
        let x0 = (x - 1).clamp(0, self.width);
        let y0 = (y - 1).clamp(0, self.height);
        let x1 = (x + size + 1).clamp(0, self.width);
        let y1 = (y + size + 1).clamp(0, self.height);
        let cx = x as f32 + size as f32 * 0.5;
        let cy = y as f32 + size as f32 * 0.5;
        let half = size as f32 * 0.5 - r;

        for py in y0..y1 {
            let t = ((py - y) as f32 / size as f32).clamp(0.0, 1.0);
            let cr = (top_rgb[0] as f32 * (1.0 - t) + bot_rgb[0] as f32 * t).round() as u8;
            let cg = (top_rgb[1] as f32 * (1.0 - t) + bot_rgb[1] as f32 * t).round() as u8;
            let cb = (top_rgb[2] as f32 * (1.0 - t) + bot_rgb[2] as f32 * t).round() as u8;
            for px in x0..x1 {
                let dx = (px as f32 + 0.5 - cx).abs() - half;
                let dy = (py as f32 + 0.5 - cy).abs() - half;
                let dist = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.max(dy).min(0.0) - r;
                let cov = (0.5 - dist).clamp(0.0, 1.0);
                if cov > 0.01 {
                    let a = (255.0 * cov).round() as u8;
                    self.blend_pixel(px, py, [cr, cg, cb, a]);
                    // Subtle 1px specular top rim
                    if py == y + 1 && dist < -0.5 {
                        self.blend_pixel(px, py, [255, 255, 255, (48.0 * cov) as u8]);
                    }
                }
            }
        }
    }

    /// Draws a sleek macOS translucent dark glass panel (`rgba(24, 26, 36, 0.86)`)
    /// with a soft drop shadow and `1px rgba(255, 255, 255, 0.14)` inner top highlight.
    pub fn fill_glass_panel(&mut self, x: i32, y: i32, w: i32, h: i32, radius: i32) {
        // Soft drop shadow
        self.fill_rounded_rect_aa(x - 2, y + 3, w + 4, h + 4, radius + 2, [0, 0, 0, 56]);
        self.fill_rounded_rect_aa(x - 1, y + 1, w + 2, h + 2, radius + 1, [0, 0, 0, 72]);
        // Translucent dark glass body: rgba(24, 26, 36, 0.90) -> alpha 230
        self.fill_rounded_rect_aa(x, y, w, h, radius, [24, 26, 36, 230]);
        // Subtle 1px inner top highlight: rgba(255, 255, 255, 0.14) -> alpha 36
        let inset = (radius / 2).max(4);
        if w > inset * 2 {
            self.fill_rect_blend(x + inset, y + 1, w - inset * 2, 1, [255, 255, 255, 36]);
        }
    }

    pub fn fill_rect_blend(&mut self, x: i32, y: i32, w: i32, h: i32, color: [u8; 4]) {
        let x0 = x.clamp(0, self.width);
        let y0 = y.clamp(0, self.height);
        let x1 = (x + w).clamp(0, self.width);
        let y1 = (y + h).clamp(0, self.height);
        for py in y0..y1 {
            for px in x0..x1 {
                self.blend_pixel(px, py, color);
            }
        }
    }

    pub fn stroke_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: [u8; 4]) {
        self.fill_rect(x, y, w, 1, color);
        self.fill_rect(x, y + h - 1, w, 1, color);
        self.fill_rect(x, y, 1, h, color);
        self.fill_rect(x + w - 1, y, 1, h, color);
    }

    pub fn draw_text_shadowed(&mut self, start_x: i32, start_y: i32, text: &str, px_size: f32, color: [u8; 4]) {
        self.draw_text_px(start_x, start_y + 1, text, px_size, [0, 0, 0, 145]);
        self.draw_text_px(start_x, start_y, text, px_size, color);
    }

    pub fn measure_text_px(&self, text: &str, px_size: f32) -> i32 {
        let mut width = 0.0f32;
        with_font_engine(|engine| {
            if engine.font.is_some() {
                for ch in text.chars() {
                    if let Some((metrics, _)) = engine.rasterize_cached(ch, px_size) {
                        width += metrics.advance_width;
                    }
                }
            }
        });
        width.ceil() as i32
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
