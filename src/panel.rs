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
const MENU_WIDTH: i32 = 700;
const MENU_HEADER_HEIGHT: i32 = 88;
const MENU_FOOTER_HEIGHT: i32 = 58;
const MENU_PADDING: i32 = 16;

/// Accent color presets: `(Name, [R, G, B], linear [f32; 4])`.
pub const ACCENT_PRESETS: [(&str, [u8; 3], [f32; 4]); 6] = [
    ("Lavender", [201, 160, 220], [0.788, 0.627, 0.863, 1.0]),
    ("Sky Blue", [137, 180, 250], [0.537, 0.706, 0.980, 1.0]),
    ("Emerald", [166, 227, 161], [0.651, 0.890, 0.631, 1.0]),
    ("Coral", [243, 139, 168], [0.953, 0.545, 0.659, 1.0]),
    ("Amber", [249, 226, 175], [0.976, 0.886, 0.686, 1.0]),
    ("Crimson", [235, 111, 146], [0.922, 0.435, 0.573, 1.0]),
];

/// Visual appearance mode for OKay DE surfaces (live-switchable in System Settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppearanceMode {
    /// Material 3 Expressive (M3E): opaque/tonal `surfaceContainer`, clear `outlineVariant`,
    /// soft SDF drop shadows, and vibrant color accents.
    Material3Expressive,
    /// Glass / Blur: translucent surfaces with Dual-Kawase blur, anti-banding noise grain,
    /// and a subtle 1px bright edge on Dock, Top Bar, and system windows.
    GlassBlur,
}

impl AppearanceMode {
    pub fn as_config_str(self) -> &'static str {
        match self {
            Self::Material3Expressive => "material3_expressive",
            Self::GlassBlur => "glass_blur",
        }
    }

    pub fn from_str(val: &str) -> Self {
        match val.trim().to_ascii_lowercase().as_str() {
            "m3e" | "m3_expressive" | "material3_expressive" | "expressive" => {
                Self::Material3Expressive
            }
            _ => Self::GlassBlur,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MaterialPalette {
    pub accent: [u8; 3],
    pub surface: [u8; 3],
    pub surface_container: [u8; 3],
    pub outline: [u8; 3],
    pub outline_variant: [u8; 3],
    pub expressive: bool,
    pub appearance_mode: AppearanceMode,
}

impl Default for MaterialPalette {
    fn default() -> Self {
        Self {
            accent: [137, 180, 250],
            surface: [24, 26, 36],
            surface_container: [32, 35, 48],
            outline: [76, 86, 112],
            outline_variant: [88, 98, 128],
            expressive: false,
            appearance_mode: AppearanceMode::GlassBlur,
        }
    }
}

thread_local! {
    static ACTIVE_M3_PALETTE: std::cell::Cell<MaterialPalette> =
        std::cell::Cell::new(MaterialPalette::default());
}

pub fn set_active_m3_palette(pal: MaterialPalette) {
    ACTIVE_M3_PALETTE.with(|c| c.set(pal));
}

pub fn active_m3_palette() -> MaterialPalette {
    ACTIVE_M3_PALETTE.with(|c| c.get())
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarMenu {
    File,
    View,
    Window,
    Help,
}

#[derive(Debug, Clone)]
pub enum PinContextAction {
    PinApp(DesktopAppEntry),
    UnpinBuiltin(LauncherItem),
    UnpinCustom(String),
}

#[derive(Debug, Clone)]
enum ClickTarget {
    ToggleLauncher,
    ToggleSettings,
    OpenSettingsTab(crate::settings::SettingsTab),
    ToggleCalendar,
    ToggleNotifDrawer,
    ToggleControlCenter,
    ToggleTrayDrawer,
    CloseWindowId(ObjectId),
    ToggleMute,
    ToggleTiling,
    ToggleOverview,
    AreaScreenshot,
    ToggleDesktopIcons,
    ReloadWallpaper,
    OpenTopMenu(TopBarMenu, i32),
    WinCenter,
    WinMinimize,
    WinMaximize,
    WinPin,
    WinNextWs,
    WinClose,
    PinnedApp(LauncherItem),
    Workspace(usize),
    FocusWindow(ObjectId),
    LaunchItem(LauncherItem),
    LaunchDesktopApp(String),
    ApplyPinAction(PinContextAction),
    PowerLock,
    PowerSuspend,
    PowerReboot,
    PowerShutdown,
    PowerExit,
    OpenAdShieldSettings,
}

#[derive(Debug)]
pub struct PanelState {
    pub launcher_open: bool,
    pub show_workspaces: bool,
    pub dock_mode: bool,
    pub panel_top: bool,
    pub accent_preset: usize,
    pub dynamic_accent: bool,
    pub m3_expressive: bool,
    pub appearance_mode: AppearanceMode,
    pub wallpaper_accent: Option<[u8; 3]>,
    pub window_gaps: i32,
    pub search_query: String,
    pub desktop_apps: Vec<DesktopAppEntry>,
    pub custom_pinned: Vec<DesktopAppEntry>,
    pub hidden_builtins: Vec<LauncherItem>,
    pub pin_context_menu: Option<(Point<i32, Logical>, PinContextAction)>,
    pub top_bar_menu: Option<(TopBarMenu, i32)>,
    pub tray_open: bool,
    pub dock_auto_revealed: bool,
    pub default_terminal: String,
    pub default_browser: String,
    pub default_file_manager: String,
    pub volume_percent: i32,
    pub volume_muted: bool,
    pub battery_percent: Option<i32>,
    pub autostart_cmds: Vec<String>,
    pub lock_pin: String,
    pub adshield_enabled: bool,
    pub pointer_accel_disabled: bool,
    pub adshield_whitelist: Vec<String>,
    pub adshield_blacklist: Vec<String>,
    hit_regions: Vec<(Rectangle<i32, Logical>, ClickTarget)>,
    right_hit_regions: Vec<(Rectangle<i32, Logical>, PinContextAction)>,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            launcher_open: false,
            show_workspaces: true,
            dock_mode: true,
            panel_top: true,
            accent_preset: 1,
            dynamic_accent: false,
            m3_expressive: false,
            appearance_mode: AppearanceMode::GlassBlur,
            wallpaper_accent: None,
            window_gaps: 10,
            search_query: String::new(),
            desktop_apps: Vec::new(),
            custom_pinned: Vec::new(),
            hidden_builtins: Vec::new(),
            pin_context_menu: None,
            top_bar_menu: None,
            tray_open: false,
            dock_auto_revealed: false,
            default_terminal: "foot".to_string(),
            default_browser: "firefox".to_string(),
            default_file_manager: "thunar".to_string(),
            volume_percent: 75,
            volume_muted: false,
            battery_percent: None,
            autostart_cmds: Vec::new(),
            lock_pin: String::new(),
            adshield_enabled: true,
            pointer_accel_disabled: false,
            adshield_whitelist: Vec::new(),
            adshield_blacklist: Vec::new(),
            hit_regions: Vec::new(),
            right_hit_regions: Vec::new(),
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
    pub lock_pin: String,
}

impl PanelState {
    pub fn new() -> Self {
        let mut s = Self::default();
        let _ = s.load_config_file();
        s
    }

    pub fn accent_rgb(&self) -> [u8; 3] {
        if self.dynamic_accent {
            if let Some(wp_rgb) = self.wallpaper_accent {
                return wp_rgb;
            }
        }
        ACCENT_PRESETS[self.accent_preset % ACCENT_PRESETS.len()].1
    }

    pub fn accent_f32(&self) -> [f32; 4] {
        let rgb = self.accent_rgb();
        [
            rgb[0] as f32 / 255.0,
            rgb[1] as f32 / 255.0,
            rgb[2] as f32 / 255.0,
            1.0,
        ]
    }

    /// Computes the cohesive Material You / M3 Expressive tonal palette from the active accent.
    pub fn material_palette(&self) -> MaterialPalette {
        let raw_accent = self.accent_rgb();
        let is_m3e = self.appearance_mode == AppearanceMode::Material3Expressive;
        let accent = if is_m3e {
            // Boost vibrancy slightly in Material 3 Expressive mode
            let max_c = raw_accent[0].max(raw_accent[1]).max(raw_accent[2]).max(1) as f32;
            let boost = (245.0 / max_c).clamp(1.0, 1.18);
            [
                ((raw_accent[0] as f32 * boost).round().clamp(0.0, 255.0)) as u8,
                ((raw_accent[1] as f32 * boost).round().clamp(0.0, 255.0)) as u8,
                ((raw_accent[2] as f32 * boost).round().clamp(0.0, 255.0)) as u8,
            ]
        } else {
            raw_accent
        };
        let mix = |base: u8, tint: u8, t: f32| -> u8 {
            ((base as f32 * (1.0 - t) + tint as f32 * t).round().clamp(0.0, 255.0)) as u8
        };
        let t_surf = if is_m3e { 0.14 } else { 0.07 };
        let t_cont = if is_m3e { 0.22 } else { 0.12 };
        let t_out = if is_m3e { 0.44 } else { 0.24 };
        let t_out_var = if is_m3e { 0.36 } else { 0.18 };

        MaterialPalette {
            accent,
            surface: [
                mix(22, accent[0], t_surf),
                mix(24, accent[1], t_surf),
                mix(32, accent[2], t_surf),
            ],
            surface_container: [
                mix(32, accent[0], t_cont),
                mix(34, accent[1], t_cont),
                mix(46, accent[2], t_cont),
            ],
            outline: [
                mix(72, accent[0], t_out),
                mix(78, accent[1], t_out),
                mix(98, accent[2], t_out),
            ],
            outline_variant: [
                mix(88, accent[0], t_out_var),
                mix(94, accent[1], t_out_var),
                mix(118, accent[2], t_out_var),
            ],
            expressive: is_m3e,
            appearance_mode: self.appearance_mode,
        }
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
            lock_pin: String::new(),
        };
        let Some(path) = config_file_path() else {
            return cfg;
        };
        let Ok(raw_bytes) = fs::read(path) else {
            return cfg;
        };
        let content = String::from_utf8_lossy(&raw_bytes);
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
                    "dock_mode" => self.dock_mode = true,
                    "panel_top" => self.panel_top = val == "true",
                    "show_workspaces" => self.show_workspaces = val != "false",
                    "accent_preset" => {
                        if let Ok(idx) = val.parse::<usize>() {
                            self.accent_preset = idx % ACCENT_PRESETS.len();
                        }
                    }
                    "dynamic_accent" => self.dynamic_accent = val == "true",
                    "m3_expressive" => {
                        self.m3_expressive = val == "true";
                        if self.m3_expressive {
                            self.appearance_mode = AppearanceMode::Material3Expressive;
                        }
                    }
                    "appearance_mode" => {
                        self.appearance_mode = AppearanceMode::from_str(val);
                        self.m3_expressive =
                            self.appearance_mode == AppearanceMode::Material3Expressive;
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
                    "adshield_enabled" => self.adshield_enabled = val != "false",
                    "pointer_accel_disabled" => self.pointer_accel_disabled = val == "true",
                    "adshield_whitelist" => {
                        if !val.is_empty() && !self.adshield_whitelist.iter().any(|d| d == val) {
                            self.adshield_whitelist.push(val.to_string());
                        }
                    }
                    "adshield_blacklist" => {
                        if !val.is_empty() && !self.adshield_blacklist.iter().any(|d| d == val) {
                            self.adshield_blacklist.push(val.to_string());
                        }
                    }
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
                    "lock_pin" => {
                        cfg.lock_pin = val.to_string();
                        self.lock_pin = val.to_string();
                    }
                    "hidden_pin" => {
                        if let Some(item) = LauncherItem::BUILTIN.iter().find(|i| i.label() == val).copied() {
                            if !self.hidden_builtins.contains(&item) {
                                self.hidden_builtins.push(item);
                            }
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
            "# OKay DE Configuration\ndock_mode = {}\npanel_top = {}\nshow_workspaces = {}\naccent_preset = {}\ndynamic_accent = {}\nm3_expressive = {}\nwindow_gaps = {}\npointer_speed = {:.2}\nnatural_scroll = {}\nanimations_enabled = {}\nanimation_speed = {:.2}\nminimize_effect = \"{}\"\ndefault_terminal = \"{}\"\ndefault_browser = \"{}\"\ndefault_file_manager = \"{}\"\nxkb_layout = \"{}\"\n",
            self.dock_mode,
            self.panel_top,
            self.show_workspaces,
            self.accent_preset,
            self.dynamic_accent,
            self.m3_expressive,
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
        body.push_str(&format!("appearance_mode = \"{}\"\n", self.appearance_mode.as_config_str()));
        body.push_str(&format!("adshield_enabled = {}\n", self.adshield_enabled));
        body.push_str(&format!("pointer_accel_disabled = {}\n", self.pointer_accel_disabled));
        for d in &self.adshield_whitelist {
            body.push_str(&format!("adshield_whitelist = \"{}\"\n", d));
        }
        for d in &self.adshield_blacklist {
            body.push_str(&format!("adshield_blacklist = \"{}\"\n", d));
        }
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
        if !self.lock_pin.is_empty() {
            body.push_str(&format!("lock_pin = \"{}\"\n", self.lock_pin));
        }
        for h in &self.hidden_builtins {
            body.push_str(&format!("hidden_pin = \"{}\"\n", h.label()));
        }
        let _ = fs::write(path, body);
    }

    pub fn set_volume(&mut self, new_vol: i32) {
        let clamped = new_vol.clamp(0, 100);
        if clamped == self.volume_percent {
            return;
        }
        self.volume_percent = clamped;
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
    let mut dirs = vec![
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
    ];
    if let Ok(home) = std::env::var("HOME") {
        let h = PathBuf::from(home);
        dirs.insert(0, h.join(".local/share/flatpak/exports/share/applications"));
        dirs.insert(0, h.join(".local/share/applications"));
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
            let Ok(raw_bytes) = fs::read(&path) else {
                continue;
            };
            let text = String::from_utf8_lossy(&raw_bytes);
            let mut in_main_section = false;
            let mut name = None;
            let mut exec = None;
            let mut icon_name = None;
            let mut categories = String::new();
            let mut no_display = false;
            let mut non_app_type = false;

            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if line.starts_with('[') {
                    if line == "[Desktop Entry]" {
                        in_main_section = true;
                        continue;
                    } else if in_main_section {
                        break;
                    } else {
                        continue;
                    }
                }
                if !in_main_section {
                    continue;
                }
                if line == "NoDisplay=true" || line == "Hidden=true" {
                    no_display = true;
                } else if let Some(t) = line.strip_prefix("Type=") {
                    if t.trim() != "Application" {
                        non_app_type = true;
                    }
                } else if name.is_none() && line.starts_with("Name=") {
                    name = line.strip_prefix("Name=").map(|s| s.trim().to_string());
                } else if icon_name.is_none() && line.starts_with("Icon=") {
                    icon_name = line
                        .strip_prefix("Icon=")
                        .map(|s| s.trim().trim_matches('"').to_string())
                        .filter(|s| !s.is_empty());
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

            if no_display || non_app_type {
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

/// Handles a right-click at global logical coordinates on Dock icons or Launcher apps.
pub fn handle_panel_right_click(state: &mut OkayState, global_pos: Point<f64, Logical>) -> bool {
    let px = global_pos.x.round() as i32;
    let py = global_pos.y.round() as i32;

    for (rect, action) in state.panel.right_hit_regions.iter().rev() {
        if rect_contains(*rect, px, py) {
            state.panel.pin_context_menu = Some((Point::from((px, py)), action.clone()));
            return true;
        }
    }
    if state.panel.pin_context_menu.is_some() {
        state.panel.pin_context_menu = None;
        return true;
    }
    false
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
        if !matches!(target, ClickTarget::ApplyPinAction(_)) {
            state.panel.pin_context_menu = None;
        }
        if !matches!(target, ClickTarget::OpenTopMenu(_, _)) {
            state.panel.top_bar_menu = None;
        }
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
            ClickTarget::OpenSettingsTab(tab) => {
                state.panel.launcher_open = false;
                state.settings.open = true;
                state.settings.active_tab = tab;
                state.settings.refresh_system_info();
            }
            ClickTarget::ToggleCalendar => {
                state.panel.launcher_open = false;
                state.panel.tray_open = false;
                state.overlays.calendar_open = !state.overlays.calendar_open;
                state.overlays.notif_drawer_open = false;
            }
            ClickTarget::ToggleNotifDrawer => {
                state.panel.launcher_open = false;
                state.panel.tray_open = false;
                state.overlays.notif_drawer_open = !state.overlays.notif_drawer_open;
                state.overlays.calendar_open = false;
                state.overlays.control_center_open = false;
            }
            ClickTarget::ToggleControlCenter => {
                state.panel.launcher_open = false;
                state.panel.tray_open = false;
                state.overlays.control_center_open = !state.overlays.control_center_open;
                state.overlays.calendar_open = false;
                state.overlays.notif_drawer_open = false;
            }
            ClickTarget::ToggleTrayDrawer => {
                state.panel.launcher_open = false;
                state.panel.tray_open = !state.panel.tray_open;
                state.overlays.calendar_open = false;
                state.overlays.notif_drawer_open = false;
                state.overlays.control_center_open = false;
            }
            ClickTarget::CloseWindowId(id) => {
                if let Some(win) = state
                    .space
                    .elements()
                    .find(|w| w.toplevel().map(|t| t.wl_surface().id() == id).unwrap_or(false))
                    .cloned()
                {
                    if let Some(toplevel) = win.toplevel() {
                        toplevel.send_close();
                    }
                }
            }
            ClickTarget::OpenTopMenu(menu, anchor_x) => {
                state.panel.launcher_open = false;
                if state.panel.top_bar_menu.map(|(m, _)| m) == Some(menu) {
                    state.panel.top_bar_menu = None;
                } else {
                    state.panel.top_bar_menu = Some((menu, anchor_x));
                    state.animations.top_menu_open_anim = 0.0;
                }
            }
            ClickTarget::ToggleMute => {
                state.panel.toggle_mute();
                let val = if state.panel.volume_muted {
                    "Muted".to_string()
                } else {
                    format!("{}%", state.panel.volume_percent)
                };
                state.overlays.show_osd(
                    "Volume",
                    &val,
                    if state.panel.volume_muted { Some(0) } else { Some(state.panel.volume_percent) },
                );
            }
            ClickTarget::ToggleTiling => {
                state.panel.launcher_open = false;
                state.toggle_tiling();
            }
            ClickTarget::ToggleOverview => {
                state.panel.launcher_open = false;
                state.overlays.overview_open = !state.overlays.overview_open;
            }
            ClickTarget::AreaScreenshot => {
                state.panel.launcher_open = false;
                crate::overlays::begin_area_screenshot(state);
            }
            ClickTarget::ToggleDesktopIcons => {
                state.overlays.show_desktop_icons = !state.overlays.show_desktop_icons;
            }
            ClickTarget::ReloadWallpaper => {
                state.desktop.reload();
                state.overlays.push_notification("Wallpaper", "Reloaded desktop wallpaper");
            }
            ClickTarget::WinCenter => state.center_focused_window(),
            ClickTarget::WinMinimize => state.minimize_focused_window(),
            ClickTarget::WinMaximize => {
                if let Some(win) = state.focused_window() {
                    state.toggle_maximize_window(&win);
                }
            }
            ClickTarget::WinPin => state.toggle_always_on_top_focused(),
            ClickTarget::WinNextWs => {
                if let Some(win) = state.focused_window() {
                    if let Some(id) = win.toplevel().map(|t| t.wl_surface().id()) {
                        let cur = state.workspaces.window_workspace(&id);
                        let next_ws = if cur >= state.workspaces.count { 1 } else { cur + 1 };
                        state.move_focused_to_workspace(next_ws);
                        state.overlays.show_osd("Moved Window", &format!("Workspace {}", next_ws), None);
                    }
                }
            }
            ClickTarget::WinClose => state.close_focused_window(),
            ClickTarget::PinnedApp(item) => {
                let bounce_key = format!("pin_{}", item.badge());
                state.animations.trigger_dock_bounce(&bounce_key);
                execute_launcher_item(state, item);
            }
            ClickTarget::Workspace(ws) => {
                state.panel.launcher_open = false;
                state.switch_workspace(ws);
            }
            ClickTarget::FocusWindow(win_id) => {
                state.panel.launcher_open = false;
                let found = state
                    .space
                    .elements()
                    .find(|w| {
                        w.toplevel()
                            .map(|t| t.wl_surface().id() == win_id)
                            .unwrap_or(false)
                    })
                    .cloned();
                if let Some(win) = found {
                    let win_ws = state
                        .workspaces
                        .window_map
                        .get(&win_id)
                        .copied()
                        .unwrap_or(state.workspaces.active);
                    if win_ws != state.workspaces.active {
                        state.switch_workspace(win_ws);
                    }
                    let is_focused = state.focused_window().as_ref() == Some(&win);
                    let is_min = state.workspaces.is_minimized(&win);
                    if is_min || is_focused {
                        state.toggle_minimize_window(&win);
                    } else {
                        state.focus_window(&win);
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
            ClickTarget::ApplyPinAction(pin_act) => {
                state.panel.pin_context_menu = None;
                match pin_act {
                    PinContextAction::PinApp(app) => {
                        if !state.panel.custom_pinned.iter().any(|p| p.exec == app.exec) {
                            let app_name = app.name.clone();
                            state.panel.custom_pinned.push(app);
                            state.panel.save_full_config(
                                state.pointer_sensitivity,
                                state.natural_scroll,
                                state.animations.enabled,
                                state.animations.speed,
                                state.animations.minimize_effect,
                                &state.settings.xkb_layout,
                            );
                            state.overlays.push_notification("Dock", format!("Pinned '{}' to Dock", app_name));
                        }
                    }
                    PinContextAction::UnpinBuiltin(item) => {
                        if !state.panel.hidden_builtins.contains(&item) {
                            state.panel.hidden_builtins.push(item);
                            state.overlays.push_notification("Dock", format!("Unpinned '{}' from Dock", item.label()));
                        }
                    }
                    PinContextAction::UnpinCustom(exec) => {
                        state.panel.custom_pinned.retain(|p| p.exec != exec);
                        state.panel.save_full_config(
                            state.pointer_sensitivity,
                            state.natural_scroll,
                            state.animations.enabled,
                            state.animations.speed,
                            state.animations.minimize_effect,
                            &state.settings.xkb_layout,
                        );
                        state.overlays.push_notification("Dock", "Unpinned app from Dock");
                    }
                }
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
            ClickTarget::PowerShutdown => {
                state.panel.launcher_open = false;
                let _ = Command::new("systemctl").arg("poweroff").spawn();
            }
            ClickTarget::PowerExit => {
                state.running = false;
                state.loop_signal.stop();
            }
            ClickTarget::OpenAdShieldSettings => {
                state.panel.launcher_open = false;
                state.panel.tray_open = false;
                state.overlays.calendar_open = false;
                state.overlays.notif_drawer_open = false;
                state.overlays.control_center_open = false;
                state.settings.open = true;
                state.settings.active_tab = crate::settings::SettingsTab::AdShield;
            }
        }
        return true;
    }

    if state.panel.top_bar_menu.take().is_some() {
        return true;
    }

    if state.panel.pin_context_menu.take().is_some() {
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
        state.panel.right_hit_regions.clear();
    }

    let accent = state.panel.accent_rgb();
    let pointer_x = state.pointer_location.x.round() as i32;
    let pointer_y = state.pointer_location.y.round() as i32;

    let focused_id = state
        .seat
        .get_keyboard()
        .and_then(|k| k.current_focus())
        .map(|s| s.id());

    let mut ws_windows: Vec<(Option<ObjectId>, String, bool, bool)> = state
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
    ws_windows.sort_by_key(|(id, _, _, _)| id.as_ref().map(|i| i.protocol_id()).unwrap_or(0));

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

        let pal = active_m3_palette();
        match pal.appearance_mode {
            AppearanceMode::GlassBlur => {
                // Transparent canvas over Dual-Kawase blur (which provides 0.48 tint + 1px bright edge)
                mb.fill_rect(0, 0, mb_w, mb_h, [0, 0, 0, 0]);
                mb.fill_rect_blend(0, mb_h - 1, mb_w, 1, [255, 255, 255, 48]);
            }
            AppearanceMode::Material3Expressive => {
                // Opaque/tonal surfaceContainer + clear outlineVariant bottom edge
                mb.fill_rect(
                    0,
                    0,
                    mb_w,
                    mb_h,
                    [pal.surface_container[0], pal.surface_container[1], pal.surface_container[2], 250],
                );
                mb.fill_rect_blend(
                    0,
                    mb_h - 1,
                    mb_w,
                    1,
                    [pal.outline_variant[0], pal.outline_variant[1], pal.outline_variant[2], 205],
                );
            }
        }

        let mut mx = 12;

        // Crisp `OKay` pill button (toggles Spotlight / Launcher)
        let logo_w = 56;
        let logo_h = mb_h - 6;
        let logo_r = logo_h / 2;
        let logo_global = Rectangle::new(
            (output_geo.loc.x + mx, output_geo.loc.y + 3).into(),
            (logo_w, logo_h).into(),
        );
        let logo_bg = if state.panel.launcher_open {
            [accent[0], accent[1], accent[2], 215]
        } else if rect_contains(logo_global, pointer_x, pointer_y) {
            [255, 255, 255, 38]
        } else {
            [255, 255, 255, 18]
        };
        mb.fill_rounded_rect_aa(mx, 3, logo_w, logo_h, logo_r, logo_bg);
        let logo_fg = if state.panel.launcher_open {
            contrast_fg(accent)
        } else {
            [246, 248, 255, 255]
        };
        let lw = mb.measure_text_px("OKay", 12.5);
        mb.draw_text_bold_px(mx + (logo_w - lw) / 2, 7, "OKay", 12.5, logo_fg);
        state
            .panel
            .hit_regions
            .push((logo_global, ClickTarget::ToggleLauncher));
        mx += logo_w + 10;

        // Focused App Name (bold-style bright white)
        let app_w = (mb.measure_text_px(&focused_app_name, 13.5) + 12).clamp(64, 180);
        mb.draw_text_px(mx, 6, &focused_app_name, 13.5, [255, 255, 255, 255]);
        mx += app_w + 8;

        // Clean macOS Menu Items: File, View, Window, Help (real interactive dropdown menus)
        let menu_items: [(&str, TopBarMenu); 4] = [
            ("File", TopBarMenu::File),
            ("View", TopBarMenu::View),
            ("Window", TopBarMenu::Window),
            ("Help", TopBarMenu::Help),
        ];
        for (label, menu_kind) in menu_items {
            let iw = mb.measure_text_px(label, 12.5) + 14;
            if mx + iw > mb_w - 380 {
                break;
            }
            let item_rect = Rectangle::new(
                (output_geo.loc.x + mx, output_geo.loc.y + 2).into(),
                (iw, mb_h - 4).into(),
            );
            let is_open = state.panel.top_bar_menu.map(|(m, _)| m) == Some(menu_kind);
            if is_open {
                mb.fill_rounded_rect_aa(mx, 3, iw, mb_h - 6, 6, [accent[0], accent[1], accent[2], 68]);
            } else if rect_contains(item_rect, pointer_x, pointer_y) {
                mb.fill_rounded_rect_aa(mx, 3, iw, mb_h - 6, 6, [255, 255, 255, 26]);
            }
            mb.draw_text_px(mx + 7, 7, label, 12.5, [224, 228, 240, 245]);
            state
                .panel
                .hit_regions
                .push((item_rect, ClickTarget::OpenTopMenu(menu_kind, mx)));
            mx += iw + 2;
        }

        // Material 3 Expressive Workspace pills & dots in menu bar (1..9) if enabled
        if state.panel.show_workspaces && mx + 200 < mb_w - 300 {
            mx += 10;
            let pill_h = mb_h - 8;
            let pill_r = pill_h / 2;
            let cy_ws = (mb_h as f32) * 0.5;
            for ws in 1..=9 {
                let is_active = state.workspaces.active == ws;
                let is_occ = occupied_workspaces[ws - 1];
                let ww = if is_active { 28 } else { 18 };
                let ws_rect = Rectangle::new(
                    (output_geo.loc.x + mx, output_geo.loc.y + 4).into(),
                    (ww, pill_h).into(),
                );
                let hovered = rect_contains(ws_rect, pointer_x, pointer_y);
                if is_active {
                    mb.fill_rounded_rect_aa(
                        mx,
                        4,
                        ww,
                        pill_h,
                        pill_r,
                        [accent[0], accent[1], accent[2], 220],
                    );
                    let lbl = ws.to_string();
                    let tw = mb.measure_text_px(&lbl, 11.5);
                    mb.draw_text_bold_px(mx + (ww - tw) / 2, 7, &lbl, 11.5, contrast_fg(accent));
                } else {
                    if hovered {
                        mb.fill_rounded_rect_aa(mx, 4, ww, pill_h, pill_r, [255, 255, 255, 28]);
                    }
                    let dot_r = if hovered {
                        4.5
                    } else if is_occ {
                        3.8
                    } else {
                        2.4
                    };
                    let dot_col = if hovered {
                        [240, 244, 255, 240]
                    } else if is_occ {
                        [206, 214, 238, 220]
                    } else {
                        [134, 140, 166, 155]
                    };
                    mb.fill_circle_aa(mx as f32 + ww as f32 * 0.5, cy_ws, dot_r, dot_col);
                }
                state.panel.hit_regions.push((ws_rect, ClickTarget::Workspace(ws)));
                mx += ww + 3;
            }
        }

        // Right-side macOS Status Pills & Live Clock (`Day DD Mon HH:MM`)
        let clock_str = current_macos_clock();
        let clock_w = (mb.measure_text_px(&clock_str, 12.0) + 20).max(120);
        let clock_x = (mb_w - clock_w - 10).max(mx + 8);
        let clock_global = Rectangle::new(
            (output_geo.loc.x + clock_x, output_geo.loc.y + 3).into(),
            (clock_w, mb_h - 6).into(),
        );
        let clock_bg = if state.overlays.calendar_open {
            [accent[0], accent[1], accent[2], 74]
        } else if rect_contains(clock_global, pointer_x, pointer_y) {
            [255, 255, 255, 34]
        } else {
            [255, 255, 255, 16]
        };
        mb.fill_rounded_rect_aa(clock_x, 4, clock_w, mb_h - 8, 6, clock_bg);
        mb.draw_text_px(clock_x + 10, 7, &clock_str, 12.0, [244, 246, 255, 255]);
        state
            .panel
            .hit_regions
            .push((clock_global, ClickTarget::ToggleCalendar));

        // Notification Bell (vector glyph + unread accent badge)
        let notif_w = 28;
        let notif_x = clock_x - notif_w - 6;
        let notif_global = Rectangle::new(
            (output_geo.loc.x + notif_x, output_geo.loc.y + 3).into(),
            (notif_w, mb_h - 6).into(),
        );
        let notif_bg = if state.overlays.notif_drawer_open {
            [accent[0], accent[1], accent[2], 74]
        } else if rect_contains(notif_global, pointer_x, pointer_y) {
            [255, 255, 255, 34]
        } else {
            [255, 255, 255, 16]
        };
        mb.fill_rounded_rect_aa(notif_x, 4, notif_w, mb_h - 8, 6, notif_bg);
        crate::icons::draw_bell_glyph(
            &mut mb,
            (notif_x + notif_w / 2) as f32,
            (mb_h / 2) as f32,
            0.95,
            [234, 238, 250, 250],
        );
        if !state.overlays.notifications.is_empty() || !state.overlays.history.is_empty() {
            mb.fill_circle_aa(
                (notif_x + notif_w - 6) as f32,
                8.0,
                2.5,
                [accent[0], accent[1], accent[2], 255],
            );
        }
        state
            .panel
            .hit_regions
            .push((notif_global, ClickTarget::ToggleNotifDrawer));

        // Control Center Icon (vector toggle switches with knobs)
        let cc_w = 30;
        let cc_x = notif_x - cc_w - 6;
        let cc_global = Rectangle::new(
            (output_geo.loc.x + cc_x, output_geo.loc.y + 3).into(),
            (cc_w, mb_h - 6).into(),
        );
        let cc_bg = if state.overlays.control_center_open {
            [accent[0], accent[1], accent[2], 74]
        } else if rect_contains(cc_global, pointer_x, pointer_y) {
            [255, 255, 255, 34]
        } else {
            [255, 255, 255, 16]
        };
        mb.fill_rounded_rect_aa(cc_x, 4, cc_w, mb_h - 8, 6, cc_bg);
        mb.fill_rounded_rect_aa(cc_x + 7, 9, 16, 4, 2, [226, 232, 248, 220]);
        mb.fill_circle_aa((cc_x + 19) as f32, 11.0, 2.5, [252, 253, 255, 255]);
        mb.fill_rounded_rect_aa(cc_x + 7, 16, 16, 4, 2, [178, 188, 218, 200]);
        mb.fill_circle_aa((cc_x + 11) as f32, 18.0, 2.5, [252, 253, 255, 255]);
        state
            .panel
            .hit_regions
            .push((cc_global, ClickTarget::ToggleControlCenter));

        // Volume / Battery status pill with vector speaker + battery icons
        let vol_txt = if state.panel.volume_muted {
            "Mute".to_string()
        } else {
            format!("{}%", state.panel.volume_percent)
        };
        let vol_txt_w = mb.measure_text_px(&vol_txt, 11.5);
        let bat_txt = state.panel.battery_percent.map(|b| format!("{}%", b));
        let bat_extra_w = bat_txt
            .as_ref()
            .map(|t| 24 + mb.measure_text_px(t, 11.5))
            .unwrap_or(0);
        let vol_w = 24 + vol_txt_w + bat_extra_w + 10;
        let vol_x = cc_x - vol_w - 6;
        let vol_global = Rectangle::new(
            (output_geo.loc.x + vol_x, output_geo.loc.y + 3).into(),
            (vol_w, mb_h - 6).into(),
        );
        let vol_bg = if rect_contains(vol_global, pointer_x, pointer_y) {
            [255, 255, 255, 34]
        } else {
            [255, 255, 255, 16]
        };
        mb.fill_rounded_rect_aa(vol_x, 4, vol_w, mb_h - 8, 6, vol_bg);
        crate::icons::draw_speaker_glyph(
            &mut mb,
            (vol_x + 7) as f32,
            (mb_h / 2) as f32,
            state.panel.volume_muted,
            0.9,
            [232, 236, 250, 250],
        );
        mb.draw_text_px(vol_x + 25, 7, &vol_txt, 11.5, [228, 232, 248, 248]);
        if let (Some(bat_pct), Some(b_str)) = (state.panel.battery_percent, bat_txt) {
            let bx = vol_x + 25 + vol_txt_w + 8;
            crate::icons::draw_battery_glyph(&mut mb, bx, mb_h / 2, bat_pct, [232, 236, 250, 240]);
            mb.draw_text_px(bx + 20, 7, &b_str, 11.5, [228, 232, 248, 248]);
        }
        state
            .panel
            .hit_regions
            .push((vol_global, ClickTarget::ToggleMute));

        // Background Apps / Tray Chevron Arrow (`^` / `v`)
        let tray_w = 28;
        let tray_x = vol_x - tray_w - 6;
        let tray_global = Rectangle::new(
            (output_geo.loc.x + tray_x, output_geo.loc.y + 3).into(),
            (tray_w, mb_h - 6).into(),
        );
        let tray_bg = if state.panel.tray_open {
            [accent[0], accent[1], accent[2], 78]
        } else if rect_contains(tray_global, pointer_x, pointer_y) {
            [255, 255, 255, 34]
        } else {
            [255, 255, 255, 16]
        };
        mb.fill_rounded_rect_aa(tray_x, 4, tray_w, mb_h - 8, 6, tray_bg);
        let acx = (tray_x + tray_w / 2) as f32;
        let acy = (mb_h / 2) as f32;
        let dy_off = if state.panel.tray_open { -2.2 } else { 2.2 };
        crate::icons::draw_thick_line(&mut mb, acx - 4.0, acy - dy_off, acx, acy + dy_off, 1.0, [234, 238, 250, 250]);
        crate::icons::draw_thick_line(&mut mb, acx, acy + dy_off, acx + 4.0, acy - dy_off, 1.0, [234, 238, 250, 250]);
        state
            .panel
            .hit_regions
            .push((tray_global, ClickTarget::ToggleTrayDrawer));

        // Compact OKay AdShield pill (shield icon + blocked counter)
        let shield_txt = if state.adshield.enabled {
            format!("{}", state.adshield.blocked_count)
        } else {
            "Off".to_string()
        };
        let shield_txt_w = mb.measure_text_px(&shield_txt, 11.5);
        let shield_w = 24 + shield_txt_w + 8;
        let shield_x = tray_x - shield_w - 6;
        let shield_global = Rectangle::new(
            (output_geo.loc.x + shield_x, output_geo.loc.y + 3).into(),
            (shield_w, mb_h - 6).into(),
        );
        let shield_bg = if state.adshield.enabled {
            if rect_contains(shield_global, pointer_x, pointer_y) {
                [accent[0], accent[1], accent[2], 96]
            } else {
                [accent[0], accent[1], accent[2], 54]
            }
        } else if rect_contains(shield_global, pointer_x, pointer_y) {
            [255, 255, 255, 34]
        } else {
            [255, 255, 255, 16]
        };
        mb.fill_rounded_rect_aa(shield_x, 4, shield_w, mb_h - 8, (mb_h - 8) / 2, shield_bg);
        let shield_col = if state.adshield.enabled {
            [140, 244, 180, 255]
        } else {
            [180, 186, 206, 220]
        };
        crate::icons::draw_shield_glyph(
            &mut mb,
            (shield_x + 12) as f32,
            (mb_h / 2) as f32,
            state.adshield.enabled,
            0.85,
            shield_col,
        );
        mb.draw_text_px(
            shield_x + 22,
            7,
            &shield_txt,
            11.5,
            [236, 240, 252, 250],
        );
        state
            .panel
            .hit_regions
            .push((shield_global, ClickTarget::OpenAdShieldSettings));

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
            if state.panel.appearance_mode == AppearanceMode::GlassBlur {
                if let Some(blur_el) = crate::render::panel_backdrop_element(
                    state,
                    renderer,
                    output,
                    scale,
                    "top_bar",
                    Rectangle::new((0, 0).into(), (mb_w, mb_h).into()),
                    0.0,
                    1.0,
                ) {
                    elements.push(blur_el);
                }
            }
        }

        // Background Apps / Tray Drawer Popover (from Top Bar Chevron Arrow)
        if state.panel.tray_open && is_primary {
            let mut bg_rows: Vec<(String, String, Option<ObjectId>)> = Vec::new();
            for win in state.space.elements() {
                if let Some(id) = win.toplevel().map(|t| t.wl_surface().id()) {
                    let title = window_title_str(win);
                    let status = if state.workspaces.is_minimized(win) {
                        "Minimized".to_string()
                    } else if state.workspaces.is_on_active_workspace(win) {
                        "Active".to_string()
                    } else {
                        format!("WS {}", state.workspaces.window_workspace(&id))
                    };
                    bg_rows.push((title, status, Some(id)));
                }
            }
            for cmd in &state.panel.autostart_cmds {
                let short_cmd = cmd.split_whitespace().next().unwrap_or(cmd).to_string();
                bg_rows.push((short_cmd, "Autostart".to_string(), None));
            }

            let tw = 288;
            let visible_rows = bg_rows.len().clamp(1, 7);
            let th = 50 + visible_rows as i32 * 36;
            let tx = (tray_x + tray_w / 2 - tw / 2).clamp(8, (output_geo.size.w - tw - 8).max(8));
            let ty = PANEL_HEIGHT + 6;
            let mut tcan = Canvas::new(tw, th);
            if !state.animations.enabled || state.animations.tray_open_anim >= 0.995 {
                tcan.fill_frosted_backdrop_panel(0, 0, tw, th, 16);
            } else {
                tcan.fill_glass_panel(0, 0, tw, th, 16);
            }
            tcan.draw_text_px(14, 12, "Background & Running Apps", 13.5, [244, 246, 255, 255]);

            if bg_rows.is_empty() {
                tcan.draw_text_px(14, 46, "No active background apps.", 12.0, [164, 170, 198, 235]);
            } else {
                for (ri, (name, status, id_opt)) in bg_rows.into_iter().take(7).enumerate() {
                    let ry = 42 + ri as i32 * 36;
                    let row_rect = Rectangle::new(
                        (output_geo.loc.x + tx + 10, output_geo.loc.y + ty + ry).into(),
                        (tw - 50, 30).into(),
                    );
                    let hov = rect_contains(row_rect, pointer_x, pointer_y);
                    tcan.fill_rounded_rect_aa(
                        10,
                        ry,
                        tw - 20,
                        30,
                        8,
                        if hov { [255, 255, 255, 24] } else { [255, 255, 255, 12] },
                    );
                    let short_name: String = name.chars().take(20).collect();
                    tcan.draw_text_px(18, ry + 8, &short_name, 12.0, [238, 240, 252, 255]);
                    let sw = tcan.measure_text_px(&status, 10.5);
                    tcan.draw_text_px(
                        tw - 46 - sw,
                        ry + 9,
                        &status,
                        10.5,
                        [accent[0], accent[1], accent[2], 235],
                    );

                    if let Some(id) = id_opt {
                        state.panel.hit_regions.push((row_rect, ClickTarget::FocusWindow(id.clone())));
                        let close_rect = Rectangle::new(
                            (output_geo.loc.x + tx + tw - 38, output_geo.loc.y + ty + ry + 3).into(),
                            (24, 24).into(),
                        );
                        let chov = rect_contains(close_rect, pointer_x, pointer_y);
                        tcan.fill_rounded_rect_aa(
                            tw - 38,
                            ry + 3,
                            24,
                            24,
                            6,
                            if chov { [225, 76, 94, 230] } else { [255, 255, 255, 18] },
                        );
                        tcan.draw_text_px(tw - 30, ry + 7, "x", 11.5, [242, 244, 255, 255]);
                        state.panel.hit_regions.push((close_rect, ClickTarget::CloseWindowId(id)));
                    }
                }
            }

            let anim_t = state.animations.tray_open_anim;
            if state.animations.enabled && anim_t < 0.995 {
                let anchor_local = (tray_x + tray_w / 2 - tx).clamp(24, tw - 24);
                if let Some(elem) = crate::animation::render_topbar_genie_canvas(
                    &tcan.pixels,
                    tw,
                    th,
                    Point::from((tx, ty)),
                    anchor_local,
                    anim_t,
                    renderer,
                    scale,
                ) {
                    elements.insert(0, elem);
                }
            } else {
                let tbuf = MemoryRenderBuffer::from_slice(
                    &tcan.pixels,
                    Fourcc::Abgr8888,
                    (tw, th),
                    1,
                    Transform::Normal,
                    None,
                );
                let tphys: Point<f64, Physical> =
                    Point::<i32, Logical>::from((tx, ty)).to_f64().to_physical(scale);
                if let Ok(elem) = MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    tphys,
                    &tbuf,
                    None,
                    None,
                    Some((tw, th).into()),
                    Kind::Unspecified,
                ) {
                    elements.insert(0, OkayRenderElements::Memory(elem));
                    if let Some(sh) = crate::render::panel_backdrop_element(
                        state,
                        renderer,
                        output,
                        scale,
                        "tray_drawer",
                        Rectangle::new((tx, ty).into(), (tw, th).into()),
                        16.0,
                        1.0,
                    ) {
                        elements.insert(1, sh);
                    }
                }
            }
        }

        // ====================================================================
        // B. Bottom Floating Dock (Auto-hides when any window is maximized)
        // ====================================================================
        let pinned_apps: Vec<(LauncherItem, &str, crate::icons::VectorIconKind)> = [
            (LauncherItem::FileManager, "FM", crate::icons::VectorIconKind::Files),
            (LauncherItem::Terminal, ">_", crate::icons::VectorIconKind::Terminal),
            (LauncherItem::Browser, "WB", crate::icons::VectorIconKind::Browser),
            (LauncherItem::OkaySettings, "OS", crate::icons::VectorIconKind::Settings),
            (LauncherItem::SystemMonitor, "SM", crate::icons::VectorIconKind::SystemMonitor),
        ]
        .into_iter()
        .filter(|(item, _, _)| !state.panel.hidden_builtins.contains(item))
        .collect();

        let custom_pinned = state.panel.custom_pinned.clone();
        let base_icon = 42;
        let icon_gap = 10;
        let divider_w = if ws_windows.is_empty() { 0 } else { 18 };
        let dock_pad_x = 16;
        let total_icons = pinned_apps.len() + custom_pinned.len() + ws_windows.len();

        let min_dock_w = 120.min((output_geo.size.w - 32).max(120));
        let max_dock_w = (output_geo.size.w - 32).max(min_dock_w);
        let base_pill_w = (dock_pad_x * 2
            + total_icons as i32 * base_icon
            + (total_icons as i32 - 1).max(0) * icon_gap
            + divider_w)
            .clamp(min_dock_w, max_dock_w);
        let dock_headroom = 26;
        let base_canvas_x_local = (output_geo.size.w - (base_pill_w + 40)) / 2;
        let dock_pill_y_local = output_geo.size.h - DOCK_HEIGHT - DOCK_MARGIN;
        let dock_canvas_y_local = dock_pill_y_local - dock_headroom;

        let dock_zone_y = output_geo.loc.y + dock_pill_y_local - 24;
        let pointer_near_dock = pointer_y >= dock_zone_y
            && pointer_x >= output_geo.loc.x + base_canvas_x_local - 32
            && pointer_x <= output_geo.loc.x + base_canvas_x_local + base_pill_w + 72;

        let has_maximized = state
            .space
            .elements()
            .any(|w| state.workspaces.is_visible(w) && state.workspaces.is_maximized(w));
        if has_maximized {
            if pointer_y >= output_geo.loc.y + output_geo.size.h - 4 {
                state.panel.dock_auto_revealed = true;
            } else if !pointer_near_dock
                && !state.panel.launcher_open
                && state.panel.pin_context_menu.is_none()
            {
                state.panel.dock_auto_revealed = false;
            }
        } else {
            state.panel.dock_auto_revealed = false;
        }
        let dock_visible = !has_maximized || state.panel.dock_auto_revealed;

        if dock_visible {

        let compute_zoom = |rest_idx: usize, extra_x: i32| -> i32 {
            if !pointer_near_dock {
                return base_icon;
            }
            let rest_x = 20 + dock_pad_x + rest_idx as i32 * (base_icon + icon_gap) + extra_x;
            let center_global_x = output_geo.loc.x + base_canvas_x_local + rest_x + base_icon / 2;
            let dist = ((pointer_x - center_global_x) as f32).abs();
            let zoom = if dist < 96.0 {
                let norm = 1.0 - dist / 96.0;
                1.0 + 0.38 * norm * norm * (3.0 - 2.0 * norm)
            } else {
                1.0
            };
            (base_icon as f32 * zoom).round() as i32
        };

        let mut pinned_sizes = Vec::with_capacity(pinned_apps.len());
        for i in 0..pinned_apps.len() {
            pinned_sizes.push(compute_zoom(i, 0));
        }
        let mut custom_sizes = Vec::with_capacity(custom_pinned.len());
        for i in 0..custom_pinned.len() {
            custom_sizes.push(compute_zoom(pinned_apps.len() + i, 0));
        }
        let mut win_sizes = Vec::with_capacity(ws_windows.len());
        for i in 0..ws_windows.len() {
            win_sizes.push(compute_zoom(pinned_apps.len() + custom_pinned.len() + i, divider_w));
        }

        let sum_sizes: i32 = pinned_sizes.iter().sum::<i32>()
            + custom_sizes.iter().sum::<i32>()
            + win_sizes.iter().sum::<i32>();
        let min_dock_w = 120.min((output_geo.size.w - 32).max(120));
        let max_dock_w = (output_geo.size.w - 32).max(min_dock_w);
        let dock_pill_w = (dock_pad_x * 2
            + sum_sizes
            + (total_icons as i32 - 1).max(0) * icon_gap
            + divider_w)
            .clamp(min_dock_w, max_dock_w);

        // Extra canvas headroom above the pill so parabolic hover magnification & bounce never clip
        let dock_canvas_w = dock_pill_w + 40;
        let dock_canvas_h = DOCK_HEIGHT + dock_headroom;
        let dock_pill_x_in_canvas = 20;
        let dock_pill_y_in_canvas = dock_headroom;
        let dock_canvas_x_local = (output_geo.size.w - dock_canvas_w) / 2;

        if is_primary {
            state.animations.dock_center_rect = Some(Rectangle::new(
                (
                    output_geo.loc.x + dock_canvas_x_local + dock_canvas_w / 2 - 21,
                    output_geo.loc.y + dock_pill_y_local + 8,
                )
                    .into(),
                (42, 42).into(),
            ));
        }

        let mut dock = Canvas::new(dock_canvas_w, dock_canvas_h);
        dock.fill_frosted_backdrop_panel(
            dock_pill_x_in_canvas,
            dock_pill_y_in_canvas,
            dock_pill_w,
            DOCK_HEIGHT,
            18,
        );

        let mut dx = dock_pill_x_in_canvas + dock_pad_x;
        let mut dock_tooltip: Option<(i32, i32, String)> = None;

        for (i, (item, glyph, kind)) in pinned_apps.into_iter().enumerate() {
            let sz = pinned_sizes[i];
            let bounce_key = format!("pin_{}", glyph);
            let bounce_y = state.animations.dock_bounce_offset(&bounce_key);
            let draw_x = dx;
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
                    (dx + sz / 2) as f32,
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
            if rect_contains(hit_global, pointer_x, pointer_y) {
                dock_tooltip = Some((
                    dock_canvas_x_local + draw_x + sz / 2,
                    dock_canvas_y_local + draw_y - 32,
                    item.label().to_string(),
                ));
            }
            state
                .panel
                .hit_regions
                .push((hit_global, ClickTarget::PinnedApp(item)));
            state
                .panel
                .right_hit_regions
                .push((hit_global, PinContextAction::UnpinBuiltin(item)));

            dx += sz + icon_gap;
        }

        // User-pinned custom apps from Desktop Icon Context Menu
        for (i, app) in custom_pinned.iter().enumerate() {
            let sz = custom_sizes[i];
            let draw_x = dx;
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
            if rect_contains(hit_global, pointer_x, pointer_y) {
                dock_tooltip = Some((
                    dock_canvas_x_local + draw_x + sz / 2,
                    dock_canvas_y_local + draw_y - 32,
                    app.name.clone(),
                ));
            }
            state
                .panel
                .hit_regions
                .push((hit_global, ClickTarget::LaunchDesktopApp(app.exec.clone())));
            state
                .panel
                .right_hit_regions
                .push((hit_global, PinContextAction::UnpinCustom(app.exec.clone())));
            dx += sz + icon_gap;
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
                let sz = win_sizes[idx];
                let draw_x = dx;
                let draw_y = dock_pill_y_in_canvas + 8 - (sz - base_icon);

                // Always record resting dock icon rect so minimize/unminimize animations
                // land directly on this app's Dock icon regardless of window position.
                let resting_dock_rect = Rectangle::new(
                    (
                        output_geo.loc.x + dock_canvas_x_local + draw_x,
                        output_geo.loc.y + dock_pill_y_local + 8,
                    )
                        .into(),
                    (sz, base_icon).into(),
                );
                if let Some(id) = win_id {
                    state
                        .animations
                        .taskbar_button_rects
                        .insert(id.clone(), resting_dock_rect);
                }

                if dx + sz > dock_pill_x_in_canvas + dock_pill_w - 8 {
                    continue;
                }

                let win_global = Rectangle::new(
                    (
                        output_geo.loc.x + dock_canvas_x_local + draw_x,
                        output_geo.loc.y + dock_canvas_y_local + draw_y,
                    )
                        .into(),
                    (sz, sz + 10).into(),
                );
                if rect_contains(win_global, pointer_x, pointer_y) {
                    let tip = if *is_min {
                        format!("{} (Minimized)", title)
                    } else {
                        title.clone()
                    };
                    dock_tooltip = Some((
                        dock_canvas_x_local + draw_x + sz / 2,
                        dock_canvas_y_local + draw_y - 32,
                        tip,
                    ));
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
                    (dx + sz / 2) as f32,
                    (dock_pill_y_in_canvas + DOCK_HEIGHT - 6) as f32,
                    if *is_focused { 2.8 } else { 2.2 },
                    dot_col,
                );

                if let Some(id) = win_id {
                    state
                        .panel
                        .hit_regions
                        .push((win_global, ClickTarget::FocusWindow(id.clone())));
                }
                dx += sz + icon_gap;
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
            let dock_pill_x_local = dock_canvas_x_local + dock_pill_x_in_canvas;
            if let Some(bd_el) = crate::render::panel_backdrop_element(
                state,
                renderer,
                output,
                scale,
                "dock",
                Rectangle::new(
                    (dock_pill_x_local, dock_pill_y_local).into(),
                    (dock_pill_w, DOCK_HEIGHT).into(),
                ),
                18.0,
                1.0,
            ) {
                elements.push(bd_el);
            }
        }
        if let Some((tip_cx, tip_y, tip_label)) = dock_tooltip {
            let short_tip: String = tip_label.chars().take(28).collect();
            let tmp = Canvas::new(1, 1);
            let tw = (tmp.measure_text_px(&short_tip, 11.5) + 22).clamp(56, 240);
            let th = 26;
            let tx = (tip_cx - tw / 2).clamp(8, (output_geo.size.w - tw - 8).max(8));
            let ty = tip_y.max(PANEL_HEIGHT + 8);
            let mut tip_canvas = Canvas::new(tw, th);
            tip_canvas.fill_glass_panel(0, 0, tw, th, 8);
            let lw = tip_canvas.measure_text_px(&short_tip, 11.5);
            tip_canvas.draw_text_px((tw - lw) / 2, 6, &short_tip, 11.5, [244, 246, 255, 255]);
            let tip_buf = MemoryRenderBuffer::from_slice(
                &tip_canvas.pixels,
                Fourcc::Abgr8888,
                (tw, th),
                1,
                Transform::Normal,
                None,
            );
            let tip_phys: Point<f64, Physical> =
                Point::<i32, Logical>::from((tx, ty)).to_f64().to_physical(scale);
            if let Ok(elem) = MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                tip_phys,
                &tip_buf,
                None,
                None,
                Some((tw, th).into()),
                Kind::Unspecified,
            ) {
                elements.push(OkayRenderElements::Memory(elem));
            }
        }
        }

        // Top Bar Dropdown Menus (`File`, `View`, `Window`, `Help`)
        if let Some((menu_kind, anchor_x)) = state.panel.top_bar_menu {
            if is_primary {
                let rows: Vec<(&str, &str, ClickTarget, bool)> = match menu_kind {
                    TopBarMenu::File => vec![
                        ("New Terminal", "Super+Ret", ClickTarget::PinnedApp(LauncherItem::Terminal), false),
                        ("File Manager", "", ClickTarget::PinnedApp(LauncherItem::FileManager), true),
                        ("Take Screenshot", "Super+S", ClickTarget::PinnedApp(LauncherItem::Screenshot), false),
                        ("Area Screenshot", "Super+Sh+S", ClickTarget::AreaScreenshot, true),
                        ("Reload Config", "", ClickTarget::PinnedApp(LauncherItem::ReloadConfig), false),
                        ("Lock Screen", "Super+L", ClickTarget::PowerLock, false),
                    ],
                    TopBarMenu::View => vec![
                        ("Window Overview", "Super+O", ClickTarget::ToggleOverview, false),
                        ("Cycle Tiling Layout", "Super+T", ClickTarget::ToggleTiling, true),
                        ("Toggle Desktop Icons", "", ClickTarget::ToggleDesktopIcons, false),
                        ("Reload Wallpaper", "", ClickTarget::ReloadWallpaper, false),
                    ],
                    TopBarMenu::Window => vec![
                        ("Minimize Focused", "Super+M", ClickTarget::WinMinimize, false),
                        ("Maximize / Restore", "", ClickTarget::WinMaximize, false),
                        ("Toggle Always on Top", "Super+A", ClickTarget::WinPin, true),
                        ("Center Floating Window", "Super+C", ClickTarget::WinCenter, false),
                        ("Move to Next Workspace", "", ClickTarget::WinNextWs, true),
                        ("Close Window", "Super+Q", ClickTarget::WinClose, false),
                    ],
                    TopBarMenu::Help => vec![
                        ("OKay System Settings", "Super+I", ClickTarget::ToggleSettings, false),
                        ("Keyboard Shortcuts", "", ClickTarget::OpenSettingsTab(crate::settings::SettingsTab::Shortcuts), false),
                    ],
                };
                let mw = 286;
                let seps = rows.iter().filter(|(_, _, _, s)| *s).count() as i32;
                let mh = 20 + rows.len() as i32 * 35 + seps * 10;
                let mx = anchor_x.clamp(8, (output_geo.size.w - mw - 8).max(8));
                let my = PANEL_HEIGHT + 8;
                let mut mcan = Canvas::new(mw, mh);
                if !state.animations.enabled || state.animations.top_menu_open_anim >= 0.995 {
                    mcan.fill_frosted_backdrop_panel(0, 0, mw, mh, 16);
                } else {
                    mcan.fill_glass_panel(0, 0, mw, mh, 16);
                }

                let mut iy = 10;
                for (lbl, hint, act, sep_after) in rows {
                    let r_rect = Rectangle::new(
                        (output_geo.loc.x + mx + 10, output_geo.loc.y + my + iy).into(),
                        (mw - 20, 32).into(),
                    );
                    let hov = rect_contains(r_rect, pointer_x, pointer_y);
                    if hov {
                        mcan.fill_rounded_rect_aa(10, iy, mw - 20, 32, 9, [accent[0], accent[1], accent[2], 225]);
                    }
                    let fg = if hov { contrast_fg(accent) } else { [240, 242, 254, 255] };
                    let hfg = if hov { fg } else { [156, 164, 192, 230] };
                    mcan.draw_text_px(20, iy + 9, lbl, 12.5, fg);
                    if !hint.is_empty() {
                        let hw = mcan.measure_text_px(hint, 11.0);
                        mcan.draw_text_px(mw - 20 - hw, iy + 10, hint, 11.0, hfg);
                    }
                    state.panel.hit_regions.push((r_rect, act));
                    iy += 35;
                    if sep_after {
                        mcan.fill_rect_blend(16, iy + 4, mw - 32, 1, [255, 255, 255, 24]);
                        iy += 10;
                    }
                }

                let anim_t = state.animations.top_menu_open_anim;
                if state.animations.enabled && anim_t < 0.995 {
                    let anchor_local = (anchor_x + 24 - mx).clamp(24, mw - 24);
                    if let Some(elem) = crate::animation::render_topbar_genie_canvas(
                        &mcan.pixels,
                        mw,
                        mh,
                        Point::from((mx, my)),
                        anchor_local,
                        anim_t,
                        renderer,
                        scale,
                    ) {
                        elements.insert(0, elem);
                    }
                } else {
                    let mbuf = MemoryRenderBuffer::from_slice(
                        &mcan.pixels,
                        Fourcc::Abgr8888,
                        (mw, mh),
                        1,
                        Transform::Normal,
                        None,
                    );
                    let mphys: Point<f64, Physical> =
                        Point::<i32, Logical>::from((mx, my)).to_f64().to_physical(scale);
                    if let Ok(elem) = MemoryRenderBufferRenderElement::from_buffer(
                        renderer,
                        mphys,
                        &mbuf,
                        None,
                        None,
                        Some((mw, mh).into()),
                        Kind::Unspecified,
                    ) {
                        elements.insert(0, OkayRenderElements::Memory(elem));
                        if let Some(sh) = crate::render::panel_backdrop_element(
                            state,
                            renderer,
                            output,
                            scale,
                            "top_menu",
                            Rectangle::new((mx, my).into(), (mw, mh).into()),
                            16.0,
                            1.0,
                        ) {
                            elements.insert(1, sh);
                        }
                    }
                }
            }
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
                if let Some(id) = win_id {
                    state
                        .panel
                        .hit_regions
                        .push((win_global, ClickTarget::FocusWindow(id.clone())));
                }
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
            if state.panel.appearance_mode == AppearanceMode::GlassBlur {
                if let Some(blur_el) = crate::render::panel_backdrop_element(
                    state,
                    renderer,
                    output,
                    scale,
                    "top_bar",
                    Rectangle::new((0, bar_y_local).into(), (bar_w, bar_h).into()),
                    0.0,
                    1.0,
                ) {
                    elements.push(blur_el);
                }
            }
        }
    }

    // 6. Render Expanded COSMIC / macOS Launchpad-style App Grid Menu
    let menu_anim = state.animations.launcher_open_anim;
    if (state.panel.launcher_open || menu_anim > 0.02) && is_primary {
        let query_lower = state.panel.search_query.to_lowercase();

        enum GridEntry {
            Builtin(LauncherItem),
            Desktop(DesktopAppEntry),
        }

        let mut grid_items: Vec<GridEntry> = Vec::new();
        for item in LauncherItem::BUILTIN {
            if query_lower.is_empty() || item.label().to_lowercase().contains(&query_lower) {
                grid_items.push(GridEntry::Builtin(item));
            }
        }
        for app in &state.panel.desktop_apps {
            if query_lower.is_empty() || app.name.to_lowercase().contains(&query_lower) {
                grid_items.push(GridEntry::Desktop(app.clone()));
            }
            if grid_items.len() >= 15 {
                break;
            }
        }
        grid_items.truncate(15);

        let cols = 5i32;
        let cell_w = 126i32;
        let cell_h = 94i32;
        let cell_gap = 10i32;
        let rows = ((grid_items.len() as i32 + cols - 1) / cols).max(1);
        let grid_h = rows * cell_h + (rows - 1).max(0) * cell_gap;

        let menu_w = MENU_WIDTH.min((output_geo.size.w - 32).max(360));
        let menu_h = MENU_HEADER_HEIGHT + MENU_PADDING + grid_h + MENU_PADDING + MENU_FOOTER_HEIGHT;
        let menu_x = ((output_geo.size.w - menu_w) / 2).max(12);
        let slide_px = ((1.0 - menu_anim) * 18.0).round() as i32;
        let base_y = ((output_geo.size.h - menu_h) / 2).max(PANEL_HEIGHT + 14);
        let menu_y = base_y - slide_px;

        let mut menu_canvas = Canvas::new(menu_w, menu_h);
        if !state.animations.enabled || state.animations.launcher_open_anim >= 0.995 {
            menu_canvas.fill_frosted_backdrop_panel(0, 0, menu_w, menu_h, 18);
        } else {
            menu_canvas.fill_glass_panel(0, 0, menu_w, menu_h, 18);
        }

        // Header + Centered Search Bar
        menu_canvas.draw_text_px(20, 14, "Applications", 16.0, [246, 248, 255, 255]);
        let hint_badge = "Right-click app to Pin to Dock";
        let hbw = menu_canvas.measure_text_px(hint_badge, 11.0);
        menu_canvas.fill_rounded_rect_aa(
            menu_w - hbw - 34,
            13,
            hbw + 16,
            22,
            7,
            [255, 255, 255, 16],
        );
        menu_canvas.draw_text_px(
            menu_w - hbw - 26,
            17,
            hint_badge,
            11.0,
            [176, 184, 210, 235],
        );

        let search_w = menu_w - 40;
        menu_canvas.fill_rounded_rect_aa(20, 44, search_w, 34, 10, [255, 255, 255, 18]);
        if !state.panel.search_query.is_empty() {
            menu_canvas.stroke_rounded_rect_aa(
                20,
                44,
                search_w,
                34,
                10,
                [accent[0], accent[1], accent[2], 160],
            );
        }
        let search_txt = if state.panel.search_query.is_empty() {
            "Search applications & commands...".to_string()
        } else {
            format!("{}|", state.panel.search_query)
        };
        let search_fg = if state.panel.search_query.is_empty() {
            [148, 154, 180, 235]
        } else {
            [248, 250, 255, 255]
        };
        menu_canvas.draw_text_px(34, 53, &search_txt, 13.0, search_fg);

        // 5-Column App Grid
        let grid_total_w = cols * cell_w + (cols - 1) * cell_gap;
        let grid_x0 = ((menu_w - grid_total_w) / 2).max(MENU_PADDING);
        let grid_y0 = MENU_HEADER_HEIGHT + MENU_PADDING;

        for (idx, entry) in grid_items.into_iter().enumerate() {
            let c = idx as i32 % cols;
            let r = idx as i32 / cols;
            let cx = grid_x0 + c * (cell_w + cell_gap);
            let cy = grid_y0 + r * (cell_h + cell_gap);

            let cell_global = Rectangle::new(
                (output_geo.loc.x + menu_x + cx, output_geo.loc.y + menu_y + cy).into(),
                (cell_w, cell_h).into(),
            );
            let hovered = rect_contains(cell_global, pointer_x, pointer_y);
            let cell_bg = if hovered {
                [accent[0], accent[1], accent[2], 56]
            } else {
                [255, 255, 255, 12]
            };
            menu_canvas.fill_rounded_rect_aa(cx, cy, cell_w, cell_h, 12, cell_bg);
            if hovered {
                menu_canvas.stroke_rounded_rect_aa(
                    cx,
                    cy,
                    cell_w,
                    cell_h,
                    12,
                    [accent[0], accent[1], accent[2], 150],
                );
            }

            let icon_sz = 44;
            let ix = cx + (cell_w - icon_sz) / 2;
            let iy = cy + 12;

            match entry {
                GridEntry::Builtin(item) => {
                    let kind = match item {
                        LauncherItem::Terminal => crate::icons::VectorIconKind::Terminal,
                        LauncherItem::FileManager => crate::icons::VectorIconKind::Files,
                        LauncherItem::Browser => crate::icons::VectorIconKind::Browser,
                        LauncherItem::SystemMonitor => crate::icons::VectorIconKind::SystemMonitor,
                        LauncherItem::Screenshot => crate::icons::VectorIconKind::Screenshot,
                        LauncherItem::OkaySettings | LauncherItem::ReloadConfig => {
                            crate::icons::VectorIconKind::Settings
                        }
                    };
                    let sys_hint = match item {
                        LauncherItem::FileManager => Some(state.panel.default_file_manager.as_str()),
                        LauncherItem::Terminal => Some(state.panel.default_terminal.as_str()),
                        LauncherItem::Browser => Some(state.panel.default_browser.as_str()),
                        _ => None,
                    };
                    crate::icons::draw_app_icon(&mut menu_canvas, ix, iy, icon_sz, sys_hint, kind);
                    let lbl: String = item.label().chars().take(15).collect();
                    let lw = menu_canvas.measure_text_px(&lbl, 12.0);
                    menu_canvas.draw_text_px(
                        cx + (cell_w - lw) / 2,
                        cy + 64,
                        &lbl,
                        12.0,
                        [240, 242, 252, 255],
                    );
                    state
                        .panel
                        .hit_regions
                        .push((cell_global, ClickTarget::LaunchItem(item)));
                    if state.panel.hidden_builtins.contains(&item) {
                        // Allow re-pinning if unpinned? Keep UnpinBuiltin if visible
                    } else {
                        state
                            .panel
                            .right_hit_regions
                            .push((cell_global, PinContextAction::UnpinBuiltin(item)));
                    }
                }
                GridEntry::Desktop(app) => {
                    let kind = crate::icons::VectorIconKind::from_hint(&app.name, &app.exec, false);
                    crate::icons::draw_app_icon(
                        &mut menu_canvas,
                        ix,
                        iy,
                        icon_sz,
                        app.icon_name.as_deref(),
                        kind,
                    );
                    let lbl: String = app.name.chars().take(15).collect();
                    let lw = menu_canvas.measure_text_px(&lbl, 12.0);
                    menu_canvas.draw_text_px(
                        cx + (cell_w - lw) / 2,
                        cy + 64,
                        &lbl,
                        12.0,
                        [240, 242, 252, 255],
                    );
                    state
                        .panel
                        .hit_regions
                        .push((cell_global, ClickTarget::LaunchDesktopApp(app.exec.clone())));
                    let is_pinned = state.panel.custom_pinned.iter().any(|p| p.exec == app.exec);
                    let pin_act = if is_pinned {
                        PinContextAction::UnpinCustom(app.exec.clone())
                    } else {
                        PinContextAction::PinApp(app)
                    };
                    state.panel.right_hit_regions.push((cell_global, pin_act));
                }
            }
        }

        // Bottom Power & Session Bar (`Lock`, `Sleep`, `Reboot`, `Power Off`, `Exit DE`)
        let fy = menu_h - MENU_FOOTER_HEIGHT + 10;
        menu_canvas.fill_rect_blend(20, fy - 10, menu_w - 40, 1, [255, 255, 255, 24]);
        let power_btns: [(&str, ClickTarget, [u8; 4]); 5] = [
            ("Lock", ClickTarget::PowerLock, [255, 255, 255, 20]),
            ("Sleep", ClickTarget::PowerSuspend, [255, 255, 255, 20]),
            ("Reboot", ClickTarget::PowerReboot, [255, 255, 255, 20]),
            ("Power Off", ClickTarget::PowerShutdown, [215, 68, 86, 175]),
            ("Exit DE", ClickTarget::PowerExit, [255, 255, 255, 20]),
        ];
        let btn_w = 114i32;
        let btn_gap = 12i32;
        let total_btns_w = 5 * btn_w + 4 * btn_gap;
        let btn_x0 = ((menu_w - total_btns_w) / 2).max(16);

        for (pi, (plabel, ptarget, pbg)) in power_btns.into_iter().enumerate() {
            let px = btn_x0 + pi as i32 * (btn_w + btn_gap);
            let p_global = Rectangle::new(
                (output_geo.loc.x + menu_x + px, output_geo.loc.y + menu_y + fy).into(),
                (btn_w, 34).into(),
            );
            let hovered = rect_contains(p_global, pointer_x, pointer_y);
            let bg = if hovered {
                if matches!(ptarget, ClickTarget::PowerShutdown) {
                    [235, 82, 102, 225]
                } else {
                    [accent[0], accent[1], accent[2], 74]
                }
            } else {
                pbg
            };
            menu_canvas.fill_rounded_rect_aa(px, fy, btn_w, 34, 9, bg);
            let tw = menu_canvas.measure_text_px(plabel, 12.0);
            menu_canvas.draw_text_px(
                px + (btn_w - tw) / 2,
                fy + 10,
                plabel,
                12.0,
                [244, 246, 255, 255],
            );
            state.panel.hit_regions.push((p_global, ptarget));
        }

        let launcher_t = state.animations.launcher_open_anim;
        if state.animations.enabled && launcher_t < 0.995 {
            if let Some(elem) = crate::animation::render_launcher_genie_canvas(
                &menu_canvas.pixels,
                menu_w,
                menu_h,
                Point::from((menu_x, menu_y)),
                launcher_t,
                renderer,
                scale,
            ) {
                elements.insert(0, elem);
            }
        } else {
            let menu_buf = MemoryRenderBuffer::from_slice(
                &menu_canvas.pixels,
                Fourcc::Abgr8888,
                (menu_w, menu_h),
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
                Some((menu_w, menu_h).into()),
                Kind::Unspecified,
            ) {
                // Front-to-back rendering order: insert at index 0 so Launcher renders above Top Bar & Dock.
                elements.insert(0, OkayRenderElements::Memory(elem));
                if let Some(sh) = crate::render::panel_backdrop_element(
                    state,
                    renderer,
                    output,
                    scale,
                    "launcher",
                    Rectangle::new((menu_x, menu_y).into(), (menu_w, menu_h).into()),
                    18.0,
                    1.0,
                ) {
                    elements.insert(1, sh);
                }
            }
        }
    }

    // 7. Render Pin / Unpin Context Menu for Dock & Launcher Apps (ALWAYS topmost above Launcher & Dock)
    if let Some((pos, ref pin_action)) = state.panel.pin_context_menu.clone() {
        if is_primary {
            let cw = 168;
            let ch = 46;
            let cx = (pos.x - output_geo.loc.x).clamp(8, (output_geo.size.w - cw - 8).max(8));
            let cy = (pos.y - output_geo.loc.y - ch - 6)
                .clamp(PANEL_HEIGHT + 6, (output_geo.size.h - ch - 8).max(8));

            let mut ctx = Canvas::new(cw, ch);
            ctx.fill_glass_panel(0, 0, cw, ch, 16);
            let label = match pin_action {
                PinContextAction::PinApp(_) => "Pin to Dock",
                PinContextAction::UnpinBuiltin(_) | PinContextAction::UnpinCustom(_) => {
                    "Unpin from Dock"
                }
            };
            let btn_rect = Rectangle::new(
                (output_geo.loc.x + cx + 6, output_geo.loc.y + cy + 6).into(),
                (cw - 12, ch - 12).into(),
            );
            let hovered = rect_contains(btn_rect, pointer_x, pointer_y);
            let bg = if hovered {
                [accent[0], accent[1], accent[2], 225]
            } else {
                [255, 255, 255, 18]
            };
            let fg = if hovered {
                contrast_fg(accent)
            } else {
                [242, 244, 255, 255]
            };
            ctx.fill_rounded_rect_aa(6, 6, cw - 12, ch - 12, 7, bg);
            let lw = ctx.measure_text_px(label, 12.5);
            ctx.draw_text_px((cw - lw) / 2, 15, label, 12.5, fg);
            state
                .panel
                .hit_regions
                .push((btn_rect, ClickTarget::ApplyPinAction(pin_action.clone())));

            let ctx_buf = MemoryRenderBuffer::from_slice(
                &ctx.pixels,
                Fourcc::Abgr8888,
                (cw, ch),
                1,
                Transform::Normal,
                None,
            );
            let ctx_phys: Point<f64, Physical> =
                Point::<i32, Logical>::from((cx, cy)).to_f64().to_physical(scale);
            if let Ok(elem) = MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                ctx_phys,
                &ctx_buf,
                None,
                None,
                Some((cw, ch).into()),
                Kind::Unspecified,
            ) {
                elements.insert(0, OkayRenderElements::Memory(elem));
            }
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
    is_maximized: bool,
    accent: [u8; 3],
    stack_tabs: Option<(&[(smithay::reexports::wayland_server::backend::ObjectId, String)], usize)>,
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
    if is_maximized {
        // Sharp 0px corners when maximized
        canvas.fill_rect(0, 0, tb_w, tb_h, bg);
    } else {
        // SDF anti-aliased rounded top corners (18px radius), matching 18px 4-corner offscreen window clip
        canvas.fill_rounded_rect_aa(0, 0, tb_w, tb_h + 18, 18, bg);
        if tb_w > 36 {
            canvas.fill_rect_blend(18, 1, tb_w - 36, 1, [255, 255, 255, 36]);
        }
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

    if let Some((tabs, active_idx)) = stack_tabs.filter(|(t, _)| t.len() >= 2) {
        // Material 3 Expressive tabbed stack pills in titlebar
        let tabs_area_w = (tb_w - 110).max(40);
        let tab_w = (tabs_area_w / tabs.len() as i32).max(28);
        for (i, (_id, tab_title)) in tabs.iter().enumerate() {
            let tx = 74 + (i as i32) * tab_w;
            let tw = (tab_w - 4).max(24);
            let is_active_tab = i == active_idx;
            let pill_col = if is_active_tab {
                [accent[0], accent[1], accent[2], 235]
            } else {
                [255, 255, 255, 20]
            };
            let fg = if is_active_tab {
                contrast_fg(accent)
            } else {
                [206, 210, 224, 220]
            };
            canvas.fill_rounded_rect_aa(tx, 4, tw, tb_h - 8, 9, pill_col);
            let max_chars = ((tw - 14) / 7).max(1) as usize;
            let label: String = tab_title.chars().take(max_chars).collect();
            let lw = canvas.measure_text_px(&label, 12.0);
            let lx = tx + ((tw - lw) / 2).max(6);
            canvas.draw_text_px(lx, 7, &label, 12.0, fg);
        }
        // Unstack button on the right
        if tb_w > 110 {
            canvas.fill_rounded_rect_aa(tb_w - 30, 5, 22, tb_h - 10, 8, [255, 255, 255, 22]);
            canvas.draw_text_px(tb_w - 22, 6, "^", 12.0, [220, 224, 238, 230]);
        }
    } else {
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
                    d.lock()
                        .ok()
                        .and_then(|guard| guard.title.clone().or_else(|| guard.app_id.clone()))
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

// ============================================================================
// TrueType Font Engine (`fontdue`: Maple Mono -> JetBrainsMono -> NotoSans)
// ============================================================================

struct FontEngine {
    font_regular: Option<Font>,
    font_bold: Option<Font>,
    font_name: String,
    cache: HashMap<(char, u16, bool), (Metrics, Vec<u8>)>,
}

static FONT_ENGINE: OnceLock<Mutex<FontEngine>> = OnceLock::new();

fn with_font_engine<R>(f: impl FnOnce(&mut FontEngine) -> R) -> R {
    let lock = FONT_ENGINE.get_or_init(|| Mutex::new(FontEngine::load()));
    match lock.lock() {
        Ok(mut guard) => f(&mut guard),
        Err(poisoned) => f(&mut poisoned.into_inner()),
    }
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
            ("Liberation Sans", |name| name.contains("liberationsans")),
            ("Noto Sans", |name| {
                name.contains("notosans-")
                    || name.contains("notosansregular")
                    || name.contains("notosansbold")
                    || name.contains("dejavusans")
            }),
            ("JetBrains / Maple", |name| {
                name.contains("jetbrainsmono") || name.contains("maplemono")
            }),
            ("System TrueType", |name| name.ends_with(".ttf")),
        ];

        let mut font_regular: Option<Font> = None;
        let mut font_bold: Option<Font> = None;
        let mut font_name = "Built-in Sans".to_string();

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
            if candidates.is_empty() {
                continue;
            }

            // Pick Regular first
            candidates.sort_by_key(|p| {
                let s = p.file_name().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
                if s.contains("regular") {
                    0
                } else if !s.contains("italic") && !s.contains("bold") && !s.contains("light") {
                    1
                } else {
                    2
                }
            });

            if font_regular.is_none() {
                for path in &candidates {
                    if let Ok(bytes) = fs::read(path) {
                        if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                            font_name = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or(label)
                                .to_string();
                            tracing::info!(?path, font = %font_name, "Loaded Regular UI font");
                            font_regular = Some(font);
                            break;
                        }
                    }
                }
            }

            // Pick matching Bold weight
            if font_bold.is_none() {
                for path in &candidates {
                    let s = path
                        .file_name()
                        .and_then(|x| x.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    if s.contains("bold") && !s.contains("italic") {
                        if let Ok(bytes) = fs::read(path) {
                            if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                                tracing::info!(?path, "Loaded Bold UI font");
                                font_bold = Some(font);
                                break;
                            }
                        }
                    }
                }
            }

            if font_regular.is_some() {
                break;
            }
        }

        Self {
            font_regular,
            font_bold,
            font_name,
            cache: HashMap::new(),
        }
    }

    fn rasterize_cached(&mut self, ch: char, px_size: f32, bold: bool) -> Option<(Metrics, &[u8])> {
        let font = if bold {
            self.font_bold.as_ref().or(self.font_regular.as_ref())?
        } else {
            self.font_regular.as_ref()?
        };
        let key = (ch, (px_size * 10.0).round() as u16, bold);
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
        let w = width.max(1);
        let h = height.max(1);
        let len = (w as usize) * (h as usize) * 4;
        Self {
            width: w,
            height: h,
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
        // Canvas stores premultiplied RGBA so GLES (GL_ONE, GL_ONE_MINUS_SRC_ALPHA) never produces bright halos.
        let src_r = (color[0] as u32 * alpha + 127) / 255;
        let src_g = (color[1] as u32 * alpha + 127) / 255;
        let src_b = (color[2] as u32 * alpha + 127) / 255;
        if alpha == 255 || self.pixels[idx + 3] == 0 {
            self.pixels[idx] = src_r as u8;
            self.pixels[idx + 1] = src_g as u8;
            self.pixels[idx + 2] = src_b as u8;
            self.pixels[idx + 3] = alpha as u8;
        } else {
            let inv = 255 - alpha;
            self.pixels[idx] = (src_r + (self.pixels[idx] as u32 * inv + 127) / 255).min(255) as u8;
            self.pixels[idx + 1] = (src_g + (self.pixels[idx + 1] as u32 * inv + 127) / 255).min(255) as u8;
            self.pixels[idx + 2] = (src_b + (self.pixels[idx + 2] as u32 * inv + 127) / 255).min(255) as u8;
            self.pixels[idx + 3] = (alpha + (self.pixels[idx + 3] as u32 * inv + 127) / 255).min(255) as u8;
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: [u8; 4]) {
        if w <= 0 || h <= 0 {
            return;
        }
        let x0 = x.clamp(0, self.width);
        let y0 = y.clamp(0, self.height);
        let x1 = (x + w).clamp(0, self.width);
        let y1 = (y + h).clamp(0, self.height);
        let a = color[3] as u32;
        let pr = ((color[0] as u32 * a + 127) / 255) as u8;
        let pg = ((color[1] as u32 * a + 127) / 255) as u8;
        let pb = ((color[2] as u32 * a + 127) / 255) as u8;

        for py in y0..y1 {
            for px in x0..x1 {
                let idx = ((py * self.width + px) as usize) * 4;
                self.pixels[idx] = pr;
                self.pixels[idx + 1] = pg;
                self.pixels[idx + 2] = pb;
                self.pixels[idx + 3] = color[3];
            }
        }
    }

    pub fn fill_rounded_rect(&mut self, x: i32, y: i32, w: i32, h: i32, radius: i32, color: [u8; 4]) {
        if w <= 0 || h <= 0 {
            return;
        }
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
        let a = color[3] as u32;
        let pr = ((color[0] as u32 * a + 127) / 255) as u8;
        let pg = ((color[1] as u32 * a + 127) / 255) as u8;
        let pb = ((color[2] as u32 * a + 127) / 255) as u8;

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
                self.pixels[idx] = pr;
                self.pixels[idx + 1] = pg;
                self.pixels[idx + 2] = pb;
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

    pub fn stroke_rounded_rect_aa(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: i32,
        color: [u8; 4],
    ) {
        if w <= 2 || h <= 2 || color[3] == 0 {
            return;
        }
        let r = (radius as f32).min((w.min(h) as f32) * 0.5).max(0.0);
        let x0 = (x - 1).clamp(0, self.width);
        let y0 = (y - 1).clamp(0, self.height);
        let x1 = (x + w + 1).clamp(0, self.width);
        let y1 = (y + h + 1).clamp(0, self.height);
        let cx = x as f32 + w as f32 * 0.5;
        let cy = y as f32 + h as f32 * 0.5;
        let hx = (w as f32 * 0.5 - r).max(0.0);
        let hy = (h as f32 * 0.5 - r).max(0.0);
        for py in y0..y1 {
            for px in x0..x1 {
                let dx = (px as f32 + 0.5 - cx).abs() - hx;
                let dy = (py as f32 + 0.5 - cy).abs() - hy;
                let dist = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.max(dy).min(0.0) - r;
                let ring = (1.0 - (dist + 0.5).abs()).clamp(0.0, 1.0);
                if ring > 0.01 {
                    let a = (color[3] as f32 * ring).round() as u8;
                    self.blend_pixel(px, py, [color[0], color[1], color[2], a]);
                }
            }
        }
    }

    /// Draws a surface panel styled according to the active `AppearanceMode`:
    /// - `Material3Expressive`: opaque/tonal `surfaceContainer` with clear `outlineVariant` border.
    /// - `GlassBlur`: translucent `surface` for standalone popups/tooltips without FBO backdrop.
    pub fn fill_glass_panel(&mut self, x: i32, y: i32, w: i32, h: i32, radius: i32) {
        self.fill_panel_impl(x, y, w, h, radius, false);
    }

    /// Draws a panel whose `GlassBlur` background is rendered by the GLSL FBO blur backdrop shader
    /// (transparent interior in `GlassBlur`, opaque `surfaceContainer` + `outlineVariant` in `M3E`).
    pub fn fill_frosted_backdrop_panel(&mut self, x: i32, y: i32, w: i32, h: i32, radius: i32) {
        self.fill_panel_impl(x, y, w, h, radius, true);
    }

    fn fill_panel_impl(&mut self, x: i32, y: i32, w: i32, h: i32, radius: i32, has_fbo_blur: bool) {
        if w <= 0 || h <= 0 {
            return;
        }
        let pal = active_m3_palette();
        match pal.appearance_mode {
            AppearanceMode::Material3Expressive => {
                // Opaque/tonal surfaceContainer body
                if w > 2 && h > 2 {
                    self.fill_rounded_rect_aa(
                        x + 1,
                        y + 1,
                        w - 2,
                        h - 2,
                        (radius - 1).max(2),
                        [pal.surface_container[0], pal.surface_container[1], pal.surface_container[2], 250],
                    );
                }
                // Clear outlineVariant perimeter border
                self.stroke_rounded_rect_aa(
                    x,
                    y,
                    w,
                    h,
                    radius,
                    [pal.outline_variant[0], pal.outline_variant[1], pal.outline_variant[2], 210],
                );
            }
            AppearanceMode::GlassBlur => {
                if !has_fbo_blur && w > 2 && h > 2 {
                    self.fill_rounded_rect_aa(
                        x + 1,
                        y + 1,
                        w - 2,
                        h - 2,
                        (radius - 1).max(2),
                        [pal.surface[0], pal.surface[1], pal.surface[2], 218],
                    );
                }
                // Subtle 1px bright edge on outline (FBO shader also renders edge when has_fbo_blur)
                if !has_fbo_blur {
                    self.stroke_rounded_rect_aa(x, y, w, h, radius, [255, 255, 255, 48]);
                }
            }
        }
        if w > 2 && h > 2 {
            // Subtle 1.5% tonal grain noise on interior pixels to prevent flat banding
            let x0 = (x + 2).clamp(0, self.width);
            let y0 = (y + 2).clamp(0, self.height);
            let x1 = (x + w - 2).clamp(0, self.width);
            let y1 = (y + h - 2).clamp(0, self.height);
            for py in y0..y1 {
                for px in x0..x1 {
                    let hval = ((px as u32).wrapping_mul(374761393)
                        ^ (py as u32).wrapping_mul(668265263))
                        .wrapping_mul(1274126177);
                    let delta = ((hval >> 24) & 3) as i16 - 1;
                    if delta != 0 {
                        let idx = ((py * self.width + px) as usize) * 4;
                        if self.pixels[idx + 3] > 100 {
                            self.pixels[idx] = (self.pixels[idx] as i16 + delta).clamp(0, 255) as u8;
                            self.pixels[idx + 1] = (self.pixels[idx + 1] as i16 + delta).clamp(0, 255) as u8;
                            self.pixels[idx + 2] = (self.pixels[idx + 2] as i16 + delta).clamp(0, 255) as u8;
                        }
                    }
                }
            }
        }
        // Subtle 1px inner top highlight
        let inset = (radius / 2).max(4);
        if w > inset * 2 {
            let hi_alpha = if pal.expressive { 42 } else { 24 };
            self.fill_rect_blend(x + inset, y + 1, w - inset * 2, 1, [255, 255, 255, hi_alpha]);
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

    /// Dark drop shadow exclusively for desktop wallpaper icon labels (`rgba(0, 0, 0, 0.65)`).
    pub fn draw_text_shadowed(&mut self, start_x: i32, start_y: i32, text: &str, px_size: f32, color: [u8; 4]) {
        self.draw_text_px(start_x, start_y + 1, text, px_size, [0, 0, 0, 166]);
        self.draw_text_px(start_x, start_y, text, px_size, color);
    }

    pub fn measure_text_px(&self, text: &str, px_size: f32) -> i32 {
        self.measure_text_weight(text, px_size, false)
    }

    pub fn measure_text_bold_px(&self, text: &str, px_size: f32) -> i32 {
        self.measure_text_weight(text, px_size, true)
    }

    fn measure_text_weight(&self, text: &str, px_size: f32, bold: bool) -> i32 {
        let mut width = 0.0f32;
        with_font_engine(|engine| {
            if engine.font_regular.is_some() {
                for ch in text.chars() {
                    if let Some((metrics, _)) = engine.rasterize_cached(ch, px_size, bold) {
                        width += metrics.advance_width;
                    }
                }
            }
        });
        width.ceil() as i32
    }

    pub fn draw_text_px(&mut self, start_x: i32, start_y: i32, text: &str, px_size: f32, color: [u8; 4]) {
        self.draw_text_weight(start_x, start_y, text, px_size, color, false);
    }

    pub fn draw_text_bold_px(&mut self, start_x: i32, start_y: i32, text: &str, px_size: f32, color: [u8; 4]) {
        self.draw_text_weight(start_x, start_y, text, px_size, color, true);
    }

    fn draw_text_weight(
        &mut self,
        start_x: i32,
        start_y: i32,
        text: &str,
        px_size: f32,
        color: [u8; 4],
        bold: bool,
    ) {
        let baseline_y = start_y + (px_size * 0.85).round() as i32;
        let mut cursor_x = start_x as f32;

        with_font_engine(|engine| {
            if engine.font_regular.is_some() {
                for ch in text.chars() {
                    if let Some((metrics, bitmap)) = engine.rasterize_cached(ch, px_size, bold) {
                        // Snap glyph origin to exact integer pixels to prevent subpixel blur
                        let gx = cursor_x.round() as i32 + metrics.xmin;
                        let gy = baseline_y - metrics.height as i32 - metrics.ymin;

                        for row in 0..metrics.height {
                            for col in 0..metrics.width {
                                let cov = bitmap[row * metrics.width + col];
                                if cov > 0 {
                                    let a = ((color[3] as u32 * cov as u32 + 127) / 255) as u8;
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

/// Draws a pill-shaped toggle switch with smooth animated left-to-right knob & track transition.
pub fn draw_animated_toggle_pill(
    canvas: &mut Canvas,
    key: &str,
    pill_x: i32,
    pill_y: i32,
    pill_w: i32,
    pill_h: i32,
    active: bool,
    accent: [u8; 3],
) {
    thread_local! {
        static TOGGLE_PROGRESS: std::cell::RefCell<std::collections::HashMap<String, f32>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    let target = if active { 1.0f32 } else { 0.0f32 };
    let t = TOGGLE_PROGRESS.with(|map| {
        let mut borrow = map.borrow_mut();
        let entry = borrow.entry(key.to_string()).or_insert(target);
        *entry += (target - *entry) * 0.28;
        if (target - *entry).abs() < 0.01 {
            *entry = target;
        }
        *entry
    });

    let r = (pill_h / 2).max(4);
    let track_r = (255.0 * (1.0 - t) + accent[0] as f32 * t).round() as u8;
    let track_g = (255.0 * (1.0 - t) + accent[1] as f32 * t).round() as u8;
    let track_b = (255.0 * (1.0 - t) + accent[2] as f32 * t).round() as u8;
    let track_a = (42.0 * (1.0 - t) + 245.0 * t).round() as u8;
    canvas.fill_rounded_rect_aa(pill_x, pill_y, pill_w, pill_h, r, [track_r, track_g, track_b, track_a]);

    let pad = (pill_h as f32 * 0.5).max(6.0);
    let min_x = pill_x as f32 + pad;
    let max_x = (pill_x + pill_w) as f32 - pad;
    let knob_cx = min_x + (max_x - min_x) * t;
    let knob_r = (pill_h as f32 * 0.375).max(4.5);
    canvas.fill_circle_aa(knob_cx, (pill_y + pill_h / 2) as f32, knob_r, [250, 252, 255, 255]);
}
