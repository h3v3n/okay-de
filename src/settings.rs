use std::{
    fs,
    path::PathBuf,
    process::Command,
};

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
    input::keyboard::XkbConfig,
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, Scale, Transform},
};

use crate::{
    panel::{active_font_name, contrast_fg, rect_contains, Canvas, ACCENT_PRESETS},
    render::OkayRenderElements,
    state::OkayState,
};

pub const SETTINGS_WIDTH: i32 = 740;
pub const SETTINGS_HEIGHT: i32 = 490;
const HEADER_HEIGHT: i32 = 38;
const SIDEBAR_WIDTH: i32 = 185;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Appearance,
    Displays,
    Network,
    Bluetooth,
    Sound,
    MouseKeyboard,
    DefaultApps,
    SystemInfo,
    Shortcuts,
    AdShield,
    Tweaks,
}

impl SettingsTab {
    const ALL: [Self; 11] = [
        Self::Appearance,
        Self::Displays,
        Self::Network,
        Self::Bluetooth,
        Self::Sound,
        Self::MouseKeyboard,
        Self::DefaultApps,
        Self::SystemInfo,
        Self::Shortcuts,
        Self::AdShield,
        Self::Tweaks,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Appearance => "1. Appearance & Anim",
            Self::Displays => "2. Displays (Ekrany)",
            Self::Network => "3. Wi-Fi & Network",
            Self::Bluetooth => "4. Bluetooth",
            Self::Sound => "5. Sound",
            Self::MouseKeyboard => "6. Mouse & Keyboard",
            Self::DefaultApps => "7. Default Apps",
            Self::SystemInfo => "8. System Info",
            Self::Shortcuts => "9. Keybindings",
            Self::AdShield => "10. AdShield & Privacy",
            Self::Tweaks => "11. Tweaks / Experim.",
        }
    }
}

#[derive(Debug, Clone)]
pub struct WifiNetwork {
    pub in_use: bool,
    pub ssid: String,
    pub signal: String,
    pub security: String,
}

#[derive(Debug, Clone)]
pub struct BluetoothDevice {
    pub mac: String,
    pub name: String,
    pub connected: bool,
}

#[derive(Debug, Clone)]
pub struct SystemInfoSnapshot {
    pub os_name: String,
    pub kernel_ver: String,
    pub cpu_model: String,
    pub cpu_load: String,
    pub ram_summary: String,
    pub disk_summary: String,
    pub gpu_renderer: String,
    pub input_devices: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SliderKind {
    OutputVol,
    MicVol,
    PointerSpeed,
    TopbarHeight,
    DockIconSize,
}

#[derive(Debug, Clone)]
enum SettingsAction {
    CloseWindow,
    SelectTab(SettingsTab),
    TogglePanelPosition,
    ToggleShowWorkspaces,
    ToggleDesktopIcons,
    SelectAccent(usize),
    ToggleDynamicAccent,
    ToggleM3Expressive,
    SetAppearanceMode(crate::panel::AppearanceMode),
    SetLightMode(bool),
    AdjustTopbarHeight(i32),
    AdjustDockIconSize(i32),
    ToggleDockAutohide,
    ResetPanelDefaults,
    SetAnimPreset(bool, f32),
    SetTargetRefreshHz(u32),
    RebuildIconCacheAndReload,
    AdjustCornerRadius(i32),
    AdjustWindowOpacity(f32),
    ToggleNightLight,
    AdjustNightLightTemp(i32),
    AdjustScrollSpeed(f64),
    AdjustKbRepeatDelay(i32),
    AdjustKbRepeatRate(i32),
    ToggleFocusFollowsMouse,
    CycleTitlebarDblClick,
    CycleHotCorner(usize),
    AdjustHotCornerDelay(i32),
    ToggleGameMode,
    SetPowerProfile(&'static str),
    ToggleAudioOveramp,
    CopySystemInfo,
    ToggleAnimations,
    AdjustAnimSpeed(f32),
    CycleMinimizeEffect,
    ReloadWallpaper,
    SetDisplayScale(usize, f64),
    CycleDisplayResolution(usize),
    CycleDisplayRefresh(usize),
    SwapMonitorsOrder,
    RefreshWifi,
    ToggleWifi(usize),
    SubmitWifiPassword,
    CancelWifiPassword,
    ToggleBluetoothPower,
    RefreshBluetooth,
    ToggleBluetoothDevice(usize),
    ToggleMuteOutput,
    ToggleMuteMic,
    SelectAudioSink(String),
    AdjustPointerSpeed(f64),
    ToggleNaturalScroll,
    TogglePointerAccel,
    CycleKeyboardLayout,
    CycleDefaultTerminal,
    CycleDefaultBrowser,
    CycleDefaultFileManager,
    OpenAutostartFile,
    CheckUpdates,
    InstallUpdates,
    ToggleAdShield,
    UpdateAdShieldList,
    FocusAdShieldInput,
    AddAdShieldWhitelist,
    AddAdShieldBlacklist,
    RemoveAdShieldWhitelist(String),
    RemoveAdShieldBlacklist(String),
}

#[derive(Debug)]
pub struct SettingsState {
    pub open: bool,
    pub active_tab: SettingsTab,
    anim_tab_y: f32,
    pub position: Point<i32, Logical>,
    pub dragging: Option<Point<f64, Logical>>,
    dragging_slider: Option<(SliderKind, Rectangle<i32, Logical>)>,
    slider_tracks: Vec<(Rectangle<i32, Logical>, SliderKind)>,
    pub wifi_networks: Vec<WifiNetwork>,
    pub wifi_status: String,
    pub wifi_password_prompt: Option<(String, String)>,
    pub bt_powered: bool,
    pub bt_status: String,
    pub bt_devices: Vec<BluetoothDevice>,
    pub mic_volume: i32,
    pub mic_muted: bool,
    pub audio_sink_name: String,
    pub audio_sinks: Vec<crate::system_backends::AudioSink>,
    pub xkb_layout: String,
    pub os_name: String,
    pub kernel_ver: String,
    pub cpu_model: String,
    pub cpu_load: String,
    pub gpu_renderer: String,
    pub ram_summary: String,
    pub disk_summary: String,
    pub input_devices: Vec<String>,
    pub update_status: String,
    pub adshield_domain_input: String,
    pub adshield_input_focused: bool,
    hit_regions: Vec<(Rectangle<i32, Logical>, SettingsAction)>,
}

impl Default for SettingsState {
    fn default() -> Self {
        let s = Self {
            open: false,
            active_tab: SettingsTab::Appearance,
            anim_tab_y: (HEADER_HEIGHT + 10) as f32,
            position: Point::from((120, 75)),
            dragging: None,
            dragging_slider: None,
            slider_tracks: Vec::new(),
            wifi_networks: Vec::new(),
            wifi_status: "Click 'Refresh Scan' to query NetworkManager".to_string(),
            wifi_password_prompt: None,
            bt_powered: true,
            bt_status: "Bluetooth Ready (click Scan Devices)".to_string(),
            bt_devices: Vec::new(),
            mic_volume: 80,
            mic_muted: false,
            audio_sink_name: "PipeWire / PulseAudio Default Output".to_string(),
            audio_sinks: Vec::new(),
            xkb_layout: "us".to_string(),
            os_name: String::new(),
            kernel_ver: String::new(),
            cpu_model: String::new(),
            cpu_load: String::new(),
            gpu_renderer: "OpenGL ES (Smithay GlesRenderer / DRM-KMS)".to_string(),
            ram_summary: String::new(),
            disk_summary: String::new(),
            input_devices: Vec::new(),
            update_status: "Check GitHub (h3v3n/okay-de) for updates".to_string(),
            adshield_domain_input: String::new(),
            adshield_input_focused: false,
            hit_regions: Vec::new(),
        };
        s
    }
}

pub fn collect_system_info_snapshot() -> SystemInfoSnapshot {
    SystemInfoSnapshot {
        os_name: read_os_release(),
        kernel_ver: fs::read_to_string("/proc/sys/kernel/osrelease")
            .unwrap_or_else(|_| "Linux".to_string())
            .trim()
            .to_string(),
        cpu_model: read_cpu_model(),
        cpu_load: read_cpu_load(),
        ram_summary: read_ram_summary(),
        disk_summary: read_disk_summary(),
        gpu_renderer: read_gpu_name(),
        input_devices: read_input_devices(),
    }
}

impl SettingsState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_system_info_snapshot(&mut self, snap: SystemInfoSnapshot) {
        self.os_name = snap.os_name;
        self.kernel_ver = snap.kernel_ver;
        self.cpu_model = snap.cpu_model;
        self.cpu_load = snap.cpu_load;
        self.ram_summary = snap.ram_summary;
        self.disk_summary = snap.disk_summary;
        self.gpu_renderer = snap.gpu_renderer;
        self.input_devices = snap.input_devices;
    }

    pub fn refresh_system_info(&mut self) {
        crate::worker::dispatch(crate::worker::WorkerRequest::RefreshSystemInfo);
    }

    pub fn refresh_wifi(&mut self, force_rescan: bool) {
        self.wifi_status = "Scanning Wi-Fi networks...".to_string();
        crate::worker::dispatch(crate::worker::WorkerRequest::RefreshWifi { force_rescan });
    }

    pub fn refresh_bluetooth(&mut self) {
        self.bt_status = "Scanning Bluetooth devices...".to_string();
        crate::worker::dispatch(crate::worker::WorkerRequest::RefreshBluetooth);
    }

    pub fn confirm_wifi_password(&mut self) {
        if let Some((ssid, pw)) = self.wifi_password_prompt.take() {
            self.wifi_status = format!("Connecting to {}...", ssid);
            crate::worker::dispatch(crate::worker::WorkerRequest::ConnectWifi {
                ssid,
                password: if pw.is_empty() { None } else { Some(pw) },
            });
        }
    }

    pub fn cancel_wifi_password(&mut self) {
        self.wifi_password_prompt = None;
    }

    pub fn is_dragging_slider(&self) -> bool {
        self.dragging_slider.is_some()
    }
}

fn save_all_prefs(state: &OkayState) {
    state.panel.save_full_config(
        state.pointer_sensitivity,
        state.natural_scroll,
        state.animations.enabled,
        state.animations.speed,
        state.animations.minimize_effect,
        &state.settings.xkb_layout,
    );
}

fn apply_slider_value(state: &mut OkayState, kind: SliderKind, track: Rectangle<i32, Logical>, px: i32) {
    let ratio = ((px - track.loc.x) as f64 / track.size.w.max(1) as f64).clamp(0.0, 1.0);
    match kind {
        SliderKind::OutputVol => {
            let pct = (ratio * 100.0).round() as i32;
            state.panel.set_volume(pct);
        }
        SliderKind::MicVol => {
            let pct = (ratio * 100.0).round() as i32;
            if pct != state.settings.mic_volume {
                state.settings.mic_volume = pct;
                let arg = format!("{}%", pct);
                let _ = Command::new("wpctl")
                    .args(["set-volume", "@DEFAULT_AUDIO_SOURCE@", &arg])
                    .spawn();
            }
        }
        SliderKind::PointerSpeed => {
            state.pointer_sensitivity = (0.25 + ratio * 2.25).clamp(0.25, 2.50);
        }
        SliderKind::TopbarHeight => {
            let h = (24.0 + ratio * 24.0).round() as i32;
            state.panel.topbar_height = h.clamp(24, 48);
            state.retile_workspace();
            state.blur_dirty = true;
        }
        SliderKind::DockIconSize => {
            let sz = (28.0 + ratio * 36.0).round() as i32;
            state.panel.dock_icon_size = sz.clamp(28, 64);
            state.retile_workspace();
            state.blur_dirty = true;
        }
    }
}

/// Handles pointer motion when the settings window header or a continuous slider is being dragged.
pub fn handle_settings_motion(state: &mut OkayState, pos: Point<f64, Logical>) -> bool {
    if let Some((kind, track)) = state.settings.dragging_slider {
        apply_slider_value(state, kind, track, pos.x.round() as i32);
        return true;
    }
    if let Some(offset) = state.settings.dragging {
        let nx = (pos.x - offset.x).round() as i32;
        let ny = (pos.y - offset.y).round() as i32;
        let min_y = state.panel.topbar_h() + 4;
        state.settings.position = Point::from((nx.max(8), ny.max(min_y)));
        return true;
    }
    false
}

/// Releases any active settings window or slider drag on mouse button release.
pub fn handle_settings_release(state: &mut OkayState) -> bool {
    if let Some((kind, _)) = state.settings.dragging_slider.take() {
        if matches!(
            kind,
            SliderKind::PointerSpeed | SliderKind::TopbarHeight | SliderKind::DockIconSize
        ) {
            save_all_prefs(state);
        }
        return true;
    }
    if state.settings.dragging.is_some() {
        state.settings.dragging = None;
        return true;
    }
    false
}

/// Handles a left-click on the `OKay Settings` window. Returns `true` if consumed.
pub fn handle_settings_click(state: &mut OkayState, global_pos: Point<f64, Logical>) -> bool {
    if !state.settings.open {
        return false;
    }

    let px = global_pos.x.round() as i32;
    let py = global_pos.y.round() as i32;
    let win_rect = Rectangle::new(
        state.settings.position,
        (SETTINGS_WIDTH, SETTINGS_HEIGHT).into(),
    );

    if !rect_contains(win_rect, px, py) {
        return false;
    }

    if let Some((track, kind)) = state
        .settings
        .slider_tracks
        .iter()
        .find(|(r, _)| rect_contains(*r, px, py))
        .copied()
    {
        apply_slider_value(state, kind, track, px);
        state.settings.dragging_slider = Some((kind, track));
        return true;
    }

    let mut matched: Option<SettingsAction> = None;
    for (rect, action) in state.settings.hit_regions.iter().rev() {
        if rect_contains(*rect, px, py) {
            matched = Some(action.clone());
            break;
        }
    }

    if let Some(action) = matched {
        match action {
            SettingsAction::CloseWindow => {
                state.settings.open = false;
                state.settings.dragging = None;
            }
            SettingsAction::SelectTab(tab) => {
                state.settings.active_tab = tab;
                state.settings.wifi_password_prompt = None;
                match tab {
                    SettingsTab::Network => {
                        state.settings.refresh_wifi(false);
                    }
                    SettingsTab::Bluetooth => {
                        state.settings.refresh_bluetooth();
                    }
                    SettingsTab::Sound => {
                        crate::worker::dispatch(crate::worker::WorkerRequest::RefreshAudio);
                    }
                    SettingsTab::SystemInfo => {
                        state.settings.refresh_system_info();
                    }
                    _ => {}
                }
            }
            SettingsAction::TogglePanelPosition => {
                state.panel.panel_top = !state.panel.panel_top;
                state.retile_workspace();
                save_all_prefs(state);
            }
            SettingsAction::ToggleShowWorkspaces => {
                state.panel.show_workspaces = !state.panel.show_workspaces;
                save_all_prefs(state);
            }
            SettingsAction::ToggleDesktopIcons => {
                state.overlays.show_desktop_icons = !state.overlays.show_desktop_icons;
            }
            SettingsAction::SelectAccent(idx) => {
                state.panel.dynamic_accent = false;
                state.panel.accent_preset = idx % ACCENT_PRESETS.len();
                state.theme.active_border_color = state.panel.accent_f32();
                save_all_prefs(state);
            }
            SettingsAction::ToggleDynamicAccent => {
                state.panel.dynamic_accent = !state.panel.dynamic_accent;
                state.panel.wallpaper_accent = state.desktop.dominant_accent();
                state.theme.active_border_color = state.panel.accent_f32();
                save_all_prefs(state);
            }
            SettingsAction::ToggleM3Expressive => {
                let next = match state.panel.appearance_mode {
                    crate::panel::AppearanceMode::Material3Expressive => {
                        crate::panel::AppearanceMode::GlassBlur
                    }
                    crate::panel::AppearanceMode::GlassBlur => {
                        crate::panel::AppearanceMode::Material3Expressive
                    }
                };
                state.panel.appearance_mode = next;
                state.panel.m3_expressive =
                    next == crate::panel::AppearanceMode::Material3Expressive;
                state.blur_dirty = true;
                save_all_prefs(state);
            }
            SettingsAction::SetAppearanceMode(mode) => {
                state.panel.appearance_mode = mode;
                state.panel.m3_expressive =
                    mode == crate::panel::AppearanceMode::Material3Expressive;
                state.blur_dirty = true;
                save_all_prefs(state);
            }
            SettingsAction::SetLightMode(light) => {
                state.panel.light_mode = light;
                state.blur_dirty = true;
                save_all_prefs(state);
            }
            SettingsAction::AdjustTopbarHeight(delta) => {
                state.panel.topbar_height = (state.panel.topbar_height + delta).clamp(24, 48);
                state.retile_workspace();
                state.blur_dirty = true;
                save_all_prefs(state);
            }
            SettingsAction::AdjustDockIconSize(delta) => {
                state.panel.dock_icon_size = (state.panel.dock_icon_size + delta).clamp(28, 64);
                state.retile_workspace();
                state.blur_dirty = true;
                save_all_prefs(state);
            }
            SettingsAction::ToggleDockAutohide => {
                state.panel.dock_autohide = !state.panel.dock_autohide;
                state.retile_workspace();
                save_all_prefs(state);
            }
            SettingsAction::ResetPanelDefaults => {
                state.panel.topbar_height = 32;
                state.panel.dock_icon_size = 44;
                state.panel.dock_padding = 8;
                state.panel.dock_autohide = false;
                state.retile_workspace();
                state.blur_dirty = true;
                save_all_prefs(state);
                state.overlays.show_osd("Panels", "Reset to 32px / 44px defaults", None);
            }
            SettingsAction::SetAnimPreset(enabled, speed) => {
                state.animations.enabled = enabled;
                state.animations.speed = speed;
                save_all_prefs(state);
            }
            SettingsAction::SetTargetRefreshHz(hz) => {
                state.panel.target_refresh_hz = hz;
                save_all_prefs(state);
            }
            SettingsAction::RebuildIconCacheAndReload => {
                let cfg = state.panel.load_config_file();
                state.pointer_sensitivity = cfg.pointer_speed;
                state.natural_scroll = cfg.natural_scroll;
                state.animations.enabled = cfg.anim_enabled;
                state.animations.speed = cfg.anim_speed;
                state.animations.minimize_effect = cfg.minimize_effect;
                state.theme.active_border_color = state.panel.accent_f32();
                state.desktop.reload();
                state.retile_workspace();
                state.blur_dirty = true;
                crate::worker::dispatch(crate::worker::WorkerRequest::RebuildIconCache);
            }
            SettingsAction::AdjustCornerRadius(delta) => {
                state.panel.window_corner_radius = (state.panel.window_corner_radius + delta).clamp(0, 24);
                save_all_prefs(state);
            }
            SettingsAction::AdjustWindowOpacity(delta) => {
                state.panel.window_opacity = (state.panel.window_opacity + delta).clamp(0.5, 1.0);
                save_all_prefs(state);
            }
            SettingsAction::ToggleNightLight => {
                state.panel.night_light_enabled = !state.panel.night_light_enabled;
                save_all_prefs(state);
            }
            SettingsAction::AdjustNightLightTemp(delta) => {
                state.panel.night_light_temp = (state.panel.night_light_temp + delta).clamp(2700, 6500);
                save_all_prefs(state);
            }
            SettingsAction::AdjustScrollSpeed(delta) => {
                state.panel.scroll_speed = (state.panel.scroll_speed + delta).clamp(0.25, 3.0);
                save_all_prefs(state);
            }
            SettingsAction::AdjustKbRepeatDelay(delta) => {
                state.panel.kb_repeat_delay = (state.panel.kb_repeat_delay + delta).clamp(150, 600);
                if let Some(kb) = state.seat.get_keyboard() {
                    kb.change_repeat_info(state.panel.kb_repeat_rate, state.panel.kb_repeat_delay);
                }
                save_all_prefs(state);
            }
            SettingsAction::AdjustKbRepeatRate(delta) => {
                state.panel.kb_repeat_rate = (state.panel.kb_repeat_rate + delta).clamp(15, 60);
                if let Some(kb) = state.seat.get_keyboard() {
                    kb.change_repeat_info(state.panel.kb_repeat_rate, state.panel.kb_repeat_delay);
                }
                save_all_prefs(state);
            }
            SettingsAction::ToggleFocusFollowsMouse => {
                state.panel.focus_follows_mouse = !state.panel.focus_follows_mouse;
                save_all_prefs(state);
            }
            SettingsAction::CycleTitlebarDblClick => {
                state.panel.titlebar_dblclick_action = state.panel.titlebar_dblclick_action.next();
                save_all_prefs(state);
            }
            SettingsAction::CycleHotCorner(idx) => {
                if idx < 4 {
                    state.panel.hot_corners[idx] = state.panel.hot_corners[idx].next();
                    save_all_prefs(state);
                }
            }
            SettingsAction::AdjustHotCornerDelay(delta) => {
                state.panel.hot_corner_delay_ms = (state.panel.hot_corner_delay_ms as i32 + delta).clamp(0, 600) as u32;
                save_all_prefs(state);
            }
            SettingsAction::ToggleGameMode => {
                state.panel.game_mode = !state.panel.game_mode;
                state.animations.enabled = !state.panel.game_mode;
                state.overlays.dnd_enabled = state.panel.game_mode;
                save_all_prefs(state);
                state.overlays.show_osd(
                    "Game Mode",
                    if state.panel.game_mode { "Enabled (No Anim + DND)" } else { "Disabled" },
                    None,
                );
            }
            SettingsAction::SetPowerProfile(prof) => {
                crate::worker::dispatch(crate::worker::WorkerRequest::RunCommand {
                    program: "powerprofilesctl".to_string(),
                    args: vec!["set".to_string(), prof.to_string()],
                });
                state.overlays.show_osd("Power Profile", prof, None);
            }
            SettingsAction::ToggleAudioOveramp => {
                state.panel.audio_overamp = !state.panel.audio_overamp;
                save_all_prefs(state);
            }
            SettingsAction::CopySystemInfo => {
                let info = format!(
                    "OS: {}\nKernel: {}\nCPU: {}\nGPU: {}\nRAM: {}\nDisk: {}",
                    state.settings.os_name,
                    state.settings.kernel_ver,
                    state.settings.cpu_model,
                    state.settings.gpu_renderer,
                    state.settings.ram_summary,
                    state.settings.disk_summary,
                );
                state.overlays.push_clipboard_text(info.clone());
                state.set_compositor_clipboard(info);
                state.overlays.push_notification("System Info", "Copied hardware summary to clipboard");
            }
            SettingsAction::ToggleAnimations => {
                state.animations.enabled = !state.animations.enabled;
                save_all_prefs(state);
            }
            SettingsAction::AdjustAnimSpeed(delta) => {
                state.animations.speed = (state.animations.speed + delta).clamp(0.4, 2.5);
                save_all_prefs(state);
            }
            SettingsAction::CycleMinimizeEffect => {
                state.animations.minimize_effect = state.animations.minimize_effect.next();
                save_all_prefs(state);
            }
            SettingsAction::ReloadWallpaper => {
                state.desktop.reload();
                state.blur_dirty = true;
                state.panel.wallpaper_accent = state.desktop.dominant_accent();
                if state.panel.dynamic_accent {
                    state.theme.active_border_color = state.panel.accent_f32();
                }
                state
                    .overlays
                    .push_notification("Wallpaper", "Reloaded ~/.config/okay-de/wallpaper.png");
            }
            SettingsAction::SetDisplayScale(out_idx, new_scale) => {
                let outputs: Vec<Output> = state.space.outputs().cloned().collect();
                if let Some(out) = outputs.get(out_idx) {
                    out.change_current_state(
                        None,
                        None,
                        Some(smithay::output::Scale::Fractional(new_scale)),
                        None,
                    );
                    smithay::desktop::layer_map_for_output(out).arrange();
                    state.update_output_fractional_scales(out);
                    state.retile_workspace();
                }
            }
            SettingsAction::CycleDisplayResolution(out_idx) => {
                let outputs: Vec<Output> = state.space.outputs().cloned().collect();
                if let Some(out) = outputs.get(out_idx) {
                    let cur = out.current_mode();
                    let modes = out.modes();
                    let mut resolutions: Vec<(i32, i32)> = Vec::new();
                    for m in &modes {
                        let r = (m.size.w, m.size.h);
                        if !resolutions.contains(&r) {
                            resolutions.push(r);
                        }
                    }
                    resolutions.sort_by(|a, b| (b.0 * b.1).cmp(&(a.0 * a.1)));
                    if resolutions.len() > 1 {
                        let cur_r = cur.map(|m| (m.size.w, m.size.h)).unwrap_or(resolutions[0]);
                        let pos = resolutions.iter().position(|r| *r == cur_r).unwrap_or(0);
                        let next_r = resolutions[(pos + 1) % resolutions.len()];
                        if let Some(target_mode) = modes
                            .iter()
                            .filter(|m| (m.size.w, m.size.h) == next_r)
                            .max_by_key(|m| m.refresh)
                            .copied()
                        {
                            out.change_current_state(Some(target_mode), None, None, None);
                            out.set_preferred(target_mode);
                            smithay::desktop::layer_map_for_output(out).arrange();
                            state.update_output_fractional_scales(out);
                            state.retile_workspace();
                        }
                    }
                }
            }
            SettingsAction::CycleDisplayRefresh(out_idx) => {
                let outputs: Vec<Output> = state.space.outputs().cloned().collect();
                if let Some(out) = outputs.get(out_idx) {
                    if let Some(cur) = out.current_mode() {
                        let modes = out.modes();
                        let mut matching: Vec<_> = modes
                            .iter()
                            .filter(|m| m.size == cur.size)
                            .copied()
                            .collect();
                        matching.sort_by(|a, b| b.refresh.cmp(&a.refresh));
                        matching.dedup_by_key(|m| m.refresh);
                        if matching.len() > 1 {
                            let pos = matching
                                .iter()
                                .position(|m| m.refresh == cur.refresh)
                                .unwrap_or(0);
                            let next_mode = matching[(pos + 1) % matching.len()];
                            out.change_current_state(Some(next_mode), None, None, None);
                            out.set_preferred(next_mode);
                            smithay::desktop::layer_map_for_output(out).arrange();
                            state.update_output_fractional_scales(out);
                        }
                    }
                }
            }
            SettingsAction::SwapMonitorsOrder => {
                let outputs: Vec<Output> = state.space.outputs().cloned().collect();
                if outputs.len() >= 2 {
                    let mut x_cursor = 0;
                    for out in outputs.iter().rev() {
                        let w = out
                            .current_mode()
                            .map(|m| m.size.w)
                            .unwrap_or(1920);
                        out.change_current_state(None, None, None, Some((x_cursor, 0).into()));
                        state.space.map_output(out, (x_cursor, 0));
                        x_cursor += w;
                    }
                    state.retile_workspace();
                }
            }
            SettingsAction::RefreshWifi => {
                state.settings.refresh_wifi(true);
            }
            SettingsAction::ToggleWifi(idx) => {
                if let Some(net) = state.settings.wifi_networks.get(idx).cloned() {
                    if net.in_use {
                        crate::worker::dispatch(crate::worker::WorkerRequest::DisconnectWifi {
                            ssid: net.ssid.clone(),
                        });
                        state.settings.wifi_status = format!("Disconnecting from {}...", net.ssid);
                    } else if net.security == "Open" {
                        crate::worker::dispatch(crate::worker::WorkerRequest::ConnectWifi {
                            ssid: net.ssid.clone(),
                            password: None,
                        });
                        state.settings.wifi_status = format!("Connecting to {}...", net.ssid);
                    } else {
                        state.settings.wifi_password_prompt =
                            Some((net.ssid.clone(), String::new()));
                    }
                }
            }
            SettingsAction::SubmitWifiPassword => {
                state.settings.confirm_wifi_password();
            }
            SettingsAction::CancelWifiPassword => {
                state.settings.cancel_wifi_password();
            }
            SettingsAction::ToggleBluetoothPower => {
                state.settings.bt_powered = !state.settings.bt_powered;
                let arg = if state.settings.bt_powered { "on" } else { "off" };
                crate::worker::dispatch(crate::worker::WorkerRequest::ToggleBluetoothPower {
                    enable: state.settings.bt_powered,
                });
                state.settings.bt_status = format!("Bluetooth power {}", arg);
            }
            SettingsAction::RefreshBluetooth => {
                state.settings.refresh_bluetooth();
            }
            SettingsAction::ToggleBluetoothDevice(idx) => {
                if let Some(dev) = state.settings.bt_devices.get_mut(idx) {
                    let connect = !dev.connected;
                    let sub = if connect { "connect" } else { "disconnect" };
                    crate::worker::dispatch(crate::worker::WorkerRequest::ToggleBluetoothDevice {
                        mac: dev.mac.clone(),
                        connect,
                    });
                    dev.connected = connect;
                    state.settings.bt_status = format!("{} {}...", sub, dev.name);
                }
            }
            SettingsAction::ToggleMuteOutput => {
                state.panel.toggle_mute();
                crate::worker::dispatch(crate::worker::WorkerRequest::ToggleAudioMute);
            }
            SettingsAction::ToggleMuteMic => {
                state.settings.mic_muted = !state.settings.mic_muted;
                if Command::new("wpctl")
                    .args(["set-mute", "@DEFAULT_AUDIO_SOURCE@", "toggle"])
                    .spawn()
                    .is_err()
                {
                    let _ = Command::new("pactl")
                        .args(["set-source-mute", "@DEFAULT_SOURCE@", "toggle"])
                        .spawn();
                }
            }
            SettingsAction::SelectAudioSink(sink_id) => {
                crate::worker::dispatch(crate::worker::WorkerRequest::SetDefaultAudioSink {
                    sink_id,
                });
            }
            SettingsAction::AdjustPointerSpeed(delta) => {
                state.pointer_sensitivity = (state.pointer_sensitivity + delta).clamp(0.25, 2.50);
                save_all_prefs(state);
            }
            SettingsAction::ToggleNaturalScroll => {
                state.natural_scroll = !state.natural_scroll;
                save_all_prefs(state);
            }
            SettingsAction::TogglePointerAccel => {
                state.pointer_accel_disabled = !state.pointer_accel_disabled;
                state.panel.pointer_accel_disabled = state.pointer_accel_disabled;
                state.apply_pointer_accel_profile();
                save_all_prefs(state);
            }
            SettingsAction::CycleKeyboardLayout => {
                let layouts = ["us", "pl", "de", "fr"];
                let cur = layouts
                    .iter()
                    .position(|x| *x == state.settings.xkb_layout)
                    .unwrap_or(0);
                let next_layout = layouts[(cur + 1) % layouts.len()];
                state.settings.xkb_layout = next_layout.to_string();
                if let Some(kb) = state.seat.get_keyboard() {
                    let xkb = XkbConfig {
                        layout: next_layout,
                        ..XkbConfig::default()
                    };
                    let _ = kb.set_xkb_config(state, xkb);
                }
                save_all_prefs(state);
            }
            SettingsAction::CycleDefaultTerminal => {
                let terms = ["foot", "kitty", "alacritty", "wezterm", "konsole", "xterm"];
                let cur = terms
                    .iter()
                    .position(|x| *x == state.panel.default_terminal)
                    .unwrap_or(0);
                state.panel.default_terminal = terms[(cur + 1) % terms.len()].to_string();
                save_all_prefs(state);
            }
            SettingsAction::CycleDefaultBrowser => {
                let browsers = ["firefox", "chromium", "brave", "floorp"];
                let cur = browsers
                    .iter()
                    .position(|x| *x == state.panel.default_browser)
                    .unwrap_or(0);
                state.panel.default_browser = browsers[(cur + 1) % browsers.len()].to_string();
                save_all_prefs(state);
            }
            SettingsAction::CycleDefaultFileManager => {
                let fms = ["thunar", "pcmanfm", "dolphin", "nautilus", "nemo"];
                let cur = fms
                    .iter()
                    .position(|x| *x == state.panel.default_file_manager)
                    .unwrap_or(0);
                state.panel.default_file_manager = fms[(cur + 1) % fms.len()].to_string();
                save_all_prefs(state);
            }
            SettingsAction::OpenAutostartFile => {
                if let Ok(home) = std::env::var("HOME") {
                    let path = PathBuf::from(home).join(".config/okay-de/autostart.sh");
                    if let Some(p) = path.parent() {
                        let _ = fs::create_dir_all(p);
                    }
                    if !path.exists() {
                        let _ = fs::write(&path, "#!/bin/sh\n# OKay DE Autostart commands\n");
                    }
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Ok(meta) = fs::metadata(&path) {
                            let mut perms = meta.permissions();
                            perms.set_mode(0o755);
                            let _ = fs::set_permissions(&path, perms);
                        }
                    }
                    let _ = Command::new("xdg-open")
                        .arg(&path)
                        .env("WAYLAND_DISPLAY", &state.socket_name)
                        .spawn();
                    state
                        .overlays
                        .push_notification("Autostart Script", &format!("Opened {}", path.display()));
                }
            }
            SettingsAction::CheckUpdates => {
                state.settings.update_status = "Checking github.com/h3v3n/okay-de...".to_string();
                crate::worker::dispatch(crate::worker::WorkerRequest::CheckSystemUpdates);
            }
            SettingsAction::ToggleAdShield => {
                let next = state.adshield.toggle();
                state.panel.adshield_enabled = next;
                save_all_prefs(state);
            }
            SettingsAction::UpdateAdShieldList => {
                state.adshield.trigger_update_blocklist();
            }
            SettingsAction::FocusAdShieldInput => {
                state.settings.adshield_input_focused = true;
            }
            SettingsAction::AddAdShieldWhitelist => {
                let raw = state.settings.adshield_domain_input.trim().to_string();
                if state.adshield.add_whitelist_domain(&raw) {
                    state.settings.adshield_domain_input.clear();
                    state.panel.adshield_whitelist = state.adshield.whitelist.clone();
                    state.panel.adshield_blacklist = state.adshield.blacklist.clone();
                    save_all_prefs(state);
                }
            }
            SettingsAction::AddAdShieldBlacklist => {
                let raw = state.settings.adshield_domain_input.trim().to_string();
                if state.adshield.add_blacklist_domain(&raw) {
                    state.settings.adshield_domain_input.clear();
                    state.panel.adshield_whitelist = state.adshield.whitelist.clone();
                    state.panel.adshield_blacklist = state.adshield.blacklist.clone();
                    save_all_prefs(state);
                }
            }
            SettingsAction::RemoveAdShieldWhitelist(dom) => {
                state.adshield.remove_whitelist_domain(&dom);
                state.panel.adshield_whitelist = state.adshield.whitelist.clone();
                save_all_prefs(state);
            }
            SettingsAction::RemoveAdShieldBlacklist(dom) => {
                state.adshield.remove_blacklist_domain(&dom);
                state.panel.adshield_blacklist = state.adshield.blacklist.clone();
                save_all_prefs(state);
            }
            SettingsAction::InstallUpdates => {
                let cmd = crate::system_backends::install_system_updates_cmd();
                let preferred = state.panel.default_terminal.clone();
                let terms = [
                    preferred.as_str(),
                    "foot",
                    "kitty",
                    "alacritty",
                    "wezterm",
                    "konsole",
                    "gnome-terminal",
                    "xterm",
                ];
                let mut launched = false;
                for term in terms {
                    let ok = if term == "gnome-terminal" {
                        Command::new(term)
                            .args(["--", "sh", "-c", &cmd])
                            .env("WAYLAND_DISPLAY", &state.socket_name)
                            .spawn()
                            .is_ok()
                    } else {
                        Command::new(term)
                            .args(["-e", "sh", "-c", &cmd])
                            .env("WAYLAND_DISPLAY", &state.socket_name)
                            .spawn()
                            .is_ok()
                    };
                    if ok {
                        launched = true;
                        break;
                    }
                }
                state.settings.update_status = if launched {
                    "GitHub updater launched in terminal".to_string()
                } else {
                    "Could not open terminal for GitHub update".to_string()
                };
            }
        }
        return true;
    }

    let header_rect = Rectangle::new(
        state.settings.position,
        (SETTINGS_WIDTH - 40, HEADER_HEIGHT).into(),
    );
    if rect_contains(header_rect, px, py) {
        state.settings.dragging = Some(Point::from((
            global_pos.x - state.settings.position.x as f64,
            global_pos.y - state.settings.position.y as f64,
        )));
    }

    true
}

/// Renders the native 8-Tab `OKay Settings` Control Center window if open on the primary output.
pub fn render_settings_elements(
    state: &mut OkayState,
    renderer: &mut GlesRenderer,
    output: &Output,
    scale: Scale<f64>,
) -> Vec<OkayRenderElements> {
    if !state.settings.open {
        return Vec::new();
    }
    let is_primary = state
        .space
        .outputs()
        .next()
        .map(|o| o == output)
        .unwrap_or(false);
    if !is_primary {
        return Vec::new();
    }
    let Some(output_geo) = state.space.output_geometry(output) else {
        return Vec::new();
    };

    state.settings.hit_regions.clear();
    state.settings.slider_tracks.clear();
    let accent = state.panel.accent_rgb();
    let wx = state.settings.position.x;
    let wy = state.settings.position.y;
    let pointer_x = state.pointer_location.x.round() as i32;
    let pointer_y = state.pointer_location.y.round() as i32;

    let mut canvas = SettingsCanvas::new(SETTINGS_WIDTH, SETTINGS_HEIGHT);
    canvas.inner.fill_frosted_backdrop_panel(0, 0, SETTINGS_WIDTH, SETTINGS_HEIGHT, 18);

    // Crisp 1px vertical sidebar divider (no extra stacked background rect)
    canvas.fill_rect_blend(
        SIDEBAR_WIDTH + 2,
        8,
        1,
        SETTINGS_HEIGHT - 16,
        [255, 255, 255, 22],
    );

    // Top-left macOS Traffic Light Close button (#ff5f57)
    let close_rect = Rectangle::new((wx + 10, wy + 10).into(), (20, 20).into());
    let close_hovered = rect_contains(close_rect, pointer_x, pointer_y);
    canvas.fill_circle_aa(20.0, 20.0, 6.5, [255, 95, 87, 255]);
    if close_hovered {
        canvas.draw_text_raw(17, 13, "x", 11.0, [24, 18, 18, 220]);
    }
    state
        .settings
        .hit_regions
        .push((close_rect, SettingsAction::CloseWindow));

    canvas.draw_text_bold_px(38, 12, "System Settings", 15.0, [244, 246, 255, 255]);

    // Smoothly interpolate sidebar active pill indicator toward selected tab Y
    let active_idx = SettingsTab::ALL
        .iter()
        .position(|t| *t == state.settings.active_tab)
        .unwrap_or(0);
    let target_tab_y = (HEADER_HEIGHT + 10 + active_idx as i32 * 38) as f32;
    state.settings.anim_tab_y += (target_tab_y - state.settings.anim_tab_y) * 0.55;
    if (target_tab_y - state.settings.anim_tab_y).abs() < 0.5 {
        state.settings.anim_tab_y = target_tab_y;
    }
    canvas.fill_rounded_rect_aa(
        10,
        state.settings.anim_tab_y.round() as i32,
        SIDEBAR_WIDTH - 16,
        32,
        8,
        [accent[0], accent[1], accent[2], 235],
    );

    for (i, tab) in SettingsTab::ALL.iter().enumerate() {
        let ty = HEADER_HEIGHT + 10 + i as i32 * 38;
        let tab_rect = Rectangle::new(
            (wx + 10, wy + ty).into(),
            (SIDEBAR_WIDTH - 16, 32).into(),
        );
        let is_selected = state.settings.active_tab == *tab;
        let hovered = rect_contains(tab_rect, pointer_x, pointer_y);
        if !is_selected && hovered {
            canvas.fill_rounded_rect_aa(10, ty, SIDEBAR_WIDTH - 16, 32, 8, [255, 255, 255, 22]);
        }
        if is_selected {
            canvas.draw_text_bold_raw(20, ty + 8, tab.title(), 13.0, contrast_fg(accent));
        } else {
            canvas.draw_text_px(20, ty + 8, tab.title(), 13.0, [232, 234, 246, 255]);
        }
        state
            .settings
            .hit_regions
            .push((tab_rect, SettingsAction::SelectTab(*tab)));
    }

    let content_x = SIDEBAR_WIDTH + 20;
    let mut cy = HEADER_HEIGHT + 14;

    match state.settings.active_tab {
        SettingsTab::Appearance => {
            canvas.draw_text_bold_px(
                content_x,
                cy,
                "Appearance, Dock & Animations",
                16.0,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 30;

            // Row 1: 2 balanced Desktop / Workspace toggles (248px each + 10px gap = 506px)
            let ws_rect = Rectangle::new((wx + content_x, wy + cy).into(), (248, 36).into());
            draw_setting_button(&mut canvas, content_x, cy, 248, 36, "Workspaces", state.panel.show_workspaces, accent);
            state.settings.hit_regions.push((ws_rect, SettingsAction::ToggleShowWorkspaces));

            let ic_rect = Rectangle::new((wx + content_x + 258, wy + cy).into(), (248, 36).into());
            draw_setting_button(&mut canvas, content_x + 258, cy, 248, 36, "Desktop Icons", state.overlays.show_desktop_icons, accent);
            state.settings.hit_regions.push((ic_rect, SettingsAction::ToggleDesktopIcons));
            cy += 42;

            // Row 2: Minimize Animation Effect Selector (Magic Lamp Genie / ScaleGlide / Off)
            let min_rect = Rectangle::new((wx + content_x, wy + cy).into(), (506, 36).into());
            let min_lbl = format!(
                "Minimize Effect:   {}   (click to cycle)",
                state.animations.minimize_effect.label()
            );
            draw_setting_button(
                &mut canvas,
                content_x,
                cy,
                506,
                36,
                &min_lbl,
                state.animations.enabled,
                accent,
            );
            state
                .settings
                .hit_regions
                .push((min_rect, SettingsAction::CycleMinimizeEffect));
            cy += 42;

            // Row 3: Auto Refresh Sync Info + Animation Speed Controls
            let max_refresh_mhz = state
                .space
                .outputs()
                .filter_map(|o| o.current_mode())
                .map(|m| m.refresh)
                .max()
                .unwrap_or(60_000);
            let sync_lbl = if max_refresh_mhz >= 120_000 {
                format!("Animations: 120 FPS ({:.0}Hz)", max_refresh_mhz as f64 / 1000.0)
            } else {
                format!("Animations: 60 FPS ({:.0}Hz)", max_refresh_mhz as f64 / 1000.0)
            };
            canvas.fill_rounded_rect_aa(content_x, cy, 248, 34, 10, [30, 33, 46, 235]);
            canvas.draw_text_px(content_x + 12, cy + 9, &sync_lbl, 12.5, [215, 224, 245, 255]);

            let spd_txt = format!("Speed: {:.2}x", state.animations.speed);
            canvas.draw_text_px(content_x + 262, cy + 9, &spd_txt, 13.0, [225, 228, 245, 255]);
            let s_dec = Rectangle::new((wx + content_x + 426, wy + cy + 1).into(), (34, 32).into());
            canvas.fill_rounded_rect_aa(content_x + 426, cy + 1, 34, 32, 8, [44, 46, 68, 255]);
            canvas.draw_text_px(content_x + 439, cy + 8, "-", 14.0, [240, 242, 252, 255]);
            state.settings.hit_regions.push((s_dec, SettingsAction::AdjustAnimSpeed(-0.25)));

            let s_inc = Rectangle::new((wx + content_x + 468, wy + cy + 1).into(), (34, 32).into());
            canvas.fill_rounded_rect_aa(content_x + 468, cy + 1, 34, 32, 8, [44, 46, 68, 255]);
            canvas.draw_text_px(content_x + 480, cy + 8, "+", 14.0, [240, 242, 252, 255]);
            state.settings.hit_regions.push((s_inc, SettingsAction::AdjustAnimSpeed(0.25)));
            cy += 40;

            // Row 4: Appearance Mode Switcher (Material 3 Expressive vs Glass / Blur)
            canvas.draw_text_px(content_x, cy, "Appearance Mode", 12.5, [215, 220, 238, 255]);
            cy += 18;
            let is_m3e = state.panel.appearance_mode
                == crate::panel::AppearanceMode::Material3Expressive;
            let is_glass =
                state.panel.appearance_mode == crate::panel::AppearanceMode::GlassBlur;

            let m3e_rect = Rectangle::new((wx + content_x, wy + cy).into(), (248, 34).into());
            draw_setting_button(
                &mut canvas,
                content_x,
                cy,
                248,
                34,
                "Material 3 Expressive",
                is_m3e,
                accent,
            );
            state.settings.hit_regions.push((
                m3e_rect,
                SettingsAction::SetAppearanceMode(
                    crate::panel::AppearanceMode::Material3Expressive,
                ),
            ));

            let glass_rect =
                Rectangle::new((wx + content_x + 258, wy + cy).into(), (248, 34).into());
            draw_setting_button(
                &mut canvas,
                content_x + 258,
                cy,
                248,
                34,
                "Glass / Blur",
                is_glass,
                accent,
            );
            state.settings.hit_regions.push((
                glass_rect,
                SettingsAction::SetAppearanceMode(crate::panel::AppearanceMode::GlassBlur),
            ));
            cy += 40;

            // Row 5: Theme Mode Switcher (Dark Mode vs Light Mode)
            canvas.draw_text_px(content_x, cy, "Theme", 12.5, [215, 220, 238, 255]);
            cy += 18;
            let dark_rect = Rectangle::new((wx + content_x, wy + cy).into(), (248, 34).into());
            draw_setting_button(
                &mut canvas,
                content_x,
                cy,
                248,
                34,
                "Dark Mode",
                !state.panel.light_mode,
                accent,
            );
            state
                .settings
                .hit_regions
                .push((dark_rect, SettingsAction::SetLightMode(false)));

            let light_rect =
                Rectangle::new((wx + content_x + 258, wy + cy).into(), (248, 34).into());
            draw_setting_button(
                &mut canvas,
                content_x + 258,
                cy,
                248,
                34,
                "Light Mode",
                state.panel.light_mode,
                accent,
            );
            state
                .settings
                .hit_regions
                .push((light_rect, SettingsAction::SetLightMode(true)));
            cy += 40;

            // Row 6: Circular Accent Color Swatch Dots with Active Ring
            canvas.draw_text_px(content_x, cy, "Accent Color", 12.5, [215, 220, 238, 255]);
            cy += 18;
            canvas.fill_rounded_rect_aa(content_x, cy - 4, 506, 36, 10, [255, 255, 255, 16]);
            for (idx, (name, rgb, _)) in ACCENT_PRESETS.iter().enumerate() {
                let bx = content_x + 14 + idx as i32 * 82;
                let preset_rect = Rectangle::new((wx + bx - 4, wy + cy).into(), (78, 28).into());
                let dot_cx = (bx + 12) as f32;
                let dot_cy = (cy + 14) as f32;
                if !state.panel.dynamic_accent && state.panel.accent_preset == idx {
                    let ring_col = if state.panel.light_mode {
                        [22, 26, 38, 235]
                    } else {
                        [255, 255, 255, 235]
                    };
                    let inner_ring = if state.panel.light_mode {
                        [240, 244, 252, 255]
                    } else {
                        [24, 26, 36, 255]
                    };
                    canvas.fill_circle_aa(dot_cx, dot_cy, 11.5, ring_col);
                    canvas.fill_circle_aa(dot_cx, dot_cy, 9.5, inner_ring);
                }
                canvas.fill_circle_aa(dot_cx, dot_cy, 8.0, [rgb[0], rgb[1], rgb[2], 255]);
                canvas.draw_text_px(bx + 26, cy + 7, name, 11.5, [232, 236, 248, 255]);
                state.settings.hit_regions.push((preset_rect, SettingsAction::SelectAccent(idx)));
            }
            cy += 38;

            // Row 7: Topbar Height & Dock Icon Size Live Sliders + Reset Defaults
            let tb_h = state.panel.topbar_h();
            let tb_lbl = format!("Topbar Height: {} px", tb_h);
            canvas.draw_text_px(content_x, cy, &tb_lbl, 12.0, [215, 220, 238, 255]);
            let tb_dec = Rectangle::new((wx + content_x + 150, wy + cy - 2).into(), (24, 20).into());
            canvas.fill_rounded_rect_aa(content_x + 150, cy - 2, 24, 20, 5, [44, 46, 68, 255]);
            canvas.draw_text_px(content_x + 158, cy, "-", 12.5, [240, 242, 252, 255]);
            state.settings.hit_regions.push((tb_dec, SettingsAction::AdjustTopbarHeight(-2)));

            let tb_track_x = content_x + 180;
            let tb_track_w = 292;
            canvas.fill_rounded_rect_aa(tb_track_x, cy + 4, tb_track_w, 8, 4, [28, 30, 44, 255]);
            let tb_norm = ((tb_h - 24) as f64 / 24.0).clamp(0.0, 1.0);
            let tb_fill = (tb_norm * tb_track_w as f64).round() as i32;
            if tb_fill > 2 {
                canvas.fill_rounded_rect_aa(tb_track_x, cy + 4, tb_fill.max(6), 8, 4, [accent[0], accent[1], accent[2], 255]);
            }
            let tb_knob = (tb_track_x + tb_fill.clamp(5, tb_track_w - 5)) as f32;
            canvas.fill_circle_aa(tb_knob, (cy + 8) as f32, 6.5, [248, 250, 255, 255]);
            state.settings.slider_tracks.push((
                Rectangle::new((wx + tb_track_x, wy + cy - 2).into(), (tb_track_w, 22).into()),
                SliderKind::TopbarHeight,
            ));

            let tb_inc = Rectangle::new((wx + content_x + 480, wy + cy - 2).into(), (24, 20).into());
            canvas.fill_rounded_rect_aa(content_x + 480, cy - 2, 24, 20, 5, [44, 46, 68, 255]);
            canvas.draw_text_px(content_x + 487, cy, "+", 12.5, [240, 242, 252, 255]);
            state.settings.hit_regions.push((tb_inc, SettingsAction::AdjustTopbarHeight(2)));
            cy += 26;

            let dk_sz = state.panel.dock_icon_sz();
            let dk_lbl = format!("Dock Icons: {} px", dk_sz);
            canvas.draw_text_px(content_x, cy, &dk_lbl, 12.0, [215, 220, 238, 255]);
            let dk_dec = Rectangle::new((wx + content_x + 150, wy + cy - 2).into(), (24, 20).into());
            canvas.fill_rounded_rect_aa(content_x + 150, cy - 2, 24, 20, 5, [44, 46, 68, 255]);
            canvas.draw_text_px(content_x + 158, cy, "-", 12.5, [240, 242, 252, 255]);
            state.settings.hit_regions.push((dk_dec, SettingsAction::AdjustDockIconSize(-2)));

            canvas.fill_rounded_rect_aa(tb_track_x, cy + 4, tb_track_w, 8, 4, [28, 30, 44, 255]);
            let dk_norm = ((dk_sz - 28) as f64 / 36.0).clamp(0.0, 1.0);
            let dk_fill = (dk_norm * tb_track_w as f64).round() as i32;
            if dk_fill > 2 {
                canvas.fill_rounded_rect_aa(tb_track_x, cy + 4, dk_fill.max(6), 8, 4, [accent[0], accent[1], accent[2], 255]);
            }
            let dk_knob = (tb_track_x + dk_fill.clamp(5, tb_track_w - 5)) as f32;
            canvas.fill_circle_aa(dk_knob, (cy + 8) as f32, 6.5, [248, 250, 255, 255]);
            state.settings.slider_tracks.push((
                Rectangle::new((wx + tb_track_x, wy + cy - 2).into(), (tb_track_w, 22).into()),
                SliderKind::DockIconSize,
            ));

            let dk_inc = Rectangle::new((wx + content_x + 480, wy + cy - 2).into(), (24, 20).into());
            canvas.fill_rounded_rect_aa(content_x + 480, cy - 2, 24, 20, 5, [44, 46, 68, 255]);
            canvas.draw_text_px(content_x + 487, cy, "+", 12.5, [240, 242, 252, 255]);
            state.settings.hit_regions.push((dk_inc, SettingsAction::AdjustDockIconSize(2)));
            cy += 26;

            // Row 8: Dock Autohide + Reset Panel Defaults + Reload Wallpaper
            let ah_rect = Rectangle::new((wx + content_x, wy + cy).into(), (160, 26).into());
            draw_setting_button(&mut canvas, content_x, cy, 160, 26, "Dock Autohide", state.panel.dock_autohide, accent);
            state.settings.hit_regions.push((ah_rect, SettingsAction::ToggleDockAutohide));

            let rst_rect = Rectangle::new((wx + content_x + 168, wy + cy).into(), (165, 26).into());
            canvas.fill_rounded_rect_aa(content_x + 168, cy, 165, 26, 6, [44, 46, 68, 255]);
            canvas.draw_text_px(content_x + 180, cy + 6, "Reset Defaults (32/44)", 11.5, [235, 238, 250, 255]);
            state.settings.hit_regions.push((rst_rect, SettingsAction::ResetPanelDefaults));

            let wp_rect = Rectangle::new((wx + content_x + 341, wy + cy).into(), (165, 26).into());
            canvas.fill_rounded_rect_aa(content_x + 341, cy, 165, 26, 6, [38, 38, 58, 255]);
            canvas.draw_text_px(content_x + 356, cy + 6, "Reload Wallpaper", 11.5, [235, 238, 250, 255]);
            state.settings.hit_regions.push((wp_rect, SettingsAction::ReloadWallpaper));
        }

        SettingsTab::Displays => {
            canvas.draw_text_px(
                content_x,
                cy,
                "Displays & Multi-Monitor Arrangement",
                15.0,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 24;

            let outputs: Vec<Output> = state.space.outputs().cloned().collect();
            let multi_lbl = if outputs.len() > 1 {
                format!("Multi-Monitor Active ({} displays detected)", outputs.len())
            } else {
                "Single Display Mode (1 display detected)".to_string()
            };
            canvas.fill_rounded_rect_aa(
                content_x,
                cy,
                506,
                26,
                6,
                if outputs.len() > 1 {
                    [accent[0], accent[1], accent[2], 42]
                } else {
                    [255, 255, 255, 14]
                },
            );
            canvas.draw_text_px(content_x + 12, cy + 6, &multi_lbl, 12.0, [226, 232, 248, 255]);
            cy += 34;

            for (idx, out) in outputs.iter().take(2).enumerate() {
                let geo = state.space.output_geometry(out).unwrap_or_default();
                let cur_scale = out.current_scale().fractional_scale();
                let cur_mode = out.current_mode();
                let modes = out.modes();

                let mut resolutions: Vec<(i32, i32)> = Vec::new();
                let mut refreshes: Vec<i32> = Vec::new();
                for m in &modes {
                    let r = (m.size.w, m.size.h);
                    if !resolutions.contains(&r) {
                        resolutions.push(r);
                    }
                    if let Some(cm) = cur_mode {
                        if m.size == cm.size && !refreshes.contains(&m.refresh) {
                            refreshes.push(m.refresh);
                        }
                    }
                }

                let refresh_hz = cur_mode.map(|m| m.refresh as f64 / 1000.0).unwrap_or(60.0);
                let has_multi_res = resolutions.len() > 1;
                let has_multi_hz = refreshes.len() > 1;
                let card_h = if has_multi_res || has_multi_hz { 112 } else { 78 };

                canvas.fill_rounded_rect_aa(content_x, cy, 506, card_h, 8, [28, 28, 44, 245]);
                let title = format!(
                    "Monitor #{}: {}  ({}x{} @ {:.0}Hz, Pos +{},+{})",
                    idx + 1,
                    out.name(),
                    geo.size.w,
                    geo.size.h,
                    refresh_hz,
                    geo.loc.x,
                    geo.loc.y
                );
                canvas.draw_text_px(content_x + 14, cy + 10, &title, 13.0, [238, 240, 252, 255]);

                canvas.draw_text_px(content_x + 14, cy + 44, "UI Scale:", 12.0, [175, 180, 205, 255]);
                for (si, s_val) in [1.0, 1.25, 1.5, 2.0].into_iter().enumerate() {
                    let sx = content_x + 96 + si as i32 * 76;
                    let s_rect = Rectangle::new((wx + sx, wy + cy + 38).into(), (68, 28).into());
                    let is_cur = (cur_scale - s_val).abs() < 0.05;
                    let bg = if is_cur {
                        [accent[0], accent[1], accent[2], 255]
                    } else {
                        [44, 44, 66, 255]
                    };
                    let fg = if is_cur { [16, 16, 24, 255] } else { [230, 232, 246, 255] };
                    canvas.fill_rounded_rect_aa(sx, cy + 38, 68, 28, 5, bg);
                    canvas.draw_text_px(sx + 14, cy + 44, &format!("{:.2}x", s_val), 12.0, fg);
                    state
                        .settings
                        .hit_regions
                        .push((s_rect, SettingsAction::SetDisplayScale(idx, s_val)));
                }

                if has_multi_res || has_multi_hz {
                    let ry = cy + 74;
                    if has_multi_res {
                        let res_rect = Rectangle::new((wx + content_x + 14, wy + ry).into(), (228, 28).into());
                        canvas.fill_rounded_rect_aa(content_x + 14, ry, 228, 28, 6, [44, 44, 66, 255]);
                        let r_lbl = format!("Resolution: {}x{}  >", geo.size.w, geo.size.h);
                        canvas.draw_text_px(content_x + 24, ry + 7, &r_lbl, 11.5, [234, 238, 250, 255]);
                        state
                            .settings
                            .hit_regions
                            .push((res_rect, SettingsAction::CycleDisplayResolution(idx)));
                    }
                    if has_multi_hz {
                        let hx = if has_multi_res { content_x + 254 } else { content_x + 14 };
                        let hz_rect = Rectangle::new((wx + hx, wy + ry).into(), (238, 28).into());
                        canvas.fill_rounded_rect_aa(hx, ry, 238, 28, 6, [44, 44, 66, 255]);
                        let h_lbl = format!("Refresh Rate: {:.0} Hz  >", refresh_hz);
                        canvas.draw_text_px(hx + 12, ry + 7, &h_lbl, 11.5, [234, 238, 250, 255]);
                        state
                            .settings
                            .hit_regions
                            .push((hz_rect, SettingsAction::CycleDisplayRefresh(idx)));
                    }
                }
                cy += card_h + 10;
            }

            if outputs.len() >= 2 {
                let swap_rect = Rectangle::new((wx + content_x, wy + cy).into(), (280, 34).into());
                canvas.fill_rounded_rect_aa(content_x, cy, 280, 34, 6, [44, 44, 68, 255]);
                canvas.draw_text_px(
                    content_x + 16,
                    cy + 9,
                    "Swap Left / Right Monitor Order",
                    13.0,
                    [238, 240, 252, 255],
                );
                state
                    .settings
                    .hit_regions
                    .push((swap_rect, SettingsAction::SwapMonitorsOrder));
                cy += 42;
            }

            // Night Light (Warm Color Temperature 2700K - 6500K)
            let nl_btn = Rectangle::new((wx + content_x, wy + cy).into(), (240, 32).into());
            let nl_lbl = if state.panel.night_light_enabled {
                format!("Night Light: ON ({}K)", state.panel.night_light_temp)
            } else {
                format!("Night Light: OFF ({}K)", state.panel.night_light_temp)
            };
            draw_setting_button(&mut canvas, content_x, cy, 240, 32, &nl_lbl, state.panel.night_light_enabled, accent);
            state.settings.hit_regions.push((nl_btn, SettingsAction::ToggleNightLight));

            let nl_dec = Rectangle::new((wx + content_x + 250, wy + cy).into(), (122, 32).into());
            canvas.fill_rounded_rect_aa(content_x + 250, cy, 122, 32, 6, [44, 46, 68, 255]);
            canvas.draw_text_px(content_x + 266, cy + 8, "-300K (Warmer)", 11.5, [238, 240, 252, 255]);
            state.settings.hit_regions.push((nl_dec, SettingsAction::AdjustNightLightTemp(-300)));

            let nl_inc = Rectangle::new((wx + content_x + 380, wy + cy).into(), (126, 32).into());
            canvas.fill_rounded_rect_aa(content_x + 380, cy, 126, 32, 6, [44, 46, 68, 255]);
            canvas.draw_text_px(content_x + 398, cy + 8, "+300K (Cooler)", 11.5, [238, 240, 252, 255]);
            state.settings.hit_regions.push((nl_inc, SettingsAction::AdjustNightLightTemp(300)));
        }

        SettingsTab::Network => {
            canvas.draw_text_px(
                content_x,
                cy,
                "Wi-Fi & Network Connections",
                15.0,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 28;

            canvas.draw_text_px(
                content_x,
                cy,
                &state.settings.wifi_status,
                13.0,
                [200, 205, 225, 255],
            );

            let scan_rect = Rectangle::new((wx + content_x + 380, wy + cy - 6).into(), (126, 28).into());
            canvas.fill_rounded_rect_aa(content_x + 380, cy - 6, 126, 28, 5, [48, 48, 72, 255]);
            canvas.draw_text_px(
                content_x + 394,
                cy,
                "Refresh Scan",
                12.5,
                [accent[0], accent[1], accent[2], 255],
            );
            state
                .settings
                .hit_regions
                .push((scan_rect, SettingsAction::RefreshWifi));
            cy += 34;

            if let Some((ref target_ssid, ref pw_buf)) = state.settings.wifi_password_prompt {
                canvas.fill_rounded_rect_aa(content_x, cy, 506, 46, 8, [accent[0], accent[1], accent[2], 36]);
                let title = format!("Password for {}:", target_ssid);
                canvas.draw_text_px(content_x + 12, cy + 6, &title, 11.5, [226, 232, 248, 255]);
                let masked = if pw_buf.is_empty() {
                    "Type password and press Enter...".to_string()
                } else {
                    "*".repeat(pw_buf.chars().count().min(28))
                };
                canvas.draw_text_px(content_x + 12, cy + 23, &masked, 12.5, [246, 248, 255, 255]);

                let ok_rect = Rectangle::new((wx + content_x + 344, wy + cy + 9).into(), (74, 28).into());
                canvas.fill_rounded_rect_aa(content_x + 344, cy + 9, 74, 28, 6, [accent[0], accent[1], accent[2], 235]);
                canvas.draw_text_raw(content_x + 356, cy + 15, "Join", 12.0, contrast_fg(accent));
                state.settings.hit_regions.push((ok_rect, SettingsAction::SubmitWifiPassword));

                let can_rect = Rectangle::new((wx + content_x + 424, wy + cy + 9).into(), (72, 28).into());
                canvas.fill_rounded_rect_aa(content_x + 424, cy + 9, 72, 28, 6, [64, 68, 88, 245]);
                canvas.draw_text_px(content_x + 436, cy + 15, "Cancel", 12.0, [240, 242, 252, 255]);
                state.settings.hit_regions.push((can_rect, SettingsAction::CancelWifiPassword));
                cy += 54;
            }

            if state.settings.wifi_networks.is_empty() {
                canvas.fill_rounded_rect_aa(content_x, cy, 506, 64, 6, [28, 28, 42, 240]);
                canvas.draw_text_px(
                    content_x + 14,
                    cy + 22,
                    "No Wi-Fi networks listed yet. Click 'Refresh Scan' above.",
                    13.0,
                    [165, 170, 195, 255],
                );
            } else {
                for (idx, net) in state.settings.wifi_networks.iter().take(6).enumerate() {
                    let row_y = cy + idx as i32 * 46;
                    let row_bg = if net.in_use {
                        [42, 52, 68, 250]
                    } else {
                        [30, 30, 46, 240]
                    };
                    canvas.fill_rounded_rect_aa(content_x, row_y, 506, 40, 6, row_bg);

                    let ssid_line = format!("{}  ({}%  /  {})", net.ssid, net.signal, net.security);
                    canvas.draw_text_px(
                        content_x + 14,
                        row_y + 11,
                        &ssid_line,
                        13.5,
                        [238, 240, 252, 255],
                    );

                    let btn_rect = Rectangle::new(
                        (wx + content_x + 396, wy + row_y + 6).into(),
                        (98, 28).into(),
                    );
                    let (btn_bg, btn_label) = if net.in_use {
                        ([175, 62, 86, 255], "Disconnect")
                    } else {
                        ([52, 86, 72, 255], "Connect")
                    };
                    canvas.fill_rounded_rect_aa(content_x + 396, row_y + 6, 98, 28, 5, btn_bg);
                    canvas.draw_text_px(
                        content_x + 410,
                        row_y + 11,
                        btn_label,
                        12.5,
                        [245, 246, 252, 255],
                    );
                    state
                        .settings
                        .hit_regions
                        .push((btn_rect, SettingsAction::ToggleWifi(idx)));
                }
            }
        }

        SettingsTab::Bluetooth => {
            canvas.draw_text_px(
                content_x,
                cy,
                "Bluetooth Adapter & Devices",
                15.0,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 30;

            let pwr_rect = Rectangle::new((wx + content_x, wy + cy).into(), (340, 34).into());
            let pwr_lbl = if state.settings.bt_powered {
                "Bluetooth Adapter Power:  Enabled"
            } else {
                "Bluetooth Adapter Power:  Disabled"
            };
            draw_setting_button(
                &mut canvas,
                content_x,
                cy,
                340,
                34,
                pwr_lbl,
                state.settings.bt_powered,
                accent,
            );
            state
                .settings
                .hit_regions
                .push((pwr_rect, SettingsAction::ToggleBluetoothPower));

            let scan_rect = Rectangle::new((wx + content_x + 356, wy + cy).into(), (150, 34).into());
            canvas.fill_rounded_rect_aa(content_x + 356, cy, 150, 34, 6, [48, 48, 72, 255]);
            canvas.draw_text_px(
                content_x + 376,
                cy + 9,
                "Scan Devices",
                13.0,
                [accent[0], accent[1], accent[2], 255],
            );
            state
                .settings
                .hit_regions
                .push((scan_rect, SettingsAction::RefreshBluetooth));
            cy += 46;

            canvas.draw_text_px(content_x, cy, &state.settings.bt_status, 13.0, [195, 200, 222, 255]);
            cy += 28;

            if state.settings.bt_devices.is_empty() {
                canvas.fill_rounded_rect_aa(content_x, cy, 506, 60, 6, [28, 28, 42, 240]);
                canvas.draw_text_px(
                    content_x + 14,
                    cy + 20,
                    "No paired Bluetooth devices found. Click 'Scan Devices' above.",
                    13.0,
                    [165, 170, 195, 255],
                );
            } else {
                for (idx, dev) in state.settings.bt_devices.iter().take(5).enumerate() {
                    let ry = cy + idx as i32 * 44;
                    canvas.fill_rounded_rect_aa(content_x, ry, 506, 38, 6, [30, 30, 46, 240]);
                    let line = format!("{}  ({})", dev.name, dev.mac);
                    canvas.draw_text_px(content_x + 14, ry + 10, &line, 13.0, [238, 240, 252, 255]);

                    let b_rect = Rectangle::new((wx + content_x + 396, wy + ry + 5).into(), (98, 28).into());
                    let b_lbl = if dev.connected { "Disconnect" } else { "Connect" };
                    canvas.fill_rounded_rect_aa(content_x + 396, ry + 5, 98, 28, 5, [48, 72, 86, 255]);
                    canvas.draw_text_px(content_x + 412, ry + 10, b_lbl, 12.0, [242, 244, 252, 255]);
                    state
                        .settings
                        .hit_regions
                        .push((b_rect, SettingsAction::ToggleBluetoothDevice(idx)));
                }
            }
        }

        SettingsTab::Sound => {
            canvas.draw_text_px(
                content_x,
                cy,
                "Sound, Master Output & Microphone",
                15.0,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 32;

            canvas.draw_text_px(
                content_x,
                cy,
                &format!("Active Audio Device: {}", state.settings.audio_sink_name),
                13.0,
                [195, 200, 222, 255],
            );
            cy += 30;

            // Master Output Volume Slider
            let out_lbl = format!(
                "Master Output Volume:  {}% {}",
                state.panel.volume_percent,
                if state.panel.volume_muted { "(MUTED)" } else { "" }
            );
            canvas.draw_text_px(content_x, cy, &out_lbl, 14.0, [238, 240, 252, 255]);
            cy += 24;

            let bar_w = 380;
            canvas.fill_rounded_rect_aa(content_x, cy + 8, bar_w, 12, 6, [28, 28, 42, 255]);
            let fill_w = ((state.panel.volume_percent as f32 / 100.0) * bar_w as f32).round() as i32;
            if fill_w > 2 {
                canvas.fill_rounded_rect_aa(
                    content_x,
                    cy + 8,
                    fill_w.max(6),
                    12,
                    6,
                    [accent[0], accent[1], accent[2], 255],
                );
            }
            let knob_x = (content_x + fill_w.clamp(6, bar_w - 6)) as f32;
            canvas.fill_circle_aa(knob_x, (cy + 14) as f32, 9.5, [accent[0], accent[1], accent[2], 235]);
            canvas.fill_circle_aa(knob_x, (cy + 14) as f32, 7.0, [248, 250, 255, 255]);
            let out_track = Rectangle::new((wx + content_x, wy + cy).into(), (bar_w, 28).into());
            state
                .settings
                .slider_tracks
                .push((out_track, SliderKind::OutputVol));

            let mute_rect = Rectangle::new((wx + content_x + 396, wy + cy).into(), (110, 28).into());
            let mute_lbl = if state.panel.volume_muted { "Unmute" } else { "Mute Out" };
            canvas.fill_rounded_rect_aa(content_x + 396, cy, 110, 28, 5, [48, 48, 72, 255]);
            canvas.draw_text_px(content_x + 414, cy + 6, mute_lbl, 12.5, [238, 240, 252, 255]);
            state
                .settings
                .hit_regions
                .push((mute_rect, SettingsAction::ToggleMuteOutput));
            cy += 50;

            // Microphone Input Volume Slider
            let mic_lbl = format!(
                "Microphone Input Level:  {}% {}",
                state.settings.mic_volume,
                if state.settings.mic_muted { "(MUTED)" } else { "" }
            );
            canvas.draw_text_px(content_x, cy, &mic_lbl, 14.0, [238, 240, 252, 255]);
            cy += 24;

            canvas.fill_rounded_rect_aa(content_x, cy + 8, bar_w, 12, 6, [28, 28, 42, 255]);
            let mfill_w = ((state.settings.mic_volume as f32 / 100.0) * bar_w as f32).round() as i32;
            if mfill_w > 2 {
                canvas.fill_rounded_rect_aa(
                    content_x,
                    cy + 8,
                    mfill_w.max(6),
                    12,
                    6,
                    [accent[0], accent[1], accent[2], 255],
                );
            }
            let mknob_x = (content_x + mfill_w.clamp(6, bar_w - 6)) as f32;
            canvas.fill_circle_aa(mknob_x, (cy + 14) as f32, 9.5, [accent[0], accent[1], accent[2], 235]);
            canvas.fill_circle_aa(mknob_x, (cy + 14) as f32, 7.0, [248, 250, 255, 255]);
            let mic_track = Rectangle::new((wx + content_x, wy + cy).into(), (bar_w, 28).into());
            state
                .settings
                .slider_tracks
                .push((mic_track, SliderKind::MicVol));

            let mmute_rect = Rectangle::new((wx + content_x + 396, wy + cy).into(), (110, 28).into());
            let mmute_lbl = if state.settings.mic_muted { "Unmute Mic" } else { "Mute Mic" };
            canvas.fill_rounded_rect_aa(content_x + 396, cy, 110, 28, 5, [48, 48, 72, 255]);
            canvas.draw_text_px(content_x + 412, cy + 6, mmute_lbl, 12.5, [238, 240, 252, 255]);
            state
                .settings
                .hit_regions
                .push((mmute_rect, SettingsAction::ToggleMuteMic));
            cy += 44;

            canvas.draw_text_px(content_x, cy, "Output Devices", 13.5, [222, 226, 244, 255]);
            cy += 22;
            if state.settings.audio_sinks.is_empty() {
                canvas.fill_rounded_rect_aa(content_x, cy, 506, 36, 7, [28, 28, 42, 240]);
                canvas.draw_text_px(
                    content_x + 12,
                    cy + 10,
                    "Using default PipeWire / PulseAudio sink",
                    12.5,
                    [168, 174, 200, 240],
                );
            } else {
                for sink in state.settings.audio_sinks.iter().take(3) {
                    let s_rect = Rectangle::new((wx + content_x, wy + cy).into(), (506, 32).into());
                    let bg = if sink.is_default {
                        [accent[0], accent[1], accent[2], 52]
                    } else if rect_contains(s_rect, pointer_x, pointer_y) {
                        [255, 255, 255, 24]
                    } else {
                        [28, 28, 42, 240]
                    };
                    canvas.fill_rounded_rect_aa(content_x, cy, 506, 32, 7, bg);
                    let lbl = truncate_with_ellipsis(&canvas, &sink.name, 12.5, 410);
                    canvas.draw_text_px(content_x + 12, cy + 8, &lbl, 12.5, [238, 240, 252, 255]);
                    if sink.is_default {
                        canvas.draw_text_px(content_x + 440, cy + 9, "Active", 11.5, [accent[0], accent[1], accent[2], 255]);
                    }
                    state
                        .settings
                        .hit_regions
                        .push((s_rect, SettingsAction::SelectAudioSink(sink.id.clone())));
                    cy += 36;
                }
            }
        }

        SettingsTab::MouseKeyboard => {
            canvas.draw_text_px(
                content_x,
                cy,
                "Mouse, Touchpad & Keyboard",
                15.0,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 30;

            let speed_label = format!(
                "Pointer Speed / Sensitivity:   {:.2}x",
                state.pointer_sensitivity
            );
            canvas.draw_text_px(content_x, cy, &speed_label, 14.0, [235, 238, 250, 255]);
            cy += 24;

            let dec_rect = Rectangle::new((wx + content_x, wy + cy).into(), (36, 30).into());
            canvas.fill_rounded_rect_aa(content_x, cy, 36, 30, 5, [44, 44, 66, 255]);
            canvas.draw_text_px(content_x + 14, cy + 6, "-", 15.0, [240, 242, 252, 255]);
            state
                .settings
                .hit_regions
                .push((dec_rect, SettingsAction::AdjustPointerSpeed(-0.15)));

            let bar_x = content_x + 46;
            let bar_w = 360;
            canvas.fill_rounded_rect_aa(bar_x, cy + 9, bar_w, 12, 6, [28, 28, 42, 255]);
            let norm = ((state.pointer_sensitivity - 0.25) / (2.50 - 0.25)).clamp(0.0, 1.0);
            let fill_w = (norm * bar_w as f64).round() as i32;
            if fill_w > 2 {
                canvas.fill_rounded_rect_aa(
                    bar_x,
                    cy + 9,
                    fill_w.max(6),
                    12,
                    6,
                    [accent[0], accent[1], accent[2], 255],
                );
            }
            let pknob_x = (bar_x + fill_w.clamp(6, bar_w - 6)) as f32;
            canvas.fill_circle_aa(pknob_x, (cy + 15) as f32, 9.5, [accent[0], accent[1], accent[2], 235]);
            canvas.fill_circle_aa(pknob_x, (cy + 15) as f32, 7.0, [248, 250, 255, 255]);
            let speed_track = Rectangle::new((wx + bar_x, wy + cy).into(), (bar_w, 30).into());
            state
                .settings
                .slider_tracks
                .push((speed_track, SliderKind::PointerSpeed));

            let inc_rect = Rectangle::new((wx + bar_x + bar_w + 10, wy + cy).into(), (36, 30).into());
            canvas.fill_rounded_rect_aa(bar_x + bar_w + 10, cy, 36, 30, 5, [44, 44, 66, 255]);
            canvas.draw_text_px(bar_x + bar_w + 22, cy + 6, "+", 15.0, [240, 242, 252, 255]);
            state
                .settings
                .hit_regions
                .push((inc_rect, SettingsAction::AdjustPointerSpeed(0.15)));
            cy += 42;

            let nat_rect = Rectangle::new((wx + content_x, wy + cy).into(), (506, 34).into());
            let nat_label = if state.natural_scroll {
                "Natural Scrolling:   Enabled"
            } else {
                "Natural Scrolling:   Disabled"
            };
            draw_setting_button(
                &mut canvas,
                content_x,
                cy,
                506,
                34,
                nat_label,
                state.natural_scroll,
                accent,
            );
            state
                .settings
                .hit_regions
                .push((nat_rect, SettingsAction::ToggleNaturalScroll));
            cy += 42;

            let accel_rect = Rectangle::new((wx + content_x, wy + cy).into(), (506, 34).into());
            let accel_label = if state.pointer_accel_disabled {
                "Mouse Acceleration:   Disabled (Flat / Raw)"
            } else {
                "Mouse Acceleration:   Enabled (Adaptive)"
            };
            draw_setting_button(
                &mut canvas,
                content_x,
                cy,
                506,
                34,
                accel_label,
                state.pointer_accel_disabled,
                accent,
            );
            state
                .settings
                .hit_regions
                .push((accel_rect, SettingsAction::TogglePointerAccel));
            cy += 42;

            let kb_rect = Rectangle::new((wx + content_x, wy + cy).into(), (506, 34).into());
            let kb_label = format!(
                "Keyboard XKB Layout:   '{}'   (click to cycle us / pl / de / fr)",
                state.settings.xkb_layout.to_uppercase()
            );
            draw_setting_button(&mut canvas, content_x, cy, 506, 34, &kb_label, true, accent);
            state
                .settings
                .hit_regions
                .push((kb_rect, SettingsAction::CycleKeyboardLayout));
            cy += 40;

            // Scroll Speed + Keyboard Repeat Delay / Rate (applied live to Smithay)
            let ss_lbl = format!("Scroll Speed: {:.2}x", state.panel.scroll_speed);
            canvas.draw_text_px(content_x, cy + 4, &ss_lbl, 12.0, [225, 228, 245, 255]);
            let ss_dec = Rectangle::new((wx + content_x + 170, wy + cy).into(), (32, 24).into());
            canvas.fill_rounded_rect_aa(content_x + 170, cy, 32, 24, 5, [44, 46, 68, 255]);
            canvas.draw_text_px(content_x + 182, cy + 4, "-", 13.0, [240, 242, 252, 255]);
            state.settings.hit_regions.push((ss_dec, SettingsAction::AdjustScrollSpeed(-0.25)));
            let ss_inc = Rectangle::new((wx + content_x + 208, wy + cy).into(), (32, 24).into());
            canvas.fill_rounded_rect_aa(content_x + 208, cy, 32, 24, 5, [44, 46, 68, 255]);
            canvas.draw_text_px(content_x + 219, cy + 4, "+", 13.0, [240, 242, 252, 255]);
            state.settings.hit_regions.push((ss_inc, SettingsAction::AdjustScrollSpeed(0.25)));

            let rp_lbl = format!("Repeat: {}ms / {}/s", state.panel.kb_repeat_delay, state.panel.kb_repeat_rate);
            canvas.draw_text_px(content_x + 252, cy + 4, &rp_lbl, 12.0, [225, 228, 245, 255]);
            let dl_dec = Rectangle::new((wx + content_x + 420, wy + cy).into(), (38, 24).into());
            canvas.fill_rounded_rect_aa(content_x + 420, cy, 38, 24, 5, [44, 46, 68, 255]);
            canvas.draw_text_px(content_x + 426, cy + 4, "-ms", 11.0, [240, 242, 252, 255]);
            state.settings.hit_regions.push((dl_dec, SettingsAction::AdjustKbRepeatDelay(-25)));
            let rt_inc = Rectangle::new((wx + content_x + 464, wy + cy).into(), (42, 24).into());
            canvas.fill_rounded_rect_aa(content_x + 464, cy, 42, 24, 5, [44, 46, 68, 255]);
            canvas.draw_text_px(content_x + 469, cy + 4, "+rate", 11.0, [240, 242, 252, 255]);
            state.settings.hit_regions.push((rt_inc, SettingsAction::AdjustKbRepeatRate(5)));
            cy += 34;

            canvas.draw_text_px(
                content_x,
                cy,
                "Detected Input Devices (libinput / sysfs):",
                13.0,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 22;
            for dev in state.settings.input_devices.iter().take(4) {
                canvas.fill_rounded_rect_aa(content_x, cy, 506, 25, 4, [28, 28, 42, 235]);
                canvas.draw_text_px(content_x + 10, cy + 5, dev, 12.0, [215, 218, 236, 255]);
                cy += 28;
            }
        }

        SettingsTab::DefaultApps => {
            canvas.draw_text_px(
                content_x,
                cy,
                "Startup & Default Applications",
                15.0,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 32;

            let term_rect = Rectangle::new((wx + content_x, wy + cy).into(), (506, 36).into());
            let term_lbl = format!(
                "Default Terminal (Super+Return):   {}   (click to cycle)",
                state.panel.default_terminal
            );
            draw_setting_button(&mut canvas, content_x, cy, 506, 36, &term_lbl, true, accent);
            state
                .settings
                .hit_regions
                .push((term_rect, SettingsAction::CycleDefaultTerminal));
            cy += 46;

            let br_rect = Rectangle::new((wx + content_x, wy + cy).into(), (506, 36).into());
            let br_lbl = format!(
                "Default Web Browser:   {}   (click to cycle)",
                state.panel.default_browser
            );
            draw_setting_button(&mut canvas, content_x, cy, 506, 36, &br_lbl, true, accent);
            state
                .settings
                .hit_regions
                .push((br_rect, SettingsAction::CycleDefaultBrowser));
            cy += 46;

            let fm_rect = Rectangle::new((wx + content_x, wy + cy).into(), (506, 36).into());
            let fm_lbl = format!(
                "Default File Manager:   {}   (click to cycle)",
                state.panel.default_file_manager
            );
            draw_setting_button(&mut canvas, content_x, cy, 506, 36, &fm_lbl, true, accent);
            state
                .settings
                .hit_regions
                .push((fm_rect, SettingsAction::CycleDefaultFileManager));
            cy += 52;

            let auto_rect = Rectangle::new((wx + content_x, wy + cy).into(), (506, 36).into());
            canvas.fill_rounded_rect_aa(content_x, cy, 506, 36, 6, [40, 40, 62, 255]);
            canvas.draw_text_px(
                content_x + 14,
                cy + 10,
                "Edit Session Autostart Script (~/.config/okay-de/autostart.sh)",
                13.0,
                [238, 240, 252, 255],
            );
            state
                .settings
                .hit_regions
                .push((auto_rect, SettingsAction::OpenAutostartFile));
            cy += 50;

            canvas.draw_text_px(
                content_x,
                cy,
                "Shortcuts: Super+Ret Terminal | Super+D Launcher | Super+I Settings | Super+L Lock",
                12.0,
                [160, 165, 190, 255],
            );
            cy += 22;
            canvas.draw_text_px(
                content_x,
                cy,
                "           Super+T Layout | Super+F Fullscreen | Super+A Pin | Super+V Clipboard",
                12.0,
                [160, 165, 190, 255],
            );
        }

        SettingsTab::SystemInfo => {
            canvas.draw_text_px(
                content_x,
                cy,
                "System Information",
                15.0,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 26;

            let outputs_summary: String = state
                .space
                .outputs()
                .map(|o| {
                    let geo = state.space.output_geometry(o).unwrap_or_default();
                    format!("{} ({}x{})", o.name(), geo.size.w, geo.size.h)
                })
                .collect::<Vec<_>>()
                .join(", ");

            let rows = [
                ("Desktop Environment", "OKay DE 0.2.0 (Smithay 0.7.0 / Wayland)"),
                ("Operating System", &state.settings.os_name),
                ("Linux Kernel", &state.settings.kernel_ver),
                ("Processor (CPU)", &state.settings.cpu_model),
                ("System Load", &state.settings.cpu_load),
                ("Graphics (GPU)", &state.settings.gpu_renderer),
                ("Memory (RAM)", &state.settings.ram_summary),
                ("Root Disk Usage", &state.settings.disk_summary),
                ("Active Monitors", &outputs_summary),
            ];

            for (label, val) in rows {
                let lines = wrap_text_two_lines(&canvas, val, 12.0, 336);
                let row_h = if lines.len() > 1 { 38 } else { 29 };
                canvas.fill_rounded_rect_aa(content_x, cy, 506, row_h, 6, [28, 28, 42, 240]);
                canvas.draw_text_px(
                    content_x + 12,
                    cy + 7,
                    label,
                    12.0,
                    [160, 165, 190, 255],
                );
                for (li, line_txt) in lines.iter().enumerate() {
                    canvas.draw_text_px(
                        content_x + 158,
                        cy + 6 + li as i32 * 15,
                        line_txt,
                        12.0,
                        [238, 240, 252, 255],
                    );
                }
                cy += row_h + 3;
            }

            // System Updates bar
            cy += 4;
            canvas.fill_rounded_rect_aa(content_x, cy, 506, 42, 8, [34, 38, 54, 245]);
            let upd_txt = truncate_with_ellipsis(&canvas, &state.settings.update_status, 12.0, 260);
            canvas.draw_text_px(content_x + 12, cy + 14, &upd_txt, 12.0, [226, 232, 248, 255]);

            let chk_rect = Rectangle::new((wx + content_x + 210, wy + cy + 7).into(), (92, 28).into());
            canvas.fill_rounded_rect_aa(content_x + 210, cy + 7, 92, 28, 6, [52, 56, 78, 255]);
            canvas.draw_text_px(content_x + 218, cy + 14, "Check Git", 11.5, [238, 240, 252, 255]);
            state
                .settings
                .hit_regions
                .push((chk_rect, SettingsAction::CheckUpdates));

            let cp_rect = Rectangle::new((wx + content_x + 308, wy + cy + 7).into(), (86, 28).into());
            canvas.fill_rounded_rect_aa(content_x + 308, cy + 7, 86, 28, 6, [52, 56, 78, 255]);
            canvas.draw_text_px(content_x + 318, cy + 14, "Copy Info", 11.5, [238, 240, 252, 255]);
            state
                .settings
                .hit_regions
                .push((cp_rect, SettingsAction::CopySystemInfo));

            let inst_rect = Rectangle::new((wx + content_x + 400, wy + cy + 7).into(), (96, 28).into());
            canvas.fill_rounded_rect_aa(
                content_x + 400,
                cy + 7,
                96,
                28,
                6,
                [accent[0], accent[1], accent[2], 235],
            );
            canvas.draw_text_raw(
                content_x + 414,
                cy + 14,
                "Update DE",
                11.5,
                contrast_fg(accent),
            );
            state
                .settings
                .hit_regions
                .push((inst_rect, SettingsAction::InstallUpdates));
        }

        SettingsTab::Shortcuts => {
            canvas.draw_text_px(
                content_x,
                cy,
                "Keyboard & Mouse Shortcuts",
                15.0,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 28;

            let bindings: &[(&str, &str)] = &[
                ("Super + Enter", "Open Terminal"),
                ("Super + D", "App Launcher"),
                ("Super + O", "Window Overview"),
                ("Super + I", "System Settings"),
                ("Super + V", "Clipboard History"),
                ("Super + L", "Lock Screen"),
                ("Super + S / PrtSc", "Full Screenshot"),
                ("Super+Shift+S", "Area Screenshot"),
                ("Super + Q", "Close Window"),
                ("Super + F", "Fullscreen"),
                ("Super + T", "Tiling Mode"),
                ("Super + A", "Always On Top"),
                ("Super + C", "Center Window"),
                ("Super + M", "Minimize Window"),
                ("Super+Shift+M", "Restore Window"),
                ("Super + [ / ]", "Master Ratio -/+"),
                ("Super + J / K", "Focus Next / Prev"),
                ("Super+Shift+J/K", "Swap Tiled Win"),
                ("Super + Arrows", "Snap Half / Corner"),
                ("Super + 1..9", "Switch Workspace"),
                ("Super+Shift+1..9", "Move Win to WS"),
                ("Alt/Super + Tab", "Switch Apps (+Shift)"),
            ];

            let col_w = 248;
            let row_h = 30;
            for (idx, (keys, desc)) in bindings.iter().enumerate() {
                let col = idx / 11;
                let row = idx % 11;
                let rx = content_x + col as i32 * (col_w + 10);
                let ry = cy + row as i32 * row_h;

                canvas.fill_rounded_rect_aa(rx, ry, col_w, row_h - 4, 7, [255, 255, 255, 14]);
                canvas.fill_rounded_rect_aa(
                    rx + 4,
                    ry + 3,
                    118,
                    row_h - 10,
                    5,
                    [accent[0], accent[1], accent[2], 48],
                );
                canvas.draw_text_px(
                    rx + 9,
                    ry + 7,
                    keys,
                    11.0,
                    [accent[0].saturating_add(40), accent[1].saturating_add(40), accent[2].saturating_add(40), 255],
                );
                canvas.draw_text_px(rx + 128, ry + 7, desc, 11.5, [232, 236, 248, 255]);
            }

            let tip_y = cy + 11 * row_h + 6;
            canvas.fill_rounded_rect_aa(content_x, tip_y, 506, 28, 7, [255, 255, 255, 10]);
            canvas.draw_text_px(
                content_x + 10,
                tip_y + 7,
                "Mouse: Super+LMB Drag | Super+RMB Resize | Title 2xClick Max | F2 Rename Icon",
                11.0,
                [172, 180, 206, 255],
            );
        }

        SettingsTab::AdShield => {
            canvas.draw_text_bold_px(
                content_x,
                cy,
                "OKay AdShield — System-Wide Ad & Tracker Blocker",
                15.0,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 28;

            // Main Toggle: System-Wide Ad & Tracker Blocker
            let main_rect = Rectangle::new((wx + content_x, wy + cy).into(), (506, 36).into());
            draw_setting_button(
                &mut canvas,
                content_x,
                cy,
                506,
                36,
                "System-Wide Ad & Tracker Blocker",
                state.adshield.enabled,
                accent,
            );
            state.settings.hit_regions.push((main_rect, SettingsAction::ToggleAdShield));
            cy += 44;

            // Stats & Update Blocklist row
            let stat_txt = format!(
                "Blocked: {}   |   Rules: {}   |   {}",
                state.adshield.blocked_count, state.adshield.rules_count, state.adshield.status_msg
            );
            let short_stat = truncate_with_ellipsis(&canvas, &stat_txt, 11.5, 326);
            canvas.fill_rounded_rect_aa(content_x, cy, 336, 32, 8, [255, 255, 255, 14]);
            canvas.draw_text_px(content_x + 10, cy + 8, &short_stat, 11.5, [216, 224, 246, 255]);

            let upd_rect = Rectangle::new((wx + content_x + 346, wy + cy).into(), (160, 32).into());
            let upd_lbl = if state.adshield.updating {
                "Updating..."
            } else {
                "Update Blocklist"
            };
            let upd_bg = if state.adshield.updating {
                [255, 255, 255, 24]
            } else {
                [accent[0], accent[1], accent[2], 215]
            };
            let upd_fg = if state.adshield.updating {
                [200, 206, 224, 230]
            } else {
                contrast_fg(accent)
            };
            canvas.fill_rounded_rect_aa(content_x + 346, cy, 160, 32, 8, upd_bg);
            let uw = canvas.measure_text_px(upd_lbl, 12.0);
            if state.adshield.updating {
                canvas.draw_text_bold_px(content_x + 346 + (160 - uw) / 2, cy + 8, upd_lbl, 12.0, upd_fg);
            } else {
                canvas.draw_text_bold_raw(content_x + 346 + (160 - uw) / 2, cy + 8, upd_lbl, 12.0, upd_fg);
            }
            state.settings.hit_regions.push((upd_rect, SettingsAction::UpdateAdShieldList));
            cy += 42;

            // Custom Whitelist / Blacklist Domain Input
            canvas.draw_text_px(content_x, cy, "Custom Exceptions & Rules (Whitelist / Blacklist)", 12.5, [215, 220, 238, 255]);
            cy += 20;
            let inp_rect = Rectangle::new((wx + content_x, wy + cy).into(), (280, 32).into());
            let inp_bg = if state.settings.adshield_input_focused {
                [36, 40, 58, 250]
            } else {
                [26, 28, 42, 235]
            };
            canvas.fill_rounded_rect_aa(content_x, cy, 280, 32, 8, inp_bg);
            if state.settings.adshield_input_focused {
                canvas.stroke_rounded_rect_aa(content_x, cy, 280, 32, 8, [accent[0], accent[1], accent[2], 220]);
            }
            let inp_txt = if state.settings.adshield_domain_input.is_empty() {
                if state.settings.adshield_input_focused {
                    "Type domain (e.g. ads.example.com)|".to_string()
                } else {
                    "Click to add domain...".to_string()
                }
            } else if state.settings.adshield_input_focused {
                format!("{}|", state.settings.adshield_domain_input)
            } else {
                state.settings.adshield_domain_input.clone()
            };
            let inp_fg = if state.settings.adshield_domain_input.is_empty() {
                [146, 154, 182, 220]
            } else {
                [244, 246, 255, 255]
            };
            let short_inp = truncate_with_ellipsis(&canvas, &inp_txt, 12.0, 260);
            canvas.draw_text_px(content_x + 10, cy + 8, &short_inp, 12.0, inp_fg);
            state.settings.hit_regions.push((inp_rect, SettingsAction::FocusAdShieldInput));

            let wl_btn = Rectangle::new((wx + content_x + 288, wy + cy).into(), (104, 32).into());
            canvas.fill_rounded_rect_aa(content_x + 288, cy, 104, 32, 8, [64, 178, 128, 210]);
            canvas.draw_text_bold_px(content_x + 300, cy + 8, "+ Whitelist", 11.5, [18, 24, 20, 255]);
            state.settings.hit_regions.push((wl_btn, SettingsAction::AddAdShieldWhitelist));

            let bl_btn = Rectangle::new((wx + content_x + 400, wy + cy).into(), (106, 32).into());
            canvas.fill_rounded_rect_aa(content_x + 400, cy, 106, 32, 8, [226, 86, 98, 215]);
            canvas.draw_text_bold_px(content_x + 412, cy + 8, "+ Blacklist", 11.5, [255, 246, 248, 255]);
            state.settings.hit_regions.push((bl_btn, SettingsAction::AddAdShieldBlacklist));
            cy += 40;

            // Active Whitelist & Blacklist pills (up to 3 each)
            if !state.adshield.whitelist.is_empty() || !state.adshield.blacklist.is_empty() {
                let mut px = content_x;
                for dom in state.adshield.whitelist.iter().take(3) {
                    let lbl = format!("WL: {} x", dom.chars().take(14).collect::<String>());
                    let pw = (canvas.measure_text_px(&lbl, 10.5) + 14).min(162);
                    if px + pw > content_x + 506 {
                        break;
                    }
                    let pr = Rectangle::new((wx + px, wy + cy).into(), (pw, 22).into());
                    canvas.fill_rounded_rect_aa(px, cy, pw, 22, 11, [64, 178, 128, 52]);
                    canvas.draw_text_px(px + 7, cy + 4, &lbl, 10.5, [156, 242, 198, 255]);
                    state.settings.hit_regions.push((pr, SettingsAction::RemoveAdShieldWhitelist(dom.clone())));
                    px += pw + 6;
                }
                for dom in state.adshield.blacklist.iter().take(3) {
                    let lbl = format!("BL: {} x", dom.chars().take(14).collect::<String>());
                    let pw = (canvas.measure_text_px(&lbl, 10.5) + 14).min(162);
                    if px + pw > content_x + 506 {
                        break;
                    }
                    let pr = Rectangle::new((wx + px, wy + cy).into(), (pw, 22).into());
                    canvas.fill_rounded_rect_aa(px, cy, pw, 22, 11, [226, 86, 98, 58]);
                    canvas.draw_text_px(px + 7, cy + 4, &lbl, 10.5, [255, 178, 186, 255]);
                    state.settings.hit_regions.push((pr, SettingsAction::RemoveAdShieldBlacklist(dom.clone())));
                    px += pw + 6;
                }
                cy += 28;
            }

            // Recent Blocked Domains list
            canvas.draw_text_px(content_x, cy, "Recently Blocked Domains", 12.5, [215, 220, 238, 255]);
            cy += 20;
            if state.adshield.recent_blocked.is_empty() {
                canvas.fill_rounded_rect_aa(content_x, cy, 506, 42, 10, [255, 255, 255, 10]);
                canvas.draw_text_px(
                    content_x + 12,
                    cy + 13,
                    "No blocked queries yet (local DNS proxy listening on 127.0.0.1:5353).",
                    11.5,
                    [156, 164, 190, 230],
                );
            } else {
                let max_rows = ((SETTINGS_HEIGHT - cy - 18) / 26).clamp(1, 8) as usize;
                for (dom, ts) in state.adshield.recent_blocked.iter().take(max_rows) {
                    canvas.fill_rounded_rect_aa(content_x, cy, 506, 23, 6, [255, 255, 255, 10]);
                    let short_dom = truncate_with_ellipsis(&canvas, dom, 11.5, 410);
                    canvas.draw_text_px(content_x + 10, cy + 4, &short_dom, 11.5, [238, 240, 252, 255]);
                    let tw = canvas.measure_text_px(ts, 11.0);
                    canvas.draw_text_px(content_x + 496 - tw, cy + 5, ts, 11.0, [154, 162, 190, 230]);
                    cy += 26;
                }
            }
        }
        SettingsTab::Tweaks => {
            canvas.draw_text_bold_px(
                content_x,
                cy,
                "Tweaks & Experimental Pro-User Controls",
                16.0,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 32;

            canvas.draw_text_px(content_x, cy, "UI Animation Speed Preset", 13.0, [215, 220, 238, 255]);
            cy += 22;
            let presets: [(&str, bool, f32); 3] = [
                ("Instant (Off)", false, 1.0),
                ("Fast (0.5x dur)", true, 2.0),
                ("Normal (1.0x)", true, 1.0),
            ];
            for (idx, (lbl, en, spd)) in presets.iter().enumerate() {
                let bx = content_x + idx as i32 * 172;
                let active = state.animations.enabled == *en
                    && (!*en || (state.animations.speed - *spd).abs() < 0.15);
                let r = Rectangle::new((wx + bx, wy + cy).into(), (162, 34).into());
                draw_setting_button(&mut canvas, bx, cy, 162, 34, lbl, active, accent);
                state.settings.hit_regions.push((r, SettingsAction::SetAnimPreset(*en, *spd)));
            }
            cy += 48;

            canvas.draw_text_px(content_x, cy, "Force Compositor Refresh Rate Target", 13.0, [215, 220, 238, 255]);
            cy += 22;
            let hz_opts: [(&str, u32); 4] = [
                ("Auto", 0),
                ("60 Hz", 60),
                ("144 Hz", 144),
                ("240 Hz", 240),
            ];
            for (idx, (lbl, hz)) in hz_opts.iter().enumerate() {
                let bx = content_x + idx as i32 * 128;
                let active = state.panel.target_refresh_hz == *hz;
                let r = Rectangle::new((wx + bx, wy + cy).into(), (120, 34).into());
                draw_setting_button(&mut canvas, bx, cy, 120, 34, lbl, active, accent);
                state.settings.hit_regions.push((r, SettingsAction::SetTargetRefreshHz(*hz)));
            }
            cy += 52;

            let rebuild_rect = Rectangle::new((wx + content_x, wy + cy).into(), (506, 32).into());
            draw_setting_button(
                &mut canvas,
                content_x,
                cy,
                506,
                32,
                "Rebuild Icon Cache (gtk-update-icon-cache) & Reload DE",
                true,
                accent,
            );
            state
                .settings
                .hit_regions
                .push((rebuild_rect, SettingsAction::RebuildIconCacheAndReload));
            cy += 40;

            // Window Corner Radius, Opacity & Focus / Titlebar Double-Click
            let rad_lbl = format!("Radius: {}px", state.panel.window_corner_radius);
            let rad_btn = Rectangle::new((wx + content_x, wy + cy).into(), (120, 28).into());
            draw_setting_button(&mut canvas, content_x, cy, 120, 28, &rad_lbl, false, accent);
            state.settings.hit_regions.push((rad_btn, SettingsAction::AdjustCornerRadius(if state.panel.window_corner_radius >= 24 { -24 } else { 4 })));

            let focus_lbl = if state.panel.focus_follows_mouse { "Focus: Hover" } else { "Focus: Click" };
            let focus_btn = Rectangle::new((wx + content_x + 128, wy + cy).into(), (122, 28).into());
            draw_setting_button(&mut canvas, content_x + 128, cy, 122, 28, focus_lbl, state.panel.focus_follows_mouse, accent);
            state.settings.hit_regions.push((focus_btn, SettingsAction::ToggleFocusFollowsMouse));

            let dbl_lbl = format!("DblClick: {}", state.panel.titlebar_dblclick_action.label());
            let dbl_btn = Rectangle::new((wx + content_x + 258, wy + cy).into(), (140, 28).into());
            draw_setting_button(&mut canvas, content_x + 258, cy, 140, 28, &dbl_lbl, false, accent);
            state.settings.hit_regions.push((dbl_btn, SettingsAction::CycleTitlebarDblClick));

            let gm_btn = Rectangle::new((wx + content_x + 406, wy + cy).into(), (100, 28).into());
            draw_setting_button(&mut canvas, content_x + 406, cy, 100, 28, "GameMode", state.panel.game_mode, accent);
            state.settings.hit_regions.push((gm_btn, SettingsAction::ToggleGameMode));
            cy += 36;

            // Hot Corners (TL / TR / BL / BR) + Power Profile
            let corner_names = ["TL", "TR", "BL", "BR"];
            for (ci, cname) in corner_names.iter().enumerate() {
                let bx = content_x + ci as i32 * 128;
                let lbl = format!("{}: {}", cname, state.panel.hot_corners[ci].label());
                let cr = Rectangle::new((wx + bx, wy + cy).into(), (120, 28).into());
                let active = state.panel.hot_corners[ci] != crate::panel::HotCornerAction::None;
                draw_setting_button(&mut canvas, bx, cy, 120, 28, &lbl, active, accent);
                state.settings.hit_regions.push((cr, SettingsAction::CycleHotCorner(ci)));
            }
            cy += 36;

            let pwr_opts: [(&str, &str); 3] = [
                ("Power: Perf", "performance"),
                ("Balanced", "balanced"),
                ("Power Saver", "power-saver"),
            ];
            for (pi, (plbl, pval)) in pwr_opts.iter().enumerate() {
                let bx = content_x + pi as i32 * 170;
                let pr = Rectangle::new((wx + bx, wy + cy).into(), (162, 28).into());
                draw_setting_button(&mut canvas, bx, cy, 162, 28, plbl, false, accent);
                state.settings.hit_regions.push((pr, SettingsAction::SetPowerProfile(pval)));
            }
            cy += 36;

            canvas.draw_text_px(
                content_x,
                cy,
                "Pro-User Shortcuts: Scroll Vol/Bri · MidClick Vol=Mute / Clock=DND · Ctrl+Scroll Dock · Super+V Clipboard",
                11.0,
                [160, 168, 194, 235],
            );
            cy += 22;
            let font_info = format!("Active Font: {} (TrueType anti-aliased)", active_font_name());
            canvas.draw_text_px(content_x, cy, &font_info, 11.5, [155, 160, 185, 255]);
        }
    }

    let buf = MemoryRenderBuffer::from_slice(
        &canvas.pixels,
        Fourcc::Abgr8888,
        (SETTINGS_WIDTH, SETTINGS_HEIGHT),
        1,
        Transform::Normal,
        None,
    );
    let local_pos = state.settings.position - output_geo.loc;
    let phys_loc: Point<f64, Physical> = local_pos.to_f64().to_physical(scale);

    match MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        phys_loc,
        &buf,
        None,
        None,
        Some((SETTINGS_WIDTH, SETTINGS_HEIGHT).into()),
        Kind::Unspecified,
    ) {
        Ok(elem) => {
            let mut out = vec![OkayRenderElements::Memory(elem)];
            if let Some(sh) = crate::render::panel_backdrop_element(
                state,
                renderer,
                output,
                scale,
                "settings",
                Rectangle::new(local_pos, (SETTINGS_WIDTH, SETTINGS_HEIGHT).into()),
                18.0,
                1.0,
            ) {
                out.push(sh);
            }
            out
        }
        Err(_) => Vec::new(),
    }
}

fn truncate_with_ellipsis(canvas: &Canvas, text: &str, px_size: f32, max_w: i32) -> String {
    if canvas.measure_text_px(text, px_size) <= max_w {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars() {
        let mut next = out.clone();
        next.push(ch);
        next.push_str("...");
        if canvas.measure_text_px(&next, px_size) > max_w {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}

fn wrap_text_two_lines(canvas: &Canvas, text: &str, px_size: f32, max_w: i32) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return vec![String::new()];
    }
    if canvas.measure_text_px(trimmed, px_size) <= max_w {
        return vec![trimmed.to_string()];
    }

    let mut line1 = String::new();
    let mut rest_start = 0;
    for (idx, word) in trimmed.split_inclusive(' ').enumerate() {
        let cand = format!("{}{}", line1, word);
        if idx > 0 && canvas.measure_text_px(cand.trim_end(), px_size) > max_w {
            break;
        }
        line1.push_str(word);
        rest_start += word.len();
    }

    let l1 = line1.trim_end();
    let l2 = trimmed[rest_start..].trim_start();
    if l1.is_empty() || l2.is_empty() {
        return vec![truncate_with_ellipsis(canvas, trimmed, px_size, max_w)];
    }
    vec![
        l1.to_string(),
        truncate_with_ellipsis(canvas, l2, px_size, max_w),
    ]
}

struct SettingsCanvas {
    inner: Canvas,
    pal: crate::panel::MaterialPalette,
}

impl std::ops::Deref for SettingsCanvas {
    type Target = Canvas;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl SettingsCanvas {
    fn new(w: i32, h: i32) -> Self {
        Self {
            inner: Canvas::new(w, h),
            pal: crate::panel::active_m3_palette(),
        }
    }

    fn adapt_bg(&self, c: [u8; 4]) -> [u8; 4] {
        if !self.pal.light_mode {
            return c;
        }
        if c[0] >= 245 && c[1] >= 245 && c[2] >= 245 && c[3] <= 90 {
            return [16, 22, 38, c[3].max(16)];
        }
        if c[0] < 75 && c[1] < 75 && c[2] < 95 && c[3] >= 180 {
            if c[0] >= 40 {
                return self.pal.control_bg();
            }
            return self.pal.card_bg();
        }
        c
    }

    fn adapt_fg(&self, c: [u8; 4]) -> [u8; 4] {
        if !self.pal.light_mode {
            return c;
        }
        if c[0] >= 205 && c[1] >= 205 && c[2] >= 210 {
            return self.pal.fg_primary();
        }
        if c[0] >= 130 && c[1] >= 135 && c[2] >= 150 && (c[2] as i32 - c[0] as i32).abs() <= 45 {
            return self.pal.fg_secondary();
        }
        c
    }

    fn fill_rounded_rect_aa(&mut self, x: i32, y: i32, w: i32, h: i32, radius: i32, color: [u8; 4]) {
        self.inner.fill_rounded_rect_aa(x, y, w, h, radius, self.adapt_bg(color));
    }

    fn stroke_rounded_rect_aa(&mut self, x: i32, y: i32, w: i32, h: i32, radius: i32, color: [u8; 4]) {
        self.inner.stroke_rounded_rect_aa(x, y, w, h, radius, self.adapt_bg(color));
    }

    fn fill_rect_blend(&mut self, x: i32, y: i32, w: i32, h: i32, color: [u8; 4]) {
        self.inner.fill_rect_blend(x, y, w, h, self.adapt_bg(color));
    }

    fn fill_circle_aa(&mut self, cx: f32, cy: f32, radius: f32, color: [u8; 4]) {
        self.inner.fill_circle_aa(cx, cy, radius, color);
    }

    fn draw_text_px(&mut self, x: i32, y: i32, text: &str, px_size: f32, color: [u8; 4]) {
        self.inner.draw_text_px(x, y, text, px_size, self.adapt_fg(color));
    }

    fn draw_text_bold_px(&mut self, x: i32, y: i32, text: &str, px_size: f32, color: [u8; 4]) {
        self.inner.draw_text_bold_px(x, y, text, px_size, self.adapt_fg(color));
    }

    fn draw_text_raw(&mut self, x: i32, y: i32, text: &str, px_size: f32, color: [u8; 4]) {
        self.inner.draw_text_px(x, y, text, px_size, color);
    }

    fn draw_text_bold_raw(&mut self, x: i32, y: i32, text: &str, px_size: f32, color: [u8; 4]) {
        self.inner.draw_text_bold_px(x, y, text, px_size, color);
    }
}

fn draw_setting_button(
    canvas: &mut SettingsCanvas,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    label: &str,
    active: bool,
    accent: [u8; 3],
) {
    // M3 Expressive / Light-aware grouped card with subtle 1px hairline outline
    canvas.fill_rounded_rect_aa(x, y, w, h, 10, [255, 255, 255, 20]);
    if w > 2 && h > 2 {
        canvas.fill_rounded_rect_aa(x + 1, y + 1, w - 2, h - 2, 9, [30, 33, 46, 235]);
    }
    // Pill-shaped animated toggle switch (40x20)
    let pill_w = 40;
    let pill_h = 20;
    let pill_x = x + w - pill_w - 10;
    let pill_y = y + (h - pill_h) / 2;
    crate::panel::draw_animated_toggle_pill(
        &mut canvas.inner,
        label,
        pill_x,
        pill_y,
        pill_w,
        pill_h,
        active,
        accent,
    );
    canvas.draw_text_px(x + 12, y + (h - 14) / 2, label, 13.0, [238, 240, 252, 255]);
}

fn read_os_release() -> String {
    if let Ok(content) = fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("PRETTY_NAME=") {
                return rest.trim_matches('"').to_string();
            }
        }
    }
    "Linux".to_string()
}

fn read_cpu_model() -> String {
    if let Ok(content) = fs::read_to_string("/proc/cpuinfo") {
        let mut model = None;
        let mut cores = 0;
        for line in content.lines() {
            if line.starts_with("processor") {
                cores += 1;
            } else if model.is_none() && line.starts_with("model name") {
                if let Some((_, v)) = line.split_once(':') {
                    model = Some(v.trim().to_string());
                }
            }
        }
        if let Some(m) = model {
            return format!("{} ({} threads)", m, cores.max(1));
        }
    }
    "Unknown CPU".to_string()
}

fn read_cpu_load() -> String {
    if let Ok(content) = fs::read_to_string("/proc/loadavg") {
        let parts: Vec<&str> = content.split_whitespace().take(3).collect();
        if parts.len() == 3 {
            return format!("Load avg: {} (1m), {} (5m), {} (15m)", parts[0], parts[1], parts[2]);
        }
    }
    "Normal".to_string()
}

fn read_ram_summary() -> String {
    if let Ok(content) = fs::read_to_string("/proc/meminfo") {
        let mut total_kb = 0u64;
        let mut avail_kb = 0u64;
        for line in content.lines() {
            if line.starts_with("MemTotal:") {
                total_kb = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            } else if line.starts_with("MemAvailable:") {
                avail_kb = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            }
        }
        if total_kb > 0 {
            let used_mib = total_kb.saturating_sub(avail_kb) / 1024;
            let total_mib = total_kb / 1024;
            let pct = (used_mib * 100) / total_mib.max(1);
            return format!("{} MiB / {} MiB ({}%)", used_mib, total_mib, pct);
        }
    }
    "Unknown RAM".to_string()
}

fn read_disk_summary() -> String {
    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        let Ok(root) = std::ffi::CString::new("/") else {
            return "Unavailable".to_string();
        };
        if libc::statvfs(root.as_ptr(), &mut stat) == 0 && stat.f_blocks > 0 {
            let total_gb = (stat.f_blocks * stat.f_frsize) / (1024 * 1024 * 1024);
            let avail_gb = (stat.f_bavail * stat.f_frsize) / (1024 * 1024 * 1024);
            let used_gb = total_gb.saturating_sub(avail_gb);
            return format!("{} GiB used / {} GiB total", used_gb, total_gb);
        }
    }
    " Mounted /".to_string()
}

fn read_gpu_name() -> String {
    if let Ok(entries) = fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let uevent = entry.path().join("device/uevent");
            if let Ok(text) = fs::read_to_string(uevent) {
                for line in text.lines() {
                    if let Some(drv) = line.strip_prefix("DRIVER=") {
                        return format!("DRM/KMS ({}) + Smithay GlesRenderer", drv);
                    }
                }
            }
        }
    }
    "Smithay GlesRenderer (EGL / OpenGL ES)".to_string()
}

fn read_input_devices() -> Vec<String> {
    let mut list = Vec::new();
    if let Ok(content) = fs::read_to_string("/proc/bus/input/devices") {
        for line in content.lines() {
            if let Some(name) = line.strip_prefix("N: Name=") {
                let clean = name.trim_matches('"').trim();
                if !clean.is_empty()
                    && !clean.contains("Power Button")
                    && !clean.contains("Video Bus")
                    && !clean.contains("Sleep Button")
                    && !list.iter().any(|x| x == clean)
                {
                    list.push(clean.to_string());
                }
            }
        }
    }
    if list.is_empty() {
        list.push("Standard Wayland / libinput Pointer & Keyboard".to_string());
    }
    list
}
