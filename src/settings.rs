use std::{
    fs,
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
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, Scale, Transform},
};

use crate::{
    panel::{active_font_name, rect_contains, Canvas, ACCENT_PRESETS},
    render::OkayRenderElements,
    state::OkayState,
};

pub const SETTINGS_WIDTH: i32 = 680;
pub const SETTINGS_HEIGHT: i32 = 440;
const HEADER_HEIGHT: i32 = 38;
const SIDEBAR_WIDTH: i32 = 175;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Appearance,
    Network,
    Mouse,
    SystemInfo,
}

impl SettingsTab {
    const ALL: [Self; 4] = [
        Self::Appearance,
        Self::Network,
        Self::Mouse,
        Self::SystemInfo,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance (Wyglad)",
            Self::Network => "Wi-Fi & Net (Siec)",
            Self::Mouse => "Mouse & Input (Mysz)",
            Self::SystemInfo => "System Info",
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
enum SettingsAction {
    CloseWindow,
    SelectTab(SettingsTab),
    ToggleDockMode,
    ToggleShowWorkspaces,
    ToggleTilingMode,
    SelectAccent(usize),
    ReloadWallpaper,
    RefreshWifi,
    ToggleWifi(usize),
    SetPointerSpeed(f64),
    AdjustPointerSpeed(f64),
    ToggleNaturalScroll,
}

#[derive(Debug)]
pub struct SettingsState {
    pub open: bool,
    pub active_tab: SettingsTab,
    pub position: Point<i32, Logical>,
    pub dragging: Option<Point<f64, Logical>>,
    pub wifi_networks: Vec<WifiNetwork>,
    pub wifi_status: String,
    pub os_name: String,
    pub kernel_ver: String,
    pub cpu_model: String,
    pub gpu_renderer: String,
    pub ram_summary: String,
    pub input_devices: Vec<String>,
    hit_regions: Vec<(Rectangle<i32, Logical>, SettingsAction)>,
}

impl Default for SettingsState {
    fn default() -> Self {
        let mut s = Self {
            open: false,
            active_tab: SettingsTab::Appearance,
            position: Point::from((140, 90)),
            dragging: None,
            wifi_networks: Vec::new(),
            wifi_status: "Click Refresh Scan to query NetworkManager".to_string(),
            os_name: String::new(),
            kernel_ver: String::new(),
            cpu_model: String::new(),
            gpu_renderer: "OpenGL ES (Smithay GlesRenderer / DRM-KMS)".to_string(),
            ram_summary: String::new(),
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
        self.ram_summary = read_ram_summary();
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
                if tab == SettingsTab::Network && state.settings.wifi_networks.is_empty() {
                    state.settings.refresh_wifi(false);
                } else if tab == SettingsTab::SystemInfo {
                    state.settings.refresh_system_info();
                }
            }
            SettingsAction::ToggleDockMode => {
                state.panel.dock_mode = !state.panel.dock_mode;
                state
                    .panel
                    .save_config_file(state.pointer_sensitivity, state.natural_scroll);
            }
            SettingsAction::ToggleShowWorkspaces => {
                state.panel.show_workspaces = !state.panel.show_workspaces;
                state
                    .panel
                    .save_config_file(state.pointer_sensitivity, state.natural_scroll);
            }
            SettingsAction::ToggleTilingMode => {
                state.toggle_tiling();
            }
            SettingsAction::SelectAccent(idx) => {
                state.panel.accent_preset = idx % ACCENT_PRESETS.len();
                state.theme.active_border_color = state.panel.accent_f32();
                state
                    .panel
                    .save_config_file(state.pointer_sensitivity, state.natural_scroll);
            }
            SettingsAction::ReloadWallpaper => {
                state.desktop.reload();
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
            SettingsAction::SetPointerSpeed(speed) => {
                state.pointer_sensitivity = speed.clamp(0.25, 2.50);
                state
                    .panel
                    .save_config_file(state.pointer_sensitivity, state.natural_scroll);
            }
            SettingsAction::AdjustPointerSpeed(delta) => {
                state.pointer_sensitivity = (state.pointer_sensitivity + delta).clamp(0.25, 2.50);
                state
                    .panel
                    .save_config_file(state.pointer_sensitivity, state.natural_scroll);
            }
            SettingsAction::ToggleNaturalScroll => {
                state.natural_scroll = !state.natural_scroll;
                state
                    .panel
                    .save_config_file(state.pointer_sensitivity, state.natural_scroll);
            }
        }
        return true;
    }

    // Clicking on the header bar initiates window drag
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

/// Renders the native `OKay Settings` window if open on the primary output.
pub fn render_settings_elements(
    state: &mut OkayState,
    renderer: &mut GlesRenderer,
    output: &Output,
    scale: Scale<f64>,
) -> Vec<OkayRenderElements<GlesRenderer>> {
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
    // Window frame & border
    canvas.fill_rounded_rect(0, 0, SETTINGS_WIDTH, SETTINGS_HEIGHT, 10, [18, 18, 28, 250]);
    canvas.stroke_rect(0, 0, SETTINGS_WIDTH, SETTINGS_HEIGHT, [accent[0], accent[1], accent[2], 210]);

    // Header bar
    canvas.fill_rect(2, 2, SETTINGS_WIDTH - 4, HEADER_HEIGHT - 2, [28, 26, 44, 255]);
    canvas.fill_rect(0, 0, SETTINGS_WIDTH, 2, [accent[0], accent[1], accent[2], 255]);
    canvas.draw_text_px(
        14,
        10,
        "OKay Settings  -  System Configuration",
        14.5,
        [240, 242, 252, 255],
    );

    // Close [X] button
    let close_rect = Rectangle::new((wx + SETTINGS_WIDTH - 34, wy + 7).into(), (24, 24).into());
    let close_bg = if rect_contains(close_rect, pointer_x, pointer_y) {
        [220, 76, 104, 255]
    } else {
        [185, 62, 88, 255]
    };
    canvas.fill_rounded_rect(SETTINGS_WIDTH - 34, 7, 24, 24, 5, close_bg);
    canvas.draw_text_px(SETTINGS_WIDTH - 26, 10, "x", 14.0, [255, 245, 248, 255]);
    state
        .settings
        .hit_regions
        .push((close_rect, SettingsAction::CloseWindow));

    // Left Sidebar
    canvas.fill_rect(
        2,
        HEADER_HEIGHT,
        SIDEBAR_WIDTH,
        SETTINGS_HEIGHT - HEADER_HEIGHT - 2,
        [24, 24, 36, 255],
    );
    canvas.fill_rect(
        SIDEBAR_WIDTH + 2,
        HEADER_HEIGHT,
        1,
        SETTINGS_HEIGHT - HEADER_HEIGHT - 2,
        [46, 46, 68, 255],
    );

    for (i, tab) in SettingsTab::ALL.iter().enumerate() {
        let ty = HEADER_HEIGHT + 12 + i as i32 * 42;
        let tab_rect = Rectangle::new(
            (wx + 10, wy + ty).into(),
            (SIDEBAR_WIDTH - 16, 34).into(),
        );
        let is_selected = state.settings.active_tab == *tab;
        let hovered = rect_contains(tab_rect, pointer_x, pointer_y);
        let bg = if is_selected {
            [accent[0], accent[1], accent[2], 255]
        } else if hovered {
            [46, 46, 68, 255]
        } else {
            [32, 32, 48, 240]
        };
        let fg = if is_selected {
            [16, 16, 24, 255]
        } else {
            [230, 232, 246, 255]
        };
        canvas.fill_rounded_rect(10, ty, SIDEBAR_WIDTH - 16, 34, 6, bg);
        canvas.draw_text_px(20, ty + 9, tab.title(), 13.5, fg);
        state
            .settings
            .hit_regions
            .push((tab_rect, SettingsAction::SelectTab(*tab)));
    }

    let content_x = SIDEBAR_WIDTH + 20;
    let mut cy = HEADER_HEIGHT + 18;

    match state.settings.active_tab {
        SettingsTab::Appearance => {
            canvas.draw_text_px(
                content_x,
                cy,
                "Appearance & Taskbar (Wyglad i pasek zadan)",
                15.5,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 32;

            // 1. Dock Mode toggle
            let dock_rect = Rectangle::new((wx + content_x, wy + cy).into(), (460, 36).into());
            let dock_label = if state.panel.dock_mode {
                "Taskbar Style:  Floating Centered Dock (macOS / Plasma style)"
            } else {
                "Taskbar Style:  Full-Width Edge-to-Edge Bar"
            };
            draw_setting_button(
                &mut canvas,
                content_x,
                cy,
                460,
                36,
                dock_label,
                state.panel.dock_mode,
                accent,
            );
            state
                .settings
                .hit_regions
                .push((dock_rect, SettingsAction::ToggleDockMode));
            cy += 46;

            // 2. Show Workspaces toggle
            let ws_rect = Rectangle::new((wx + content_x, wy + cy).into(), (460, 36).into());
            let ws_label = if state.panel.show_workspaces {
                "Workspace Indicators (1..9):  Visible on Taskbar"
            } else {
                "Workspace Indicators (1..9):  Hidden"
            };
            draw_setting_button(
                &mut canvas,
                content_x,
                cy,
                460,
                36,
                ws_label,
                state.panel.show_workspaces,
                accent,
            );
            state
                .settings
                .hit_regions
                .push((ws_rect, SettingsAction::ToggleShowWorkspaces));
            cy += 46;

            // 3. Tiling vs Floating toggle
            let tile_rect = Rectangle::new((wx + content_x, wy + cy).into(), (460, 36).into());
            let tile_label = if state.workspaces.tiling_enabled {
                "Window Layout (Super+T):  Automatic Master-Stack Tiling"
            } else {
                "Window Layout (Super+T):  Floating Windows"
            };
            draw_setting_button(
                &mut canvas,
                content_x,
                cy,
                460,
                36,
                tile_label,
                state.workspaces.tiling_enabled,
                accent,
            );
            state
                .settings
                .hit_regions
                .push((tile_rect, SettingsAction::ToggleTilingMode));
            cy += 50;

            // 4. Accent Color Presets
            canvas.draw_text_px(
                content_x,
                cy,
                "Accent Color Preset:",
                13.5,
                [210, 214, 235, 255],
            );
            cy += 24;
            for (idx, (name, rgb, _)) in ACCENT_PRESETS.iter().enumerate() {
                let bx = content_x + idx as i32 * 92;
                let preset_rect = Rectangle::new((wx + bx, wy + cy).into(), (86, 32).into());
                canvas.fill_rounded_rect(bx, cy, 86, 32, 6, [rgb[0], rgb[1], rgb[2], 255]);
                if state.panel.accent_preset == idx {
                    canvas.stroke_rect(bx - 1, cy - 1, 88, 34, [255, 255, 255, 255]);
                }
                canvas.draw_text_px(bx + 10, cy + 8, name, 12.5, [16, 16, 24, 255]);
                state
                    .settings
                    .hit_regions
                    .push((preset_rect, SettingsAction::SelectAccent(idx)));
            }
            cy += 46;

            // 5. Reload Wallpaper button + Font status
            let wp_rect = Rectangle::new((wx + content_x, wy + cy).into(), (460, 34).into());
            canvas.fill_rounded_rect(content_x, cy, 460, 34, 6, [38, 38, 58, 255]);
            canvas.draw_text_px(
                content_x + 14,
                cy + 9,
                "Reload Wallpaper (~/.config/okay-de/wallpaper.png)",
                13.0,
                [235, 238, 250, 255],
            );
            state
                .settings
                .hit_regions
                .push((wp_rect, SettingsAction::ReloadWallpaper));
            cy += 44;

            let font_info = format!("Active UI Font: {} (TrueType anti-aliased)", active_font_name());
            canvas.draw_text_px(content_x, cy, &font_info, 12.5, [155, 160, 185, 255]);
        }

        SettingsTab::Network => {
            canvas.draw_text_px(
                content_x,
                cy,
                "Wi-Fi & Network (Siec bezprzewodowa)",
                15.5,
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

            let scan_rect = Rectangle::new((wx + content_x + 340, wy + cy - 6).into(), (120, 28).into());
            canvas.fill_rounded_rect(content_x + 340, cy - 6, 120, 28, 5, [48, 48, 72, 255]);
            canvas.draw_text_px(
                content_x + 354,
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
                canvas.fill_rounded_rect(content_x, cy, 460, 64, 6, [28, 28, 42, 240]);
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
                    canvas.fill_rounded_rect(content_x, row_y, 460, 40, 6, row_bg);

                    let ssid_line = format!(
                        "{}  ({}%  /  {})",
                        net.ssid, net.signal, net.security
                    );
                    canvas.draw_text_px(
                        content_x + 14,
                        row_y + 11,
                        &ssid_line,
                        13.5,
                        [238, 240, 252, 255],
                    );

                    let btn_rect = Rectangle::new(
                        (wx + content_x + 352, wy + row_y + 6).into(),
                        (98, 28).into(),
                    );
                    let (btn_bg, btn_label) = if net.in_use {
                        ([175, 62, 86, 255], "Disconnect")
                    } else {
                        ([52, 86, 72, 255], "Connect")
                    };
                    canvas.fill_rounded_rect(content_x + 352, row_y + 6, 98, 28, 5, btn_bg);
                    canvas.draw_text_px(
                        content_x + 366,
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

        SettingsTab::Mouse => {
            canvas.draw_text_px(
                content_x,
                cy,
                "Mouse, Touchpad & Input (Mysz i urzadzenia)",
                15.5,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 34;

            let speed_label = format!(
                "Pointer Speed / Sensitivity:   {:.2}x",
                state.pointer_sensitivity
            );
            canvas.draw_text_px(content_x, cy, &speed_label, 14.0, [235, 238, 250, 255]);
            cy += 26;

            // [-] button
            let dec_rect = Rectangle::new((wx + content_x, wy + cy).into(), (36, 30).into());
            canvas.fill_rounded_rect(content_x, cy, 36, 30, 5, [44, 44, 66, 255]);
            canvas.draw_text_px(content_x + 14, cy + 6, "-", 15.0, [240, 242, 252, 255]);
            state
                .settings
                .hit_regions
                .push((dec_rect, SettingsAction::AdjustPointerSpeed(-0.15)));

            // Slider bar (clickable segments)
            let bar_x = content_x + 46;
            let bar_w = 350;
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

            // [+] button
            let inc_rect = Rectangle::new((wx + bar_x + bar_w + 10, wy + cy).into(), (36, 30).into());
            canvas.fill_rounded_rect(bar_x + bar_w + 10, cy, 36, 30, 5, [44, 44, 66, 255]);
            canvas.draw_text_px(bar_x + bar_w + 22, cy + 6, "+", 15.0, [240, 242, 252, 255]);
            state
                .settings
                .hit_regions
                .push((inc_rect, SettingsAction::AdjustPointerSpeed(0.15)));
            cy += 46;

            // Natural scroll toggle
            let nat_rect = Rectangle::new((wx + content_x, wy + cy).into(), (442, 36).into());
            let nat_label = if state.natural_scroll {
                "Natural Scrolling (Przewijanie naturalne):   Enabled"
            } else {
                "Natural Scrolling (Przewijanie naturalne):   Disabled"
            };
            draw_setting_button(
                &mut canvas,
                content_x,
                cy,
                442,
                36,
                nat_label,
                state.natural_scroll,
                accent,
            );
            state
                .settings
                .hit_regions
                .push((nat_rect, SettingsAction::ToggleNaturalScroll));
            cy += 52;

            canvas.draw_text_px(
                content_x,
                cy,
                "Detected Input Hardware (libinput / sysfs):",
                13.5,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 24;
            for dev in state.settings.input_devices.iter().take(5) {
                canvas.fill_rounded_rect(content_x, cy, 442, 26, 4, [28, 28, 42, 235]);
                let line = format!("•  {}", dev);
                canvas.draw_text_px(content_x + 10, cy + 5, &line, 12.5, [215, 218, 236, 255]);
                cy += 30;
            }
        }

        SettingsTab::SystemInfo => {
            canvas.draw_text_px(
                content_x,
                cy,
                "System Information (Informacje o systemie)",
                15.5,
                [accent[0], accent[1], accent[2], 255],
            );
            cy += 30;

            let outputs_summary: String = state
                .space
                .outputs()
                .map(|o| {
                    let geo = state.space.output_geometry(o).unwrap_or_default();
                    format!("{} ({}x{} @ +{},+{})", o.name(), geo.size.w, geo.size.h, geo.loc.x, geo.loc.y)
                })
                .collect::<Vec<_>>()
                .join(", ");

            let rows = [
                ("Desktop Environment", "OKay DE 0.1.0 (Native Smithay 0.7.0 / Wayland)"),
                ("Operating System", &state.settings.os_name),
                ("Linux Kernel", &state.settings.kernel_ver),
                ("Processor (CPU)", &state.settings.cpu_model),
                ("Graphics (GPU)", &state.settings.gpu_renderer),
                ("Memory (RAM)", &state.settings.ram_summary),
                ("Active Monitors", &outputs_summary),
                ("UI Font Engine", &active_font_name()),
            ];

            for (label, val) in rows {
                canvas.fill_rounded_rect(content_x, cy, 460, 34, 5, [28, 28, 42, 240]);
                canvas.draw_text_px(
                    content_x + 12,
                    cy + 9,
                    label,
                    12.5,
                    [160, 165, 190, 255],
                );
                let max_chars = 38;
                let trimmed: String = val.chars().take(max_chars).collect();
                canvas.draw_text_px(
                    content_x + 165,
                    cy + 9,
                    &trimmed,
                    13.0,
                    [238, 240, 252, 255],
                );
                cy += 40;
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
    let bg = if active {
        [44, 42, 66, 255]
    } else {
        [30, 30, 46, 245]
    };
    canvas.fill_rounded_rect(x, y, w, h, 6, bg);
    let badge_bg = if active {
        [accent[0], accent[1], accent[2], 255]
    } else {
        [64, 64, 88, 255]
    };
    canvas.fill_rounded_rect(x + w - 54, y + 7, 42, h - 14, 5, badge_bg);
    let badge_txt = if active { "ON" } else { "OFF" };
    let badge_fg = if active {
        [16, 16, 24, 255]
    } else {
        [220, 224, 240, 255]
    };
    canvas.draw_text_px(x + w - 43, y + 10, badge_txt, 12.0, badge_fg);
    canvas.draw_text_px(x + 14, y + 10, label, 13.0, [238, 240, 252, 255]);
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
            return format!("{} MiB / {} MiB used", used_mib, total_mib);
        }
    }
    "Unknown RAM".to_string()
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
