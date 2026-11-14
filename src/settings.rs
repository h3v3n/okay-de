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
}

impl SettingsTab {
    const ALL: [Self; 8] = [
        Self::Appearance,
        Self::Displays,
        Self::Network,
        Self::Bluetooth,
        Self::Sound,
        Self::MouseKeyboard,
        Self::DefaultApps,
        Self::SystemInfo,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Appearance => "1. Appearance & Anim",
            Self::Displays => "2. Displays (Ekrany)",
            Self::Network => "3. Wi-Fi & Network",
            Self::Bluetooth => "4. Bluetooth",
            Self::Sound => "5. Sound (Dzwiek)",
            Self::MouseKeyboard => "6. Mouse & Keyboard",
            Self::DefaultApps => "7. Default Apps",
            Self::SystemInfo => "8. System Info",
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
enum SettingsAction {
    CloseWindow,
    SelectTab(SettingsTab),
    ToggleDockMode,
    TogglePanelPosition,
    ToggleShowWorkspaces,
    ToggleDesktopIcons,
    SelectAccent(usize),
    AdjustWindowGaps(i32),
    AdjustBorderWidth(i32),
    ToggleAnimations,
    AdjustAnimSpeed(f32),
    CycleMinimizeEffect,
    ReloadWallpaper,
    SetDisplayScale(usize, f64),
    SwapMonitorsOrder,
    RefreshWifi,
    ToggleWifi(usize),
    ToggleBluetoothPower,
    RefreshBluetooth,
    ToggleBluetoothDevice(usize),
    SetVolume(i32),
    ToggleMuteOutput,
    SetMicVolume(i32),
    ToggleMuteMic,
    SetPointerSpeed(f64),
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
    pub wifi_networks: Vec<WifiNetwork>,
    pub wifi_status: String,
    pub bt_powered: bool,
    pub bt_status: String,
    pub bt_devices: Vec<BluetoothDevice>,
    pub mic_volume: i32,
    pub mic_muted: bool,
    pub audio_sink_name: String,
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
            wifi_networks: Vec::new(),
            wifi_status: "Click 'Refresh Scan' to query NetworkManager".to_string(),
            bt_powered: true,
            bt_status: "Bluetooth Ready (click Scan Devices)".to_string(),
            bt_devices: Vec::new(),
            mic_volume: 80,
            mic_muted: false,
            audio_sink_name: "PipeWire / PulseAudio Default Output".to_string(),
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
        s.refresh_system_info();
        s
    }
}

impl SettingsState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn refresh_system_info(&mut self) {
        self.os_name = read_os_release();
        self.kernel_ver = fs::read_to_string("/proc/sys/kernel/osrelease")
            .unwrap_or_else(|_| "Linux".to_string())
            .trim()
            .to_string();
        self.cpu_model = read_cpu_model();
        self.cpu_load = read_cpu_load();
        self.ram_summary = read_ram_summary();
        self.disk_summary = read_disk_summary();
        self.gpu_renderer = read_gpu_name();
        self.input_devices = read_input_devices();
    }

    pub fn refresh_wifi(&mut self, force_rescan: bool) {
        let rescan_arg = if force_rescan { "yes" } else { "no" };
        let output = Command::new("nmcli")
            .args([
                "-t",
                "-f",
                "IN-USE,SSID,SIGNAL,SECURITY",
                "dev",
                "wifi",
                "list",
                "--rescan",
                rescan_arg,
            ])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);
                let mut nets = Vec::new();
                let mut active_ssid = None;
                for line in text.lines() {
                    let parts: Vec<&str> = line.splitn(4, ':').collect();
                    if parts.len() >= 4 {
                        let in_use = parts[0].trim() == "*";
                        let ssid = parts[1].trim();
                        if ssid.is_empty() {
                            continue;
                        }
                        if in_use {
                            active_ssid = Some(ssid.to_string());
                        }
                        if nets.iter().any(|n: &WifiNetwork| n.ssid == ssid) {
                            continue;
                        }
                        nets.push(WifiNetwork {
                            in_use,
                            ssid: ssid.to_string(),
                            signal: parts[2].trim().to_string(),
                            security: if parts[3].trim().is_empty() {
                                "Open".to_string()
                            } else {
                                parts[3].trim().to_string()
                            },
                        });
                        if nets.len() >= 6 {
                            break;
                        }
                    }
                }
                self.wifi_status = match active_ssid {
                    Some(s) => format!("Connected to {}", s),
                    None => format!("Found {} Wi-Fi networks", nets.len()),
                };
                self.wifi_networks = nets;
            }
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr);
                self.wifi_status = format!("nmcli: {}", err.lines().next().unwrap_or("unavailable"));
            }
            Err(_) => {
                self.wifi_status = "NetworkManager (nmcli) not installed".to_string();
            }
        }
    }

    pub fn refresh_bluetooth(&mut self) {
        let out = Command::new("bluetoothctl").arg("devices").output();
        match out {
            Ok(res) if res.status.success() => {
                let text = String::from_utf8_lossy(&res.stdout);
                let mut devs = Vec::new();
                for line in text.lines() {
                    // Format: `Device XX:XX:XX:XX:XX:XX Name`
                    let mut parts = line.splitn(3, ' ');
                    if parts.next() == Some("Device") {
                        if let (Some(mac), Some(name)) = (parts.next(), parts.next()) {
                            devs.push(BluetoothDevice {
                                mac: mac.to_string(),
                                name: name.trim().to_string(),
                                connected: false,
                            });
                        }
                    }
                }
                self.bt_status = format!("Found {} Bluetooth devices", devs.len());
                self.bt_devices = devs;
            }
            _ => {
                self.bt_status = "bluetoothctl service not active or unavailable".to_string();
            }
        }
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

/// Handles pointer motion when the settings window header is being dragged.
pub fn handle_settings_motion(state: &mut OkayState, pos: Point<f64, Logical>) -> bool {
    if let Some(offset) = state.settings.dragging {
        let nx = (pos.x - offset.x).round() as i32;
        let ny = (pos.y - offset.y).round() as i32;
        state.settings.position = Point::from((nx.max(8), ny.max(8)));
        return true;
    }
    false
}

/// Releases any active settings window drag on mouse button release.
pub fn handle_settings_release(state: &mut OkayState) -> bool {
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
                match tab {
                    SettingsTab::Network if state.settings.wifi_networks.is_empty() => {
                        state.settings.refresh_wifi(false);
                    }
                    SettingsTab::Bluetooth if state.settings.bt_devices.is_empty() => {
                        state.settings.refresh_bluetooth();
                    }
                    SettingsTab::SystemInfo => {
                        state.settings.refresh_system_info();
                    }
                    _ => {}
                }
            }
            SettingsAction::ToggleDockMode => {
                state.panel.dock_mode = !state.panel.dock_mode;
                save_all_prefs(state);
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
                state.panel.accent_preset = idx % ACCENT_PRESETS.len();
                state.theme.active_border_color = state.panel.accent_f32();
                save_all_prefs(state);
            }
            SettingsAction::AdjustWindowGaps(delta) => {
                state.panel.window_gaps = (state.panel.window_gaps + delta).clamp(0, 28);
                state.retile_workspace();
                save_all_prefs(state);
            }
            SettingsAction::AdjustBorderWidth(delta) => {
                state.theme.border_width = (state.theme.border_width + delta).clamp(1, 6);
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
                    state.retile_workspace();
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
                        let _ = Command::new("nmcli")
                            .args(["dev", "wifi", "disconnect"])
                            .spawn();
                        state.settings.wifi_status = format!("Disconnecting from {}...", net.ssid);
                    } else {
                        let _ = Command::new("nmcli")
                            .args(["dev", "wifi", "connect", &net.ssid])
                            .spawn();
                        state.settings.wifi_status = format!("Connecting to {}...", net.ssid);
                    }
                }
            }
            SettingsAction::ToggleBluetoothPower => {
                state.settings.bt_powered = !state.settings.bt_powered;
                let arg = if state.settings.bt_powered { "on" } else { "off" };
                let _ = Command::new("bluetoothctl").args(["power", arg]).spawn();
                state.settings.bt_status = format!("Bluetooth power {}", arg);
            }
            SettingsAction::RefreshBluetooth => {
                state.settings.refresh_bluetooth();
            }
            SettingsAction::ToggleBluetoothDevice(idx) => {
                if let Some(dev) = state.settings.bt_devices.get_mut(idx) {
                    let sub = if dev.connected { "disconnect" } else { "connect" };
                    let _ = Command::new("bluetoothctl").args([sub, &dev.mac]).spawn();
                    dev.connected = !dev.connected;
                    state.settings.bt_status = format!("{} {}...", sub, dev.name);
                }
            }
            SettingsAction::SetVolume(vol) => {
                state.panel.set_volume(vol);
            }
            SettingsAction::ToggleMuteOutput => {
                state.panel.toggle_mute();
            }
            SettingsAction::SetMicVolume(vol) => {
                state.settings.mic_volume = vol.clamp(0, 100);
                let pct = format!("{}%", state.settings.mic_volume);
                let _ = Command::new("wpctl")
                    .args(["set-volume", "@DEFAULT_AUDIO_SOURCE@", &pct])
                    .spawn();
            }
            SettingsAction::ToggleMuteMic => {
                state.settings.mic_muted = !state.settings.mic_muted;
                let _ = Command::new("wpctl")
                    .args(["set-mute", "@DEFAULT_AUDIO_SOURCE@", "toggle"])
                    .spawn();
            }
            SettingsAction::SetPointerSpeed(speed) => {
                state.pointer_sensitivity = speed.clamp(0.25, 2.50);
                save_all_prefs(state);
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
                    let _ = Command::new("xdg-open")
                        .arg(&path)
                        .env("WAYLAND_DISPLAY", &state.socket_name)
                        .spawn();
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
    let accent = state.panel.accent_rgb();
    let wx = state.settings.position.x;
    let wy = state.settings.position.y;
    let pointer_x = state.pointer_location.x.round() as i32;
    let pointer_y = state.pointer_location.y.round() as i32;

    let mut canvas = Canvas::new(SETTINGS_WIDTH, SETTINGS_HEIGHT);
    canvas.fill_glass_panel(0, 0, SETTINGS_WIDTH, SETTINGS_HEIGHT, 14);

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
        let ty = HEADER_HEIGHT + 10 + i as i32 * 40;
        let tab_rect = Rectangle::new(
            (wx + 10, wy + ty).into(),
            (SIDEBAR_WIDTH - 16, 34).into(),
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
        canvas.fill_rounded_rect_aa(10, ty, SIDEBAR_WIDTH - 16, 34, 8, bg);
        canvas.draw_text_px(20, ty + 9, tab.title(), 13.0, fg);
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
                "Appearance, Taskbar & Animations (Wyglad i animacje)",
                15.0,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 26;

            // Row 1: Dock Mode + Panel Position
            let dock_rect = Rectangle::new((wx + content_x, wy + cy).into(), (248, 32).into());
            let dock_lbl = if state.panel.dock_mode { "Style: Floating Dock" } else { "Style: Full Taskbar" };
            draw_setting_button(&mut canvas, content_x, cy, 248, 32, dock_lbl, state.panel.dock_mode, accent);
            state.settings.hit_regions.push((dock_rect, SettingsAction::ToggleDockMode));

            let pos_rect = Rectangle::new((wx + content_x + 258, wy + cy).into(), (248, 32).into());
            let pos_lbl = if state.panel.panel_top { "Panel Edge: Top" } else { "Panel Edge: Bottom" };
            draw_setting_button(&mut canvas, content_x + 258, cy, 248, 32, pos_lbl, state.panel.panel_top, accent);
            state.settings.hit_regions.push((pos_rect, SettingsAction::TogglePanelPosition));
            cy += 38;

            // Row 2: Show Workspaces + Desktop Icons
            let ws_rect = Rectangle::new((wx + content_x, wy + cy).into(), (248, 32).into());
            let ws_lbl = if state.panel.show_workspaces { "Workspaces (1..9): Shown" } else { "Workspaces (1..9): Hidden" };
            draw_setting_button(&mut canvas, content_x, cy, 248, 32, ws_lbl, state.panel.show_workspaces, accent);
            state.settings.hit_regions.push((ws_rect, SettingsAction::ToggleShowWorkspaces));

            let ic_rect = Rectangle::new((wx + content_x + 258, wy + cy).into(), (248, 32).into());
            let ic_lbl = if state.overlays.show_desktop_icons { "Desktop Icons: Shown" } else { "Desktop Icons: Hidden" };
            draw_setting_button(&mut canvas, content_x + 258, cy, 248, 32, ic_lbl, state.overlays.show_desktop_icons, accent);
            state.settings.hit_regions.push((ic_rect, SettingsAction::ToggleDesktopIcons));
            cy += 40;

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
            canvas.fill_rounded_rect(content_x + 365, cy, 32, 32, 5, [44, 44, 66, 255]);
            canvas.draw_text_px(content_x + 377, cy + 7, "-", 14.0, [240, 242, 252, 255]);
            state.settings.hit_regions.push((s_dec, SettingsAction::AdjustAnimSpeed(-0.25)));

            let s_inc = Rectangle::new((wx + content_x + 405, wy + cy).into(), (32, 32).into());
            canvas.fill_rounded_rect(content_x + 405, cy, 32, 32, 5, [44, 44, 66, 255]);
            canvas.draw_text_px(content_x + 416, cy + 7, "+", 14.0, [240, 242, 252, 255]);
            state.settings.hit_regions.push((s_inc, SettingsAction::AdjustAnimSpeed(0.25)));
            cy += 40;

            // Row 5: Window Gaps & Border Width
            let gaps_txt = format!("Window Gaps: {}px", state.panel.window_gaps);
            canvas.draw_text_px(content_x, cy + 8, &gaps_txt, 13.0, [225, 228, 245, 255]);
            let g_dec = Rectangle::new((wx + content_x + 150, wy + cy).into(), (30, 30).into());
            canvas.fill_rounded_rect(content_x + 150, cy, 30, 30, 5, [44, 44, 66, 255]);
            canvas.draw_text_px(content_x + 161, cy + 6, "-", 14.0, [240, 242, 252, 255]);
            state.settings.hit_regions.push((g_dec, SettingsAction::AdjustWindowGaps(-2)));

            let g_inc = Rectangle::new((wx + content_x + 186, wy + cy).into(), (30, 30).into());
            canvas.fill_rounded_rect(content_x + 186, cy, 30, 30, 5, [44, 44, 66, 255]);
            canvas.draw_text_px(content_x + 196, cy + 6, "+", 14.0, [240, 242, 252, 255]);
            state.settings.hit_regions.push((g_inc, SettingsAction::AdjustWindowGaps(2)));

            let bw_txt = format!("Border: {}px", state.theme.border_width);
            canvas.draw_text_px(content_x + 250, cy + 8, &bw_txt, 13.0, [225, 228, 245, 255]);
            let b_dec = Rectangle::new((wx + content_x + 365, wy + cy).into(), (30, 30).into());
            canvas.fill_rounded_rect(content_x + 365, cy, 30, 30, 5, [44, 44, 66, 255]);
            canvas.draw_text_px(content_x + 376, cy + 6, "-", 14.0, [240, 242, 252, 255]);
            state.settings.hit_regions.push((b_dec, SettingsAction::AdjustBorderWidth(-1)));

            let b_inc = Rectangle::new((wx + content_x + 401, wy + cy).into(), (30, 30).into());
            canvas.fill_rounded_rect(content_x + 401, cy, 30, 30, 5, [44, 44, 66, 255]);
            canvas.draw_text_px(content_x + 411, cy + 6, "+", 14.0, [240, 242, 252, 255]);
            state.settings.hit_regions.push((b_inc, SettingsAction::AdjustBorderWidth(1)));
            cy += 40;

            // Row 6: Circular Accent Color Swatch Dots with Active Ring
            canvas.draw_text_px(content_x, cy, "Accent Color", 13.0, [215, 220, 238, 255]);
            cy += 22;
            canvas.fill_rounded_rect_aa(content_x, cy - 4, 506, 40, 10, [255, 255, 255, 16]);
            for (idx, (name, rgb, _)) in ACCENT_PRESETS.iter().enumerate() {
                let bx = content_x + 14 + idx as i32 * 82;
                let preset_rect = Rectangle::new((wx + bx - 4, wy + cy).into(), (78, 32).into());
                let dot_cx = (bx + 12) as f32;
                let dot_cy = (cy + 16) as f32;
                if state.panel.accent_preset == idx {
                    canvas.fill_circle_aa(dot_cx, dot_cy, 11.5, [255, 255, 255, 235]);
                    canvas.fill_circle_aa(dot_cx, dot_cy, 9.5, [24, 26, 36, 255]);
                }
                canvas.fill_circle_aa(dot_cx, dot_cy, 8.0, [rgb[0], rgb[1], rgb[2], 255]);
                canvas.draw_text_px(bx + 26, cy + 8, name, 11.5, [232, 236, 248, 255]);
                state.settings.hit_regions.push((preset_rect, SettingsAction::SelectAccent(idx)));
            }
            cy += 44;

            // Row 7: Reload Wallpaper + Font Info
            let wp_rect = Rectangle::new((wx + content_x, wy + cy).into(), (506, 32).into());
            canvas.fill_rounded_rect(content_x, cy, 506, 32, 6, [38, 38, 58, 255]);
            canvas.draw_text_px(
                content_x + 14,
                cy + 8,
                "Reload Wallpaper (~/.config/okay-de/wallpaper.png)",
                13.0,
                [235, 238, 250, 255],
            );
            state.settings.hit_regions.push((wp_rect, SettingsAction::ReloadWallpaper));
            cy += 38;

            let font_info = format!("Active Font: {} (TrueType anti-aliased)", active_font_name());
            canvas.draw_text_px(content_x, cy, &font_info, 12.0, [155, 160, 185, 255]);
        }

        SettingsTab::Displays => {
            canvas.draw_text_px(
                content_x,
                cy,
                "Displays & Multi-Monitor Arrangement (Ekrany)",
                15.0,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 30;

            let outputs: Vec<Output> = state.space.outputs().cloned().collect();
            for (idx, out) in outputs.iter().enumerate() {
                let geo = state.space.output_geometry(out).unwrap_or_default();
                let cur_scale = out.current_scale().fractional_scale();
                let refresh_hz = out
                    .current_mode()
                    .map(|m| m.refresh as f64 / 1000.0)
                    .unwrap_or(60.0);

                canvas.fill_rounded_rect(content_x, cy, 506, 78, 7, [28, 28, 44, 245]);
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
                canvas.draw_text_px(content_x + 14, cy + 10, &title, 13.5, [238, 240, 252, 255]);

                canvas.draw_text_px(
                    content_x + 14,
                    cy + 44,
                    "UI Scale:",
                    12.5,
                    [175, 180, 205, 255],
                );
                for (si, s_val) in [1.0, 1.25, 1.5, 2.0].into_iter().enumerate() {
                    let sx = content_x + 96 + si as i32 * 76;
                    let s_rect = Rectangle::new((wx + sx, wy + cy + 38).into(), (68, 28).into());
                    let is_cur = (cur_scale - s_val).abs() < 0.05;
                    let bg = if is_cur {
                        [accent[0], accent[1], accent[2], 255]
                    } else {
                        [44, 44, 66, 255]
                    };
                    let fg = if is_cur {
                        [16, 16, 24, 255]
                    } else {
                        [230, 232, 246, 255]
                    };
                    canvas.fill_rounded_rect(sx, cy + 38, 68, 28, 5, bg);
                    canvas.draw_text_px(sx + 14, cy + 44, &format!("{:.2}x", s_val), 12.0, fg);
                    state
                        .settings
                        .hit_regions
                        .push((s_rect, SettingsAction::SetDisplayScale(idx, s_val)));
                }
                cy += 88;
            }

            if outputs.len() >= 2 {
                let swap_rect = Rectangle::new((wx + content_x, wy + cy).into(), (280, 34).into());
                canvas.fill_rounded_rect(content_x, cy, 280, 34, 6, [44, 44, 68, 255]);
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
                "Wi-Fi & Network Connections (Siec bezprzewodowa)",
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
            canvas.fill_rounded_rect(content_x + 380, cy - 6, 126, 28, 5, [48, 48, 72, 255]);
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

            if state.settings.wifi_networks.is_empty() {
                canvas.fill_rounded_rect(content_x, cy, 506, 64, 6, [28, 28, 42, 240]);
                canvas.draw_text_px(
                    content_x + 14,
                    cy + 22,
                    "No Wi-Fi networks listed yet. Click 'Refresh Scan' above.",
                    13.0,
                    [165, 170, 195, 255],
                );
            } else {
                for (idx, net) in state.settings.wifi_networks.iter().enumerate() {
                    let row_y = cy + idx as i32 * 46;
                    let row_bg = if net.in_use {
                        [42, 52, 68, 250]
                    } else {
                        [30, 30, 46, 240]
                    };
                    canvas.fill_rounded_rect(content_x, row_y, 506, 40, 6, row_bg);

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
                    canvas.fill_rounded_rect(content_x + 396, row_y + 6, 98, 28, 5, btn_bg);
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
            canvas.fill_rounded_rect(content_x + 356, cy, 150, 34, 6, [48, 48, 72, 255]);
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
                canvas.fill_rounded_rect(content_x, cy, 506, 60, 6, [28, 28, 42, 240]);
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
                    canvas.fill_rounded_rect(content_x, ry, 506, 38, 6, [30, 30, 46, 240]);
                    let line = format!("{}  ({})", dev.name, dev.mac);
                    canvas.draw_text_px(content_x + 14, ry + 10, &line, 13.0, [238, 240, 252, 255]);

                    let b_rect = Rectangle::new((wx + content_x + 396, wy + ry + 5).into(), (98, 28).into());
                    let b_lbl = if dev.connected { "Disconnect" } else { "Connect" };
                    canvas.fill_rounded_rect(content_x + 396, ry + 5, 98, 28, 5, [48, 72, 86, 255]);
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
                "Sound, Master Output & Microphone (Dzwiek)",
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
            canvas.fill_rounded_rect(content_x, cy + 6, bar_w, 16, 6, [28, 28, 42, 255]);
            let fill_w = ((state.panel.volume_percent as f32 / 100.0) * bar_w as f32).round() as i32;
            canvas.fill_rounded_rect(
                content_x,
                cy + 6,
                fill_w.max(8),
                16,
                6,
                [accent[0], accent[1], accent[2], 255],
            );
            for step in 0..=10 {
                let sx = content_x + step * (bar_w / 11);
                let s_rect = Rectangle::new((wx + sx, wy + cy).into(), (bar_w / 11, 28).into());
                state
                    .settings
                    .hit_regions
                    .push((s_rect, SettingsAction::SetVolume(step * 10)));
            }

            let mute_rect = Rectangle::new((wx + content_x + 396, wy + cy).into(), (110, 28).into());
            let mute_lbl = if state.panel.volume_muted { "Unmute" } else { "Mute Out" };
            canvas.fill_rounded_rect(content_x + 396, cy, 110, 28, 5, [48, 48, 72, 255]);
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

            canvas.fill_rounded_rect(content_x, cy + 6, bar_w, 16, 6, [28, 28, 42, 255]);
            let mfill_w = ((state.settings.mic_volume as f32 / 100.0) * bar_w as f32).round() as i32;
            canvas.fill_rounded_rect(
                content_x,
                cy + 6,
                mfill_w.max(8),
                16,
                6,
                [accent[0], accent[1], accent[2], 255],
            );
            for step in 0..=10 {
                let sx = content_x + step * (bar_w / 11);
                let s_rect = Rectangle::new((wx + sx, wy + cy).into(), (bar_w / 11, 28).into());
                state
                    .settings
                    .hit_regions
                    .push((s_rect, SettingsAction::SetMicVolume(step * 10)));
            }

            let mmute_rect = Rectangle::new((wx + content_x + 396, wy + cy).into(), (110, 28).into());
            let mmute_lbl = if state.settings.mic_muted { "Unmute Mic" } else { "Mute Mic" };
            canvas.fill_rounded_rect(content_x + 396, cy, 110, 28, 5, [48, 48, 72, 255]);
            canvas.draw_text_px(content_x + 412, cy + 6, mmute_lbl, 12.5, [238, 240, 252, 255]);
            state
                .settings
                .hit_regions
                .push((mmute_rect, SettingsAction::ToggleMuteMic));
        }

        SettingsTab::MouseKeyboard => {
            canvas.draw_text_px(
                content_x,
                cy,
                "Mouse, Touchpad & Keyboard (Mysz i klawiatura)",
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
            canvas.fill_rounded_rect(content_x, cy, 36, 30, 5, [44, 44, 66, 255]);
            canvas.draw_text_px(content_x + 14, cy + 6, "-", 15.0, [240, 242, 252, 255]);
            state
                .settings
                .hit_regions
                .push((dec_rect, SettingsAction::AdjustPointerSpeed(-0.15)));

            let bar_x = content_x + 46;
            let bar_w = 360;
            canvas.fill_rounded_rect(bar_x, cy + 8, bar_w, 14, 6, [28, 28, 42, 255]);
            let norm = ((state.pointer_sensitivity - 0.25) / (2.50 - 0.25)).clamp(0.0, 1.0);
            let fill_w = (norm * bar_w as f64).round() as i32;
            canvas.fill_rounded_rect(
                bar_x,
                cy + 8,
                fill_w.max(10),
                14,
                6,
                [accent[0], accent[1], accent[2], 255],
            );

            for step in 0..=9 {
                let seg_w = bar_w / 10;
                let sx = bar_x + step * seg_w;
                let spd = 0.25 + (step as f64 / 9.0) * 2.25;
                let seg_rect = Rectangle::new((wx + sx, wy + cy).into(), (seg_w, 30).into());
                state
                    .settings
                    .hit_regions
                    .push((seg_rect, SettingsAction::SetPointerSpeed(spd)));
            }

            let inc_rect = Rectangle::new((wx + bar_x + bar_w + 10, wy + cy).into(), (36, 30).into());
            canvas.fill_rounded_rect(bar_x + bar_w + 10, cy, 36, 30, 5, [44, 44, 66, 255]);
            canvas.draw_text_px(bar_x + bar_w + 22, cy + 6, "+", 15.0, [240, 242, 252, 255]);
            state
                .settings
                .hit_regions
                .push((inc_rect, SettingsAction::AdjustPointerSpeed(0.15)));
            cy += 42;

            let nat_rect = Rectangle::new((wx + content_x, wy + cy).into(), (506, 34).into());
            let nat_label = if state.natural_scroll {
                "Natural Scrolling (Przewijanie naturalne):   Enabled"
            } else {
                "Natural Scrolling (Przewijanie naturalne):   Disabled"
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
                canvas.fill_rounded_rect(content_x, cy, 506, 25, 4, [28, 28, 42, 235]);
                canvas.draw_text_px(content_x + 10, cy + 5, dev, 12.0, [215, 218, 236, 255]);
                cy += 28;
            }
        }

        SettingsTab::DefaultApps => {
            canvas.draw_text_px(
                content_x,
                cy,
                "Startup & Default Applications (Domyslne aplikacje)",
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
            canvas.fill_rounded_rect(content_x, cy, 506, 36, 6, [40, 40, 62, 255]);
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
                "System Information (Informacje o systemie)",
                15.0,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 28;

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
                canvas.fill_rounded_rect(content_x, cy, 506, 32, 5, [28, 28, 42, 240]);
                canvas.draw_text_px(
                    content_x + 12,
                    cy + 8,
                    label,
                    12.5,
                    [160, 165, 190, 255],
                );
                let trimmed: String = val.chars().take(42).collect();
                canvas.draw_text_px(
                    content_x + 165,
                    cy + 8,
                    &trimmed,
                    12.5,
                    [238, 240, 252, 255],
                );
                cy += 37;
            }
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
        let root = std::ffi::CString::new("/").unwrap();
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
