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
}

impl SettingsTab {
    const ALL: [Self; 9] = [
        Self::Appearance,
        Self::Displays,
        Self::Network,
        Self::Bluetooth,
        Self::Sound,
        Self::MouseKeyboard,
        Self::DefaultApps,
        Self::SystemInfo,
        Self::Shortcuts,
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
    CycleKeyboardLayout,
    CycleDefaultTerminal,
    CycleDefaultBrowser,
    CycleDefaultFileManager,
    OpenAutostartFile,
}

#[derive(Debug)]
pub struct SettingsState {
    pub open: bool,
    pub active_tab: SettingsTab,
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
    hit_regions: Vec<(Rectangle<i32, Logical>, SettingsAction)>,
}

impl Default for SettingsState {
    fn default() -> Self {
        let mut s = Self {
            open: false,
            active_tab: SettingsTab::Appearance,
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
            hit_regions: Vec::new(),
        };
        let snap = collect_system_info_snapshot();
        s.apply_system_info_snapshot(snap);
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
        let min_y = crate::panel::PANEL_HEIGHT + 4;
        state.settings.position = Point::from((nx.max(8), ny.max(min_y)));
        return true;
    }
    false
}

/// Releases any active settings window or slider drag on mouse button release.
pub fn handle_settings_release(state: &mut OkayState) -> bool {
    if let Some((kind, _)) = state.settings.dragging_slider.take() {
        if kind == SliderKind::PointerSpeed {
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
                state.panel.m3_expressive = !state.panel.m3_expressive;
                save_all_prefs(state);
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

    let mut canvas = Canvas::new(SETTINGS_WIDTH, SETTINGS_HEIGHT);
    canvas.fill_glass_panel(0, 0, SETTINGS_WIDTH, SETTINGS_HEIGHT, 18);

    // Left macOS System Settings Sidebar (translucent tinted panel + 1px divider)
    canvas.fill_rounded_rect_aa(2, 2, SIDEBAR_WIDTH, SETTINGS_HEIGHT - 4, 12, [255, 255, 255, 10]);
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
        canvas.draw_text_px(17, 13, "x", 11.0, [24, 18, 18, 220]);
    }
    state
        .settings
        .hit_regions
        .push((close_rect, SettingsAction::CloseWindow));

    canvas.draw_text_px(38, 13, "System Settings", 14.0, [242, 244, 252, 255]);

    for (i, tab) in SettingsTab::ALL.iter().enumerate() {
        let ty = HEADER_HEIGHT + 10 + i as i32 * 38;
        let tab_rect = Rectangle::new(
            (wx + 10, wy + ty).into(),
            (SIDEBAR_WIDTH - 16, 32).into(),
        );
        let is_selected = state.settings.active_tab == *tab;
        let hovered = rect_contains(tab_rect, pointer_x, pointer_y);
        let bg = if is_selected {
            [accent[0], accent[1], accent[2], 235]
        } else if hovered {
            [255, 255, 255, 24]
        } else {
            [255, 255, 255, 8]
        };
        let fg = if is_selected {
            contrast_fg(accent)
        } else {
            [232, 234, 246, 255]
        };
        canvas.fill_rounded_rect_aa(10, ty, SIDEBAR_WIDTH - 16, 32, 8, bg);
        canvas.draw_text_px(20, ty + 8, tab.title(), 13.0, fg);
        state
            .settings
            .hit_regions
            .push((tab_rect, SettingsAction::SelectTab(*tab)));
    }

    let content_x = SIDEBAR_WIDTH + 20;
    let mut cy = HEADER_HEIGHT + 14;

    match state.settings.active_tab {
        SettingsTab::Appearance => {
            canvas.draw_text_px(
                content_x,
                cy,
                "Appearance, Dock & Animations",
                15.0,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 28;

            // Row 1: 3 balanced Dock / Panel / Desktop toggles (162px each, 10px gap = 506px)
            let pos_rect = Rectangle::new((wx + content_x, wy + cy).into(), (162, 34).into());
            let pos_lbl = if state.panel.panel_top { "Top Bar" } else { "Bottom Bar" };
            draw_setting_button(&mut canvas, content_x, cy, 162, 34, pos_lbl, state.panel.panel_top, accent);
            state.settings.hit_regions.push((pos_rect, SettingsAction::TogglePanelPosition));

            let ws_rect = Rectangle::new((wx + content_x + 172, wy + cy).into(), (162, 34).into());
            draw_setting_button(&mut canvas, content_x + 172, cy, 162, 34, "Workspaces", state.panel.show_workspaces, accent);
            state.settings.hit_regions.push((ws_rect, SettingsAction::ToggleShowWorkspaces));

            let ic_rect = Rectangle::new((wx + content_x + 344, wy + cy).into(), (162, 34).into());
            draw_setting_button(&mut canvas, content_x + 344, cy, 162, 34, "Desk Icons", state.overlays.show_desktop_icons, accent);
            state.settings.hit_regions.push((ic_rect, SettingsAction::ToggleDesktopIcons));
            cy += 44;

            // Row 3: Minimize Animation Effect Selector (Magic Lamp Genie / ScaleGlide / Off)
            let min_rect = Rectangle::new((wx + content_x, wy + cy).into(), (506, 34).into());
            let min_lbl = format!(
                "Minimize Animation Effect:   {}   (click to cycle)",
                state.animations.minimize_effect.label()
            );
            draw_setting_button(
                &mut canvas,
                content_x,
                cy,
                506,
                34,
                &min_lbl,
                state.animations.enabled,
                accent,
            );
            state
                .settings
                .hit_regions
                .push((min_rect, SettingsAction::CycleMinimizeEffect));
            cy += 40;

            // Row 4: Animations Toggle + Speed Slider
            let anim_rect = Rectangle::new((wx + content_x, wy + cy).into(), (230, 32).into());
            let anim_lbl = if state.animations.enabled { "60+ FPS Animations: ON" } else { "Animations: OFF" };
            draw_setting_button(&mut canvas, content_x, cy, 230, 32, anim_lbl, state.animations.enabled, accent);
            state.settings.hit_regions.push((anim_rect, SettingsAction::ToggleAnimations));

            let spd_txt = format!("Speed: {:.2}x", state.animations.speed);
            canvas.draw_text_px(content_x + 244, cy + 8, &spd_txt, 13.0, [225, 228, 245, 255]);
            let s_dec = Rectangle::new((wx + content_x + 365, wy + cy).into(), (32, 32).into());
            canvas.fill_rounded_rect_aa(content_x + 365, cy, 32, 32, 5, [44, 44, 66, 255]);
            canvas.draw_text_px(content_x + 377, cy + 7, "-", 14.0, [240, 242, 252, 255]);
            state.settings.hit_regions.push((s_dec, SettingsAction::AdjustAnimSpeed(-0.25)));

            let s_inc = Rectangle::new((wx + content_x + 405, wy + cy).into(), (32, 32).into());
            canvas.fill_rounded_rect_aa(content_x + 405, cy, 32, 32, 5, [44, 44, 66, 255]);
            canvas.draw_text_px(content_x + 416, cy + 7, "+", 14.0, [240, 242, 252, 255]);
            state.settings.hit_regions.push((s_inc, SettingsAction::AdjustAnimSpeed(0.25)));
            cy += 40;

            // Material You Dynamic Wallpaper Accent + M3 Expressive Mode
            let dyn_rect = Rectangle::new((wx + content_x, wy + cy).into(), (248, 32).into());
            draw_setting_button(
                &mut canvas,
                content_x,
                cy,
                248,
                32,
                "Dynamic (From Wallpaper)",
                state.panel.dynamic_accent,
                accent,
            );
            state
                .settings
                .hit_regions
                .push((dyn_rect, SettingsAction::ToggleDynamicAccent));

            let exp_rect = Rectangle::new((wx + content_x + 258, wy + cy).into(), (248, 32).into());
            draw_setting_button(
                &mut canvas,
                content_x + 258,
                cy,
                248,
                32,
                "M3 Expressive Surfaces",
                state.panel.m3_expressive,
                accent,
            );
            state
                .settings
                .hit_regions
                .push((exp_rect, SettingsAction::ToggleM3Expressive));
            cy += 38;

            // Row 6: Circular Accent Color Swatch Dots with Active Ring
            canvas.draw_text_px(content_x, cy, "Accent Color", 13.0, [215, 220, 238, 255]);
            cy += 20;
            canvas.fill_rounded_rect_aa(content_x, cy - 4, 506, 38, 10, [255, 255, 255, 16]);
            for (idx, (name, rgb, _)) in ACCENT_PRESETS.iter().enumerate() {
                let bx = content_x + 14 + idx as i32 * 82;
                let preset_rect = Rectangle::new((wx + bx - 4, wy + cy).into(), (78, 30).into());
                let dot_cx = (bx + 12) as f32;
                let dot_cy = (cy + 15) as f32;
                if !state.panel.dynamic_accent && state.panel.accent_preset == idx {
                    canvas.fill_circle_aa(dot_cx, dot_cy, 11.5, [255, 255, 255, 235]);
                    canvas.fill_circle_aa(dot_cx, dot_cy, 9.5, [24, 26, 36, 255]);
                }
                canvas.fill_circle_aa(dot_cx, dot_cy, 8.0, [rgb[0], rgb[1], rgb[2], 255]);
                canvas.draw_text_px(bx + 26, cy + 7, name, 11.5, [232, 236, 248, 255]);
                state.settings.hit_regions.push((preset_rect, SettingsAction::SelectAccent(idx)));
            }
            cy += 40;

            // Row 7: Reload Wallpaper + Font Info
            let wp_rect = Rectangle::new((wx + content_x, wy + cy).into(), (506, 30).into());
            canvas.fill_rounded_rect_aa(content_x, cy, 506, 30, 6, [38, 38, 58, 255]);
            canvas.draw_text_px(
                content_x + 14,
                cy + 7,
                "Reload Wallpaper (~/.config/okay-de/wallpaper.png)",
                12.5,
                [235, 238, 250, 255],
            );
            state.settings.hit_regions.push((wp_rect, SettingsAction::ReloadWallpaper));
            cy += 34;

            let font_info = format!("Active Font: {} (TrueType anti-aliased)", active_font_name());
            canvas.draw_text_px(content_x, cy, &font_info, 12.0, [155, 160, 185, 255]);
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
            }
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
                canvas.draw_text_px(content_x + 356, cy + 15, "Join", 12.0, contrast_fg(accent));
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
            cy += 44;

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
                let row_h = if lines.len() > 1 { 42 } else { 31 };
                canvas.fill_rounded_rect_aa(content_x, cy, 506, row_h, 6, [28, 28, 42, 240]);
                canvas.draw_text_px(
                    content_x + 12,
                    cy + 8,
                    label,
                    12.0,
                    [160, 165, 190, 255],
                );
                for (li, line_txt) in lines.iter().enumerate() {
                    canvas.draw_text_px(
                        content_x + 158,
                        cy + 7 + li as i32 * 16,
                        line_txt,
                        12.0,
                        [238, 240, 252, 255],
                    );
                }
                cy += row_h + 4;
            }
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
        Ok(elem) => vec![OkayRenderElements::Memory(elem)],
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

fn draw_setting_button(
    canvas: &mut Canvas,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    label: &str,
    active: bool,
    accent: [u8; 3],
) {
    // macOS grouped card: rgba(255, 255, 255, 0.06)
    canvas.fill_rounded_rect_aa(x, y, w, h, 10, [255, 255, 255, 16]);
    // Pill-shaped macOS toggle switch (40x20)
    let pill_w = 40;
    let pill_h = 20;
    let pill_x = x + w - pill_w - 10;
    let pill_y = y + (h - pill_h) / 2;
    let pill_bg = if active {
        [accent[0], accent[1], accent[2], 245]
    } else {
        [255, 255, 255, 42]
    };
    canvas.fill_rounded_rect_aa(pill_x, pill_y, pill_w, pill_h, 10, pill_bg);
    let knob_cx = if active {
        (pill_x + pill_w - 10) as f32
    } else {
        (pill_x + 10) as f32
    };
    canvas.fill_circle_aa(knob_cx, (pill_y + pill_h / 2) as f32, 7.5, [250, 252, 255, 255]);
    canvas.draw_text_px(x + 12, y + (h - 14) / 2, label, 12.5, [238, 240, 252, 255]);
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
