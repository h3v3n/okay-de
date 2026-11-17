use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
    panel::{contrast_fg, rect_contains, window_title_str, Canvas, PANEL_HEIGHT},
    render::OkayRenderElements,
    settings::SettingsTab,
    state::OkayState,
};

#[derive(Debug, Clone)]
pub struct NotificationToast {
    pub title: String,
    pub body: String,
    pub created_at: Instant,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct DesktopIcon {
    pub label: String,
    pub icon_name: Option<String>,
    pub kind: crate::icons::VectorIconKind,
    pub exec_cmd: Option<String>,
    pub grid_col: i32,
    pub grid_row: i32,
    pub action: DesktopIconAction,
}

#[derive(Debug, Clone)]
pub enum DesktopIconAction {
    OpenHome,
    OpenTerminal,
    OpenSettings,
    TakeScreenshot,
    OpenPath(PathBuf),
}

#[derive(Debug, Clone)]
pub struct CreatingFolderState {
    pub name: String,
    pub grid_col: i32,
    pub grid_row: i32,
}

#[derive(Debug)]
pub struct OverlaysState {
    pub notifications: Vec<NotificationToast>,
    pub history: Vec<(String, String)>,
    pub notif_drawer_open: bool,
    pub calendar_open: bool,
    pub clipboard_open: bool,
    pub control_center_open: bool,
    pub cc_wifi_expanded: bool,
    pub cc_bt_expanded: bool,
    pub clipboard_items: Vec<String>,
    pub alttab_visible_until: Option<Instant>,
    pub locked: bool,
    pub lock_input: String,
    pub context_menu_pos: Option<Point<i32, Logical>>,
    pub icon_context_menu: Option<(Point<i32, Logical>, usize)>,
    pub show_desktop_icons: bool,
    pub desktop_icons: Vec<DesktopIcon>,
    pub selected_icons: Vec<usize>,
    pub last_icon_click: Option<(usize, Instant)>,
    pub rubberband_start: Option<Point<f64, Logical>>,
    pub rubberband_current: Option<Point<f64, Logical>>,
    pub dragging_icon: Option<usize>,
    pub creating_folder: Option<CreatingFolderState>,
    hit_regions: Vec<(Rectangle<i32, Logical>, OverlayClick)>,
}

#[derive(Debug, Clone)]
enum OverlayClick {
    DismissToast(usize),
    ClearNotifications,
    CloseNotifDrawer,
    SelectClipboard(usize),
    CloseCalendar,
    ControlToggleWifi,
    ControlToggleBt,
    ControlExpandWifi,
    ControlExpandBt,
    ControlConnectWifi(usize),
    ControlConnectBt(usize),
    ControlToggleTiling,
    ControlCycleMinimize,
    ControlSetVolume(i32),
    ControlOpenSettings,
    DesktopIconClick(usize),
    IconCtxOpen(usize),
    IconCtxPin(usize),
    IconCtxProperties(usize),
    IconCtxDelete(usize),
    ContextNewFolder,
    ContextOpenTerminal,
    ContextReloadWallpaper,
    ContextToggleIcons,
    ContextDisplaySettings,
    ContextOkaySettings,
}

impl Default for OverlaysState {
    fn default() -> Self {
        let mut s = Self {
            notifications: Vec::new(),
            history: Vec::new(),
            notif_drawer_open: false,
            calendar_open: false,
            clipboard_open: false,
            control_center_open: false,
            cc_wifi_expanded: false,
            cc_bt_expanded: false,
            clipboard_items: vec![
                "Welcome to OKay DE (Super+V Clipboard)".to_string(),
                "~/.config/okay-de/config.toml".to_string(),
            ],
            alttab_visible_until: None,
            locked: false,
            lock_input: String::new(),
            context_menu_pos: None,
            icon_context_menu: None,
            show_desktop_icons: true,
            desktop_icons: Vec::new(),
            selected_icons: Vec::new(),
            last_icon_click: None,
            rubberband_start: None,
            rubberband_current: None,
            dragging_icon: None,
            creating_folder: None,
            hit_regions: Vec::new(),
        };
        s.refresh_desktop_icons();
        s.push_notification(
            "OKay DE Ready",
            "Press Super+I for Settings, Super+D for Apps, Super+T to Tile",
        );
        s
    }
}

impl OverlaysState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts inline folder creation on the desktop grid with an editable name input.
    pub fn begin_creating_folder(&mut self) {
        self.show_desktop_icons = true;
        self.selected_icons.clear();

        let mut col = 1;
        let mut row = 0;
        'outer: loop {
            for r in 0..6 {
                let occupied = self
                    .desktop_icons
                    .iter()
                    .any(|ic| ic.grid_col == col && ic.grid_row == r);
                if !occupied {
                    row = r;
                    break 'outer;
                }
            }
            col += 1;
            if col > 12 {
                col = 1;
                row = 0;
                break;
            }
        }

        self.creating_folder = Some(CreatingFolderState {
            name: "New Folder".to_string(),
            grid_col: col,
            grid_row: row,
        });
    }

    /// Confirms inline folder creation, creates the directory in `~/Desktop`, and refreshes icons.
    pub fn confirm_creating_folder(&mut self) {
        let Some(cf) = self.creating_folder.take() else {
            return;
        };
        let sanitized: String = cf
            .name
            .trim()
            .chars()
            .map(|c| if c == '/' || c == '\0' { '_' } else { c })
            .collect();
        if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
            return;
        }
        if let Ok(home) = std::env::var("HOME") {
            let desktop_dir = PathBuf::from(home).join("Desktop");
            let mut target = desktop_dir.join(&sanitized);
            if target.exists() {
                for n in 1..=99 {
                    let candidate = desktop_dir.join(format!("{} ({})", sanitized, n));
                    if !candidate.exists() {
                        target = candidate;
                        break;
                    }
                }
            }
            if fs::create_dir_all(&target).is_ok() {
                let folder_name = target
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&sanitized)
                    .to_string();
                self.refresh_desktop_icons();
                self.push_notification(
                    "Desktop",
                    format!("Created ~/Desktop/{}", folder_name),
                );
            }
        }
    }

    /// Cancels inline folder creation without writing to disk.
    pub fn cancel_creating_folder(&mut self) {
        self.creating_folder = None;
    }

    pub fn push_notification(&mut self, title: impl Into<String>, body: impl Into<String>) {
        let t = title.into();
        let b = body.into();
        self.history.insert(0, (t.clone(), b.clone()));
        if self.history.len() > 20 {
            self.history.truncate(20);
        }
        self.notifications.push(NotificationToast {
            title: t,
            body: b,
            created_at: Instant::now(),
            timeout: Duration::from_secs(5),
        });
    }

    pub fn refresh_desktop_icons(&mut self) {
        let mut icons = vec![
            DesktopIcon {
                label: "Home".to_string(),
                icon_name: Some("user-home".to_string()),
                kind: crate::icons::VectorIconKind::Home,
                exec_cmd: None,
                grid_col: 0,
                grid_row: 0,
                action: DesktopIconAction::OpenHome,
            },
            DesktopIcon {
                label: "Terminal".to_string(),
                icon_name: Some("utilities-terminal".to_string()),
                kind: crate::icons::VectorIconKind::Terminal,
                exec_cmd: Some("foot".to_string()),
                grid_col: 0,
                grid_row: 1,
                action: DesktopIconAction::OpenTerminal,
            },
            DesktopIcon {
                label: "Settings".to_string(),
                icon_name: Some("preferences-system".to_string()),
                kind: crate::icons::VectorIconKind::Settings,
                exec_cmd: None,
                grid_col: 0,
                grid_row: 2,
                action: DesktopIconAction::OpenSettings,
            },
            DesktopIcon {
                label: "Screenshot".to_string(),
                icon_name: Some("applets-screenshooter".to_string()),
                kind: crate::icons::VectorIconKind::Screenshot,
                exec_cmd: None,
                grid_col: 0,
                grid_row: 3,
                action: DesktopIconAction::TakeScreenshot,
            },
        ];

        if let Ok(home) = std::env::var("HOME") {
            let desktop_dir = PathBuf::from(home).join("Desktop");
            if let Ok(entries) = fs::read_dir(desktop_dir) {
                let mut row = 0;
                let mut col = 1;
                for entry in entries.flatten().take(12) {
                    let raw_name = entry.file_name().to_string_lossy().to_string();
                    if raw_name.starts_with('.') {
                        continue;
                    }
                    let path = entry.path();
                    let is_desktop = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.eq_ignore_ascii_case("desktop"))
                        .unwrap_or(false);

                    let (label, icon_name, exec_cmd, kind) = if is_desktop {
                        let txt = fs::read_to_string(&path).unwrap_or_default();
                        let mut parsed_name = None;
                        let mut parsed_icon = None;
                        let mut parsed_exec = None;
                        for line in txt.lines() {
                            let l = line.trim();
                            if l.starts_with('[') && l != "[Desktop Entry]" {
                                break;
                            }
                            if parsed_name.is_none() {
                                if let Some(v) = l.strip_prefix("Name=") {
                                    parsed_name = Some(v.trim().to_string());
                                }
                            }
                            if parsed_icon.is_none() {
                                if let Some(v) = l.strip_prefix("Icon=") {
                                    parsed_icon = Some(v.trim().to_string());
                                }
                            }
                            if parsed_exec.is_none() {
                                if let Some(v) = l.strip_prefix("Exec=") {
                                    let cleaned: Vec<&str> = v
                                        .split_whitespace()
                                        .filter(|tok| !tok.starts_with('%'))
                                        .collect();
                                    if !cleaned.is_empty() {
                                        parsed_exec = Some(cleaned.join(" "));
                                    }
                                }
                            }
                        }
                        let clean = parsed_name.unwrap_or_else(|| {
                            raw_name
                                .strip_suffix(".desktop")
                                .unwrap_or(&raw_name)
                                .to_string()
                        });
                        let k = crate::icons::VectorIconKind::from_hint(
                            &clean,
                            parsed_exec.as_deref().unwrap_or(""),
                            false,
                        );
                        (clean, parsed_icon, parsed_exec, k)
                    } else if path.is_dir() {
                        (
                            raw_name,
                            Some("folder".to_string()),
                            None,
                            crate::icons::VectorIconKind::Folder,
                        )
                    } else {
                        let k = crate::icons::VectorIconKind::from_hint(&raw_name, "", false);
                        (raw_name, None, None, k)
                    };

                    icons.push(DesktopIcon {
                        label,
                        icon_name,
                        kind,
                        exec_cmd,
                        grid_col: col,
                        grid_row: row,
                        action: DesktopIconAction::OpenPath(path),
                    });
                    row += 1;
                    if row >= 5 {
                        row = 0;
                        col += 1;
                    }
                }
            }
        }
        self.desktop_icons = icons;
    }

    /// Periodic maintenance: expires old toast notifications, checks external notification file,
    /// and polls clipboard if `wl-paste` is available.
    pub fn poll_background(&mut self) {
        let now = Instant::now();
        self.notifications
            .retain(|n| now.duration_since(n.created_at) < n.timeout);

        if let Some(until) = self.alttab_visible_until {
            if now >= until {
                self.alttab_visible_until = None;
            }
        }

        // Check external notification spool (`/tmp/okay-de-notify`) so scripts/apps can push toasts
        let notify_file = PathBuf::from("/tmp/okay-de-notify");
        if notify_file.exists() {
            if let Ok(text) = fs::read_to_string(&notify_file) {
                let _ = fs::remove_file(&notify_file);
                for line in text.lines() {
                    let line = line.trim();
                    if !line.is_empty() {
                        if let Some((t, b)) = line.split_once('|') {
                            self.push_notification(t.trim(), b.trim());
                        } else {
                            self.push_notification("System Notification", line);
                        }
                    }
                }
            }
        }
    }

    pub fn push_clipboard_text(&mut self, text: String) {
        let trimmed = text.trim().to_string();
        if !trimmed.is_empty() && self.clipboard_items.first() != Some(&trimmed) {
            self.clipboard_items.retain(|x| x != &trimmed);
            self.clipboard_items.insert(0, trimmed);
            if self.clipboard_items.len() > 20 {
                self.clipboard_items.truncate(20);
            }
        }
    }

    pub fn poll_clipboard_now(&mut self) {
        crate::worker::dispatch(crate::worker::WorkerRequest::PollExternalClipboard);
    }
}

/// Captures a screenshot and saves a PNG image to `~/Pictures/Screenshots/` + pushes a toast.
pub fn take_screenshot(state: &mut OkayState) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let dir = PathBuf::from(home).join("Pictures/Screenshots");
    let _ = fs::create_dir_all(&dir);

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let filename = format!("okay-screenshot-{}.png", ts);
    let path = dir.join(&filename);

    // Prepare compositor snapshot PNG bytes as fallback if `grim` is unavailable.
    let mut canvas = Canvas::new(640, 360);
    let accent = state.panel.accent_rgb();
    canvas.fill_rect(0, 0, 640, 360, [18, 18, 28, 255]);
    canvas.stroke_rect(0, 0, 640, 360, [accent[0], accent[1], accent[2], 255]);
    canvas.draw_text_px(
        24,
        24,
        "OKay DE Screenshot Snapshot",
        18.0,
        [accent[0], accent[1], accent[2], 255],
    );
    let info = format!(
        "Workspace {}  |  Active Windows: {}",
        state.workspaces.active,
        state.space.elements().count()
    );
    canvas.draw_text_px(24, 58, &info, 14.0, [230, 232, 246, 255]);

    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, 640, 360);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        if let Ok(mut writer) = encoder.write_header() {
            let _ = writer.write_image_data(&canvas.pixels);
        }
    }

    // Run `grim` on the worker thread so it never deadlocks or blocks the compositor loop.
    crate::worker::dispatch(crate::worker::WorkerRequest::TakeScreenshot {
        path,
        socket_name: state.socket_name.clone(),
        fallback_png: if png_bytes.is_empty() { None } else { Some(png_bytes) },
    });
}

/// Handles right-click on the desktop background or on a desktop icon.
pub fn handle_desktop_right_click(state: &mut OkayState, pos: Point<f64, Logical>) -> bool {
    if state.overlays.locked {
        return true;
    }
    if state.surface_under(pos).is_some() || state.window_under(pos).is_some() {
        return false;
    }
    let px = pos.x.round() as i32;
    let py = pos.y.round() as i32;

    // Check if right-click landed on a desktop icon
    for (rect, action) in state.overlays.hit_regions.iter().rev() {
        if let OverlayClick::DesktopIconClick(idx) = action {
            if rect_contains(*rect, px, py) {
                state.overlays.context_menu_pos = None;
                state.overlays.selected_icons.clear();
                state.overlays.selected_icons.push(*idx);
                state.overlays.icon_context_menu = Some((Point::from((px, py)), *idx));
                return true;
            }
        }
    }

    state.overlays.icon_context_menu = None;
    state.overlays.context_menu_pos = Some(Point::from((px, py)));
    true
}

/// Starts a desktop rubber-band drag selection on empty desktop space.
pub fn start_desktop_rubberband(state: &mut OkayState, pos: Point<f64, Logical>) {
    state.overlays.context_menu_pos = None;
    state.overlays.icon_context_menu = None;
    state.overlays.selected_icons.clear();
    state.overlays.rubberband_start = Some(pos);
    state.overlays.rubberband_current = Some(pos);
}

/// Handles dragging a desktop icon across the desktop grid or dragging the rubber-band box.
pub fn handle_overlays_motion(state: &mut OkayState, pos: Point<f64, Logical>) -> bool {
    if let Some(idx) = state.overlays.dragging_icon {
        let top_margin = if state.panel.dock_mode || state.panel.panel_top {
            PANEL_HEIGHT + 18
        } else {
            20
        };
        let col = ((pos.x.round() as i32 - 20).max(0) / 98).clamp(0, 14);
        let row = ((pos.y.round() as i32 - top_margin).max(0) / 94).clamp(0, 8);
        let occupied = state
            .overlays
            .desktop_icons
            .iter()
            .enumerate()
            .any(|(i, ic)| i != idx && ic.grid_col == col && ic.grid_row == row)
            || state
                .overlays
                .creating_folder
                .as_ref()
                .map(|cf| cf.grid_col == col && cf.grid_row == row)
                .unwrap_or(false);
        if !occupied {
            if let Some(icon) = state.overlays.desktop_icons.get_mut(idx) {
                icon.grid_col = col;
                icon.grid_row = row;
            }
        }
        return true;
    }
    if let Some(start) = state.overlays.rubberband_start {
        state.overlays.rubberband_current = Some(pos);
        let sx = start.x.round() as i32;
        let sy = start.y.round() as i32;
        let cx = pos.x.round() as i32;
        let cy = pos.y.round() as i32;
        let rx = sx.min(cx);
        let ry = sy.min(cy);
        let rw = (sx - cx).abs();
        let rh = (sy - cy).abs();
        let sel_rect: Rectangle<i32, Logical> = Rectangle::new((rx, ry).into(), (rw, rh).into());

        state.overlays.selected_icons.clear();
        for (rect, action) in &state.overlays.hit_regions {
            if let OverlayClick::DesktopIconClick(idx) = action {
                let ix = rect.loc.x;
                let iy = rect.loc.y;
                let iw = rect.size.w;
                let ih = rect.size.h;
                let overlaps = ix < sel_rect.loc.x + sel_rect.size.w
                    && ix + iw > sel_rect.loc.x
                    && iy < sel_rect.loc.y + sel_rect.size.h
                    && iy + ih > sel_rect.loc.y;
                if overlaps && !state.overlays.selected_icons.contains(idx) {
                    state.overlays.selected_icons.push(*idx);
                }
            }
        }
        return true;
    }
    false
}

pub fn handle_overlays_release(state: &mut OkayState) -> bool {
    if state.overlays.dragging_icon.take().is_some() {
        return true;
    }
    if state.overlays.rubberband_start.take().is_some() {
        state.overlays.rubberband_current = None;
        return true;
    }
    false
}

fn launch_desktop_icon(state: &mut OkayState, icon: &DesktopIcon) {
    match &icon.action {
        DesktopIconAction::OpenHome => {
            let _ = Command::new("xdg-open")
                .arg(std::env::var("HOME").unwrap_or_else(|_| "/".to_string()))
                .env("WAYLAND_DISPLAY", &state.socket_name)
                .spawn();
        }
        DesktopIconAction::OpenTerminal => {
            crate::panel::spawn_terminal(&state.socket_name);
        }
        DesktopIconAction::OpenSettings => {
            state.settings.open = true;
            state.settings.refresh_system_info();
        }
        DesktopIconAction::TakeScreenshot => {
            take_screenshot(state);
        }
        DesktopIconAction::OpenPath(p) => {
            if let Some(cmd) = &icon.exec_cmd {
                let _ = Command::new("sh")
                    .arg("-c")
                    .arg(cmd)
                    .env("WAYLAND_DISPLAY", &state.socket_name)
                    .spawn();
            } else {
                let _ = Command::new("xdg-open")
                    .arg(p)
                    .env("WAYLAND_DISPLAY", &state.socket_name)
                    .spawn();
            }
        }
    }
}

/// Handles left-click on any active overlay (context menu, notifications, calendar, clipboard, desktop icons).
pub fn handle_overlays_click(state: &mut OkayState, pos: Point<f64, Logical>) -> bool {
    if state.overlays.locked {
        return true;
    }

    if state.overlays.creating_folder.is_some() {
        state.overlays.confirm_creating_folder();
        return true;
    }

    let px = pos.x.round() as i32;
    let py = pos.y.round() as i32;

    let mut matched: Option<OverlayClick> = None;
    for (rect, action) in state.overlays.hit_regions.iter().rev() {
        if rect_contains(*rect, px, py) {
            matched = Some(action.clone());
            break;
        }
    }

    if let Some(action) = matched {
        match action {
            OverlayClick::DismissToast(idx) => {
                if idx < state.overlays.notifications.len() {
                    state.overlays.notifications.remove(idx);
                }
            }
            OverlayClick::ClearNotifications => {
                state.overlays.history.clear();
                state.overlays.notifications.clear();
            }
            OverlayClick::CloseNotifDrawer => {
                state.overlays.notif_drawer_open = false;
            }
            OverlayClick::SelectClipboard(idx) => {
                if let Some(text) = state.overlays.clipboard_items.get(idx).cloned() {
                    state.overlays.push_clipboard_text(text.clone());
                    state.set_compositor_clipboard(text);
                    state
                        .overlays
                        .push_notification("Clipboard", "Copied selected entry to clipboard");
                }
                state.overlays.clipboard_open = false;
            }
            OverlayClick::CloseCalendar => {
                state.overlays.calendar_open = false;
            }
            OverlayClick::ControlToggleWifi => {
                let is_on = !state.settings.wifi_status.to_lowercase().contains("off");
                let flag = if is_on { "off" } else { "on" };
                state.settings.wifi_status = if is_on { "Off".to_string() } else { "On".to_string() };
                crate::worker::dispatch(crate::worker::WorkerRequest::RunCommand {
                    program: "nmcli".to_string(),
                    args: vec!["radio".into(), "wifi".into(), flag.into()],
                });
            }
            OverlayClick::ControlToggleBt => {
                state.settings.bt_powered = !state.settings.bt_powered;
                let flag = if state.settings.bt_powered { "on" } else { "off" };
                crate::worker::dispatch(crate::worker::WorkerRequest::ToggleBluetoothPower {
                    enable: state.settings.bt_powered,
                });
                state.settings.bt_status = format!("Bluetooth {}", flag);
            }
            OverlayClick::ControlExpandWifi => {
                state.overlays.cc_wifi_expanded = !state.overlays.cc_wifi_expanded;
                if state.overlays.cc_wifi_expanded {
                    state.overlays.cc_bt_expanded = false;
                    state.settings.refresh_wifi(true);
                }
            }
            OverlayClick::ControlExpandBt => {
                state.overlays.cc_bt_expanded = !state.overlays.cc_bt_expanded;
                if state.overlays.cc_bt_expanded {
                    state.overlays.cc_wifi_expanded = false;
                    state.settings.refresh_bluetooth();
                }
            }
            OverlayClick::ControlConnectWifi(idx) => {
                if let Some(net) = state.settings.wifi_networks.get(idx).cloned() {
                    if net.in_use {
                        crate::worker::dispatch(crate::worker::WorkerRequest::RunCommand {
                            program: "nmcli".to_string(),
                            args: vec!["dev".into(), "wifi".into(), "disconnect".into()],
                        });
                        state.settings.wifi_status = format!("Disconnecting {}...", net.ssid);
                    } else {
                        crate::worker::dispatch(crate::worker::WorkerRequest::ConnectWifi {
                            ssid: net.ssid.clone(),
                        });
                        state.settings.wifi_status = format!("Connecting to {}...", net.ssid);
                    }
                }
            }
            OverlayClick::ControlConnectBt(idx) => {
                if let Some(dev) = state.settings.bt_devices.get_mut(idx) {
                    let connect = !dev.connected;
                    crate::worker::dispatch(crate::worker::WorkerRequest::ToggleBluetoothDevice {
                        mac: dev.mac.clone(),
                        connect,
                    });
                    dev.connected = connect;
                    state.settings.bt_status = if connect {
                        format!("Connecting {}...", dev.name)
                    } else {
                        format!("Disconnecting {}...", dev.name)
                    };
                }
            }
            OverlayClick::ControlToggleTiling => {
                state.toggle_tiling();
            }
            OverlayClick::ControlCycleMinimize => {
                state.animations.minimize_effect = state.animations.minimize_effect.next();
                state.panel.save_full_config(
                    state.pointer_sensitivity,
                    state.natural_scroll,
                    state.animations.enabled,
                    state.animations.speed,
                    state.animations.minimize_effect,
                    &state.settings.xkb_layout,
                );
            }
            OverlayClick::ControlSetVolume(vol) => {
                state.panel.set_volume(vol);
            }
            OverlayClick::ControlOpenSettings => {
                state.overlays.control_center_open = false;
                state.settings.open = true;
                state.settings.refresh_system_info();
            }
            OverlayClick::DesktopIconClick(idx) => {
                // Only interact with desktop icons if no window covers this point
                if state.window_under(pos).is_some() {
                    return false;
                }
                state.overlays.context_menu_pos = None;
                state.overlays.icon_context_menu = None;
                state.overlays.selected_icons.clear();
                state.overlays.selected_icons.push(idx);
                state.overlays.dragging_icon = Some(idx);

                let now = std::time::Instant::now();
                let is_double_click = state
                    .overlays
                    .last_icon_click
                    .map(|(last_idx, last_t)| {
                        last_idx == idx && now.duration_since(last_t).as_millis() <= 420
                    })
                    .unwrap_or(false);

                if is_double_click {
                    state.overlays.last_icon_click = None;
                    if let Some(icon) = state.overlays.desktop_icons.get(idx).cloned() {
                        launch_desktop_icon(state, &icon);
                    }
                } else {
                    state.overlays.last_icon_click = Some((idx, now));
                }
            }
            OverlayClick::IconCtxOpen(idx) => {
                state.overlays.icon_context_menu = None;
                if let Some(icon) = state.overlays.desktop_icons.get(idx).cloned() {
                    launch_desktop_icon(state, &icon);
                }
            }
            OverlayClick::IconCtxPin(idx) => {
                state.overlays.icon_context_menu = None;
                if let Some(icon) = state.overlays.desktop_icons.get(idx).cloned() {
                    let exec = match &icon.action {
                        DesktopIconAction::OpenHome => "xdg-open ~".to_string(),
                        DesktopIconAction::OpenTerminal => "foot".to_string(),
                        DesktopIconAction::OpenSettings => "okay-settings".to_string(),
                        DesktopIconAction::TakeScreenshot => "grim".to_string(),
                        DesktopIconAction::OpenPath(p) => icon
                            .exec_cmd
                            .clone()
                            .unwrap_or_else(|| format!("xdg-open '{}'", p.display())),
                    };
                    if !state.panel.custom_pinned.iter().any(|p| p.name == icon.label) {
                        state.panel.custom_pinned.push(crate::panel::DesktopAppEntry {
                            name: icon.label.clone(),
                            exec,
                            icon_name: icon.icon_name.clone(),
                            category: "Pinned".to_string(),
                        });
                        state.panel.save_full_config(
                            state.pointer_sensitivity,
                            state.natural_scroll,
                            state.animations.enabled,
                            state.animations.speed,
                            state.animations.minimize_effect,
                            &state.settings.xkb_layout,
                        );
                    }
                    state.overlays.push_notification(
                        "Pinned to Dock",
                        format!("Added '{}' to your Dock", icon.label),
                    );
                }
            }
            OverlayClick::IconCtxProperties(idx) => {
                state.overlays.icon_context_menu = None;
                if let Some(icon) = state.overlays.desktop_icons.get(idx).cloned() {
                    let info = match &icon.action {
                        DesktopIconAction::OpenPath(p) => p.to_string_lossy().into_owned(),
                        _ => "Built-in Desktop Shortcut".to_string(),
                    };
                    state
                        .overlays
                        .push_notification(format!("Properties: {}", icon.label), info);
                }
            }
            OverlayClick::IconCtxDelete(idx) => {
                state.overlays.icon_context_menu = None;
                if let Some(icon) = state.overlays.desktop_icons.get(idx).cloned() {
                    match &icon.action {
                        DesktopIconAction::OpenPath(p) => {
                            let src = PathBuf::from(p);
                            if let Ok(home) = std::env::var("HOME") {
                                let trash_dir = PathBuf::from(home).join(".local/share/Trash/files");
                                let _ = fs::create_dir_all(&trash_dir);
                                if let Some(fname) = src.file_name() {
                                    let _ = fs::rename(&src, trash_dir.join(fname));
                                }
                            }
                            state.overlays.selected_icons.clear();
                            state.overlays.refresh_desktop_icons();
                            state
                                .overlays
                                .push_notification("Moved to Trash", format!("Removed '{}'", icon.label));
                        }
                        _ => {
                            state.overlays.push_notification(
                                "System Shortcut",
                                "Built-in desktop shortcuts cannot be deleted",
                            );
                        }
                    }
                }
            }
            OverlayClick::ContextNewFolder => {
                state.overlays.context_menu_pos = None;
                state.overlays.begin_creating_folder();
            }
            OverlayClick::ContextOpenTerminal => {
                state.overlays.context_menu_pos = None;
                crate::panel::spawn_terminal(&state.socket_name);
            }
            OverlayClick::ContextReloadWallpaper => {
                state.overlays.context_menu_pos = None;
                state.desktop.reload();
                state
                    .overlays
                    .push_notification("Wallpaper", "Reloaded desktop wallpaper");
            }
            OverlayClick::ContextToggleIcons => {
                state.overlays.context_menu_pos = None;
                state.overlays.show_desktop_icons = !state.overlays.show_desktop_icons;
            }
            OverlayClick::ContextDisplaySettings => {
                state.overlays.context_menu_pos = None;
                state.settings.open = true;
                state.settings.active_tab = SettingsTab::Displays;
                state.settings.refresh_system_info();
            }
            OverlayClick::ContextOkaySettings => {
                state.overlays.context_menu_pos = None;
                state.settings.open = true;
                state.settings.active_tab = SettingsTab::Appearance;
                state.settings.refresh_system_info();
            }
        }
        return true;
    }

    if state.overlays.context_menu_pos.is_some() || state.overlays.icon_context_menu.is_some() {
        state.overlays.context_menu_pos = None;
        state.overlays.icon_context_menu = None;
        return true;
    }
    if state.overlays.calendar_open {
        state.overlays.calendar_open = false;
    }
    if state.overlays.notif_drawer_open {
        state.overlays.notif_drawer_open = false;
    }
    if state.overlays.clipboard_open {
        state.overlays.clipboard_open = false;
    }
    if state.overlays.control_center_open {
        state.overlays.control_center_open = false;
    }

    false
}

/// Renders topmost overlays: Lockscreen, Alt+Tab switcher, Toasts, Notification Drawer,
/// Calendar Popup, Clipboard Manager, and Desktop Right-Click Context Menu.
pub fn render_top_overlays(
    state: &mut OkayState,
    renderer: &mut GlesRenderer,
    output: &Output,
    scale: Scale<f64>,
) -> Vec<OkayRenderElements> {
    let is_primary = state
        .space
        .outputs()
        .next()
        .map(|o| o == output)
        .unwrap_or(false);
    let Some(output_geo) = state.space.output_geometry(output) else {
        return Vec::new();
    };

    state.overlays.poll_background();
    if is_primary {
        state.overlays.hit_regions.clear();
    }

    let accent = state.panel.accent_rgb();
    let mut elements = Vec::new();

    // 1. Full-screen Lockscreen (`Super + L`)
    if state.overlays.locked {
        let w = output_geo.size.w.max(320);
        let h = output_geo.size.h.max(240);
        let mut canvas = Canvas::new(w, h);
        canvas.fill_rect(0, 0, w, h, [12, 12, 20, 252]);

        let card_w = 380;
        let card_h = 200;
        let cx = (w - card_w) / 2;
        let cy = (h - card_h) / 2;
        canvas.fill_glass_panel(cx, cy, card_w, card_h, 16);

        canvas.draw_text_px(
            cx + 115,
            cy + 24,
            "OKay DE Locked",
            18.0,
            [246, 248, 255, 255],
        );
        canvas.draw_text_px(
            cx + 52,
            cy + 64,
            "Type password or press Enter to unlock session",
            13.0,
            [200, 205, 228, 255],
        );

        canvas.fill_rounded_rect_aa(cx + 40, cy + 104, card_w - 80, 36, 8, [16, 16, 26, 255]);
        let bullets: String = std::iter::repeat('*')
            .take(state.overlays.lock_input.len().max(1))
            .collect();
        canvas.draw_text_px(cx + 56, cy + 113, &bullets, 16.0, [240, 242, 252, 255]);

        if let Some(elem) = upload_canvas(&canvas, (0, 0), renderer, scale) {
            elements.push(elem);
        }
        return elements;
    }

    if !is_primary {
        return elements;
    }

    // 2. Right-Click Desktop Context Menu (Polished Dark Glass)
    if let Some(menu_pos) = state.overlays.context_menu_pos {
        let mw = 264;
        let mh = 232;
        let mx = (menu_pos.x - output_geo.loc.x).clamp(8, (output_geo.size.w - mw - 8).max(8));
        let my = (menu_pos.y - output_geo.loc.y).clamp(8, (output_geo.size.h - mh - 8).max(8));

        let mut canvas = Canvas::new(mw, mh);
        canvas.fill_glass_panel(0, 0, mw, mh, 10);

        // Label, optional shortcut hint, action, draw_separator_after
        let items: [(&str, &str, OverlayClick, bool); 6] = [
            ("New Folder on Desktop", "", OverlayClick::ContextNewFolder, false),
            ("Open Terminal Here", "Super+Ret", OverlayClick::ContextOpenTerminal, true),
            ("Reload Wallpaper", "", OverlayClick::ContextReloadWallpaper, false),
            ("Toggle Desktop Icons", "", OverlayClick::ContextToggleIcons, true),
            ("Display Settings", "", OverlayClick::ContextDisplaySettings, false),
            ("OKay Settings", "Super+I", OverlayClick::ContextOkaySettings, false),
        ];

        let mut iy = 10;
        for (label, hint, act, sep_after) in items {
            let row_rect = Rectangle::new(
                (output_geo.loc.x + mx + 8, output_geo.loc.y + my + iy).into(),
                (mw - 16, 30).into(),
            );
            let hovered = rect_contains(
                row_rect,
                state.pointer_location.x.round() as i32,
                state.pointer_location.y.round() as i32,
            );
            if hovered {
                canvas.fill_rounded_rect_aa(8, iy, mw - 16, 30, 6, [accent[0], accent[1], accent[2], 225]);
            }
            let fg = if hovered {
                contrast_fg(accent)
            } else {
                [238, 240, 252, 255]
            };
            let hint_fg = if hovered {
                contrast_fg(accent)
            } else {
                [150, 158, 185, 220]
            };
            canvas.draw_text_px(18, iy + 7, label, 12.5, fg);
            if !hint.is_empty() {
                let hw = canvas.measure_text_px(hint, 11.0);
                canvas.draw_text_px(mw - 18 - hw, iy + 8, hint, 11.0, hint_fg);
            }
            state.overlays.hit_regions.push((row_rect, act));
            iy += 32;
            if sep_after {
                canvas.fill_rect(14, iy + 3, mw - 28, 1, [255, 255, 255, 24]);
                iy += 8;
            }
        }

        if let Some(elem) = upload_canvas(&canvas, (mx, my), renderer, scale) {
            elements.push(elem);
        }
    }

    // 2b. Dedicated Desktop Icon Right-Click Context Menu
    if let Some((menu_pos, icon_idx)) = state.overlays.icon_context_menu {
        let mw = 236;
        let mh = 160;
        let mx = (menu_pos.x - output_geo.loc.x).clamp(8, (output_geo.size.w - mw - 8).max(8));
        let my = (menu_pos.y - output_geo.loc.y).clamp(8, (output_geo.size.h - mh - 8).max(8));

        let mut canvas = Canvas::new(mw, mh);
        canvas.fill_glass_panel(0, 0, mw, mh, 10);

        let items: [(&str, &str, OverlayClick, bool, bool); 4] = [
            ("Open", "DblClick", OverlayClick::IconCtxOpen(icon_idx), false, false),
            ("Pin to Dock", "", OverlayClick::IconCtxPin(icon_idx), false, true),
            ("Properties", "", OverlayClick::IconCtxProperties(icon_idx), false, true),
            ("Move to Trash", "Del", OverlayClick::IconCtxDelete(icon_idx), true, false),
        ];

        let mut iy = 10;
        for (label, hint, act, is_danger, sep_after) in items {
            let row_rect = Rectangle::new(
                (output_geo.loc.x + mx + 8, output_geo.loc.y + my + iy).into(),
                (mw - 16, 30).into(),
            );
            let hovered = rect_contains(
                row_rect,
                state.pointer_location.x.round() as i32,
                state.pointer_location.y.round() as i32,
            );
            if hovered {
                let hcol = if is_danger {
                    [215, 72, 82, 230]
                } else {
                    [accent[0], accent[1], accent[2], 225]
                };
                canvas.fill_rounded_rect_aa(8, iy, mw - 16, 30, 6, hcol);
            }
            let fg = if hovered {
                if is_danger {
                    [255, 255, 255, 255]
                } else {
                    contrast_fg(accent)
                }
            } else if is_danger {
                [248, 128, 138, 255]
            } else {
                [238, 240, 252, 255]
            };
            let hint_fg = if hovered {
                fg
            } else {
                [150, 158, 185, 220]
            };
            canvas.draw_text_px(18, iy + 7, label, 12.5, fg);
            if !hint.is_empty() {
                let hw = canvas.measure_text_px(hint, 11.0);
                canvas.draw_text_px(mw - 18 - hw, iy + 8, hint, 11.0, hint_fg);
            }
            state.overlays.hit_regions.push((row_rect, act));
            iy += 32;
            if sep_after {
                canvas.fill_rect(14, iy + 3, mw - 28, 1, [255, 255, 255, 24]);
                iy += 8;
            }
        }

        if let Some(elem) = upload_canvas(&canvas, (mx, my), renderer, scale) {
            elements.push(elem);
        }
    }

    // 2c. Desktop Rubber-Band Selection Box
    if let (Some(start), Some(cur)) = (
        state.overlays.rubberband_start,
        state.overlays.rubberband_current,
    ) {
        let sx = start.x.round() as i32;
        let sy = start.y.round() as i32;
        let cx = cur.x.round() as i32;
        let cy = cur.y.round() as i32;
        let rx = (sx.min(cx) - output_geo.loc.x).max(0);
        let ry = (sy.min(cy) - output_geo.loc.y).max(0);
        let rw = (sx - cx).abs().max(2);
        let rh = (sy - cy).abs().max(2);
        if rw >= 4 && rh >= 4 {
            let mut canvas = Canvas::new(rw, rh);
            canvas.fill_rounded_rect_aa(0, 0, rw, rh, 6, [100, 160, 255, 56]);
            canvas.stroke_rect(0, 0, rw, rh, [120, 180, 255, 210]);
            if let Some(elem) = upload_canvas(&canvas, (rx, ry), renderer, scale) {
                elements.push(elem);
            }
        }
    }

    // 3. Alt+Tab Window Switcher Overlay
    if state.overlays.alttab_visible_until.is_some() {
        let wins: Vec<(String, bool)> = state
            .space
            .elements()
            .filter(|w| state.workspaces.is_on_active_workspace(w))
            .map(|w| {
                let is_focused = state.focused_window().as_ref() == Some(w);
                (window_title_str(w), is_focused)
            })
            .collect();

        if !wins.is_empty() {
            let sw = 380;
            let sh = 48 + wins.len() as i32 * 36;
            let sx = (output_geo.size.w - sw) / 2;
            let sy = (output_geo.size.h - sh) / 2;
            let mut canvas = Canvas::new(sw, sh);
            canvas.fill_glass_panel(0, 0, sw, sh, 14);
            canvas.draw_text_px(
                16,
                12,
                "Window Switcher (Alt+Tab)",
                14.0,
                [244, 246, 254, 255],
            );

            for (i, (title, focused)) in wins.iter().enumerate() {
                let ry = 40 + i as i32 * 34;
                let bg = if *focused {
                    [accent[0], accent[1], accent[2], 235]
                } else {
                    [255, 255, 255, 16]
                };
                let fg = if *focused {
                    contrast_fg(accent)
                } else {
                    [235, 238, 250, 255]
                };
                canvas.fill_rounded_rect_aa(10, ry, sw - 20, 30, 8, bg);
                let short: String = title.chars().take(38).collect();
                canvas.draw_text_px(20, ry + 7, &short, 13.5, fg);
            }

            if let Some(elem) = upload_canvas(&canvas, (sx, sy), renderer, scale) {
                elements.push(elem);
            }
        }
    }

    // 4. Clipboard History Manager (`Super + V`)
    if state.overlays.clipboard_open {
        let cw = 380;
        let ch = 68 + (state.overlays.clipboard_items.len().max(1) as i32 * 36);
        let cx = (output_geo.size.w - cw) / 2;
        let cy = (output_geo.size.h - ch) / 2;
        let ptr_x = state.pointer_location.x.round() as i32;
        let ptr_y = state.pointer_location.y.round() as i32;

        let mut canvas = Canvas::new(cw, ch);
        canvas.fill_glass_panel(0, 0, cw, ch, 14);
        canvas.draw_text_px(
            16,
            14,
            "Clipboard History",
            14.5,
            [244, 246, 254, 255],
        );
        let badge = format!("{} items", state.overlays.clipboard_items.len());
        let bw = canvas.measure_text_px(&badge, 11.0);
        canvas.fill_rounded_rect_aa(cw - bw - 28, 12, bw + 14, 20, 6, [accent[0], accent[1], accent[2], 36]);
        canvas.draw_text_px(cw - bw - 21, 15, &badge, 11.0, [accent[0], accent[1], accent[2], 245]);

        for (idx, item) in state.overlays.clipboard_items.iter().enumerate() {
            let iy = 48 + idx as i32 * 36;
            let row_rect = Rectangle::new(
                (output_geo.loc.x + cx + 12, output_geo.loc.y + cy + iy).into(),
                (cw - 24, 30).into(),
            );
            let hovered = rect_contains(row_rect, ptr_x, ptr_y);
            let row_bg = if hovered {
                [accent[0], accent[1], accent[2], 54]
            } else {
                [255, 255, 255, 14]
            };
            canvas.fill_rounded_rect_aa(12, iy, cw - 24, 30, 8, row_bg);
            if hovered || idx == 0 {
                canvas.fill_rounded_rect_aa(14, iy + 5, 3, 20, 2, [accent[0], accent[1], accent[2], 235]);
            }
            let short: String = item.chars().take(42).collect();
            canvas.draw_text_px(24, iy + 7, &short, 12.5, [238, 240, 252, 255]);
            state
                .overlays
                .hit_regions
                .push((row_rect, OverlayClick::SelectClipboard(idx)));
        }

        if let Some(elem) = upload_canvas(&canvas, (cx, cy), renderer, scale) {
            elements.push(elem);
        }
    }

    // 5. Calendar Popup (when clicking taskbar clock)
    if state.overlays.calendar_open {
        let cal_w = 276;
        let cal_h = 244;
        let cal_x = (output_geo.size.w - cal_w - 12).max(8);
        let cal_y = if state.panel.dock_mode || state.panel.panel_top {
            PANEL_HEIGHT + 8
        } else {
            (output_geo.size.h - 36 - cal_h - 8).max(8)
        };

        let mut canvas = Canvas::new(cal_w, cal_h);
        canvas.fill_glass_panel(0, 0, cal_w, cal_h, 14);

        let (date_header, cur_day) = current_date_info();
        canvas.draw_text_px(16, 14, &date_header, 14.5, [246, 248, 255, 255]);
        canvas.fill_rounded_rect_aa(12, 38, cal_w - 24, 24, 6, [255, 255, 255, 12]);
        canvas.draw_text_px(
            18,
            43,
            "Mo   Tu   We   Th   Fr   Sa   Su",
            12.0,
            [168, 174, 200, 245],
        );

        for day in 1..=30 {
            let col = (day - 1) % 7;
            let row = (day - 1) / 7;
            let dx = 16 + col * 35;
            let dy = 72 + row * 31;
            if day == cur_day {
                canvas.fill_rounded_rect_aa(dx - 4, dy - 3, 28, 24, 7, [accent[0], accent[1], accent[2], 240]);
                canvas.draw_text_px(dx + 2, dy, &format!("{:2}", day), 12.5, contrast_fg(accent));
            } else {
                canvas.draw_text_px(dx + 2, dy, &format!("{:2}", day), 12.5, [230, 234, 248, 250]);
            }
        }

        state.overlays.hit_regions.push((
            Rectangle::new(
                (output_geo.loc.x + cal_x, output_geo.loc.y + cal_y).into(),
                (cal_w, cal_h).into(),
            ),
            OverlayClick::CloseCalendar,
        ));

        if let Some(elem) = upload_canvas(&canvas, (cal_x, cal_y), renderer, scale) {
            elements.push(elem);
        }
    }

    // 6. Notification Center Drawer
    if state.overlays.notif_drawer_open {
        let nw = 336;
        let nh = 304;
        let nx = (output_geo.size.w - nw - 12).max(8);
        let ny = if state.panel.dock_mode || state.panel.panel_top {
            PANEL_HEIGHT + 8
        } else {
            (output_geo.size.h - 36 - nh - 8).max(8)
        };
        let ptr_x = state.pointer_location.x.round() as i32;
        let ptr_y = state.pointer_location.y.round() as i32;

        let mut canvas = Canvas::new(nw, nh);
        canvas.fill_glass_panel(0, 0, nw, nh, 14);
        canvas.draw_text_px(
            16,
            14,
            "Notification Center",
            14.5,
            [246, 248, 255, 255],
        );

        let clear_rect = Rectangle::new(
            (output_geo.loc.x + nx + nw - 78, output_geo.loc.y + ny + 11).into(),
            (64, 24).into(),
        );
        let clear_bg = if rect_contains(clear_rect, ptr_x, ptr_y) {
            [accent[0], accent[1], accent[2], 68]
        } else {
            [255, 255, 255, 20]
        };
        canvas.fill_rounded_rect_aa(nw - 78, 11, 64, 24, 7, clear_bg);
        canvas.draw_text_px(nw - 63, 15, "Clear", 12.0, [238, 240, 252, 255]);
        state
            .overlays
            .hit_regions
            .push((clear_rect, OverlayClick::ClearNotifications));

        if state.overlays.history.is_empty() {
            canvas.draw_text_px(16, 62, "No recent notifications.", 12.5, [156, 162, 188, 235]);
        } else {
            for (i, (title, body)) in state.overlays.history.iter().take(4).enumerate() {
                let iy = 48 + i as i32 * 60;
                canvas.fill_rounded_rect_aa(12, iy, nw - 24, 52, 10, [255, 255, 255, 14]);
                canvas.fill_rounded_rect_aa(14, iy + 8, 3, 36, 2, [accent[0], accent[1], accent[2], 235]);
                let st: String = title.chars().take(33).collect();
                let sb: String = body.chars().take(37).collect();
                canvas.draw_text_px(24, iy + 8, &st, 12.5, [242, 244, 255, 255]);
                canvas.draw_text_px(24, iy + 28, &sb, 11.5, [178, 184, 208, 240]);
            }
        }

        state.overlays.hit_regions.push((
            Rectangle::new(
                (output_geo.loc.x + nx, output_geo.loc.y + ny).into(),
                (nw, nh).into(),
            ),
            OverlayClick::CloseNotifDrawer,
        ));

        if let Some(elem) = upload_canvas(&canvas, (nx, ny), renderer, scale) {
            elements.push(elem);
        }
    }

    // 6b. Compact macOS Control Center Popup (with expandable Wi-Fi & Bluetooth lists)
    if state.overlays.control_center_open {
        let wifi_expanded = state.overlays.cc_wifi_expanded;
        let bt_expanded = state.overlays.cc_bt_expanded;
        let list_rows = if wifi_expanded {
            state.settings.wifi_networks.len().clamp(1, 5) as i32
        } else if bt_expanded {
            state.settings.bt_devices.len().clamp(1, 5) as i32
        } else {
            0
        };
        let extra_h = if wifi_expanded || bt_expanded {
            40 + list_rows * 32
        } else {
            0
        };

        let cc_w = 310;
        let cc_h = 266 + extra_h;
        let cc_x = (output_geo.size.w - cc_w - 12).max(8);
        let cc_y = if state.panel.dock_mode || state.panel.panel_top {
            PANEL_HEIGHT + 8
        } else {
            (output_geo.size.h - 36 - cc_h - 8).max(8)
        };
        let ptr_x = state.pointer_location.x.round() as i32;
        let ptr_y = state.pointer_location.y.round() as i32;

        let mut canvas = Canvas::new(cc_w, cc_h);
        canvas.fill_glass_panel(0, 0, cc_w, cc_h, 16);

        // Connectivity card (Wi-Fi, Bluetooth, Tiling)
        canvas.fill_rounded_rect_aa(12, 12, cc_w - 24, 116, 12, [255, 255, 255, 14]);
        let wifi_on = !state.settings.wifi_status.to_lowercase().contains("off");
        let toggles: [(&str, &str, bool, OverlayClick, Option<(bool, OverlayClick)>); 3] = [
            (
                "Wi-Fi",
                if wifi_on { "Connected" } else { "Off" },
                wifi_on,
                OverlayClick::ControlToggleWifi,
                Some((wifi_expanded, OverlayClick::ControlExpandWifi)),
            ),
            (
                "Bluetooth",
                if state.settings.bt_powered { "On" } else { "Off" },
                state.settings.bt_powered,
                OverlayClick::ControlToggleBt,
                Some((bt_expanded, OverlayClick::ControlExpandBt)),
            ),
            (
                "Layout",
                state.workspaces.layout_mode.short_label(),
                state.workspaces.tiling_enabled(),
                OverlayClick::ControlToggleTiling,
                None,
            ),
        ];
        for (i, (lbl, sub, active, act, expand_opt)) in toggles.into_iter().enumerate() {
            let ry = 18 + i as i32 * 35;
            let main_w = if expand_opt.is_some() { cc_w - 68 } else { cc_w - 32 };
            let row_rect = Rectangle::new(
                (output_geo.loc.x + cc_x + 16, output_geo.loc.y + cc_y + ry).into(),
                (main_w, 32).into(),
            );
            if rect_contains(row_rect, ptr_x, ptr_y) {
                canvas.fill_rounded_rect_aa(16, ry, main_w, 32, 8, [255, 255, 255, 18]);
            }
            let pill_col = if active { [accent[0], accent[1], accent[2], 240] } else { [255, 255, 255, 30] };
            let pill_fg = if active { contrast_fg(accent) } else { [220, 224, 240, 255] };
            canvas.fill_circle_aa(34.0, (ry + 16) as f32, 12.0, pill_col);
            canvas.draw_text_px(27, ry + 9, &lbl[..2], 11.0, pill_fg);
            canvas.draw_text_px(54, ry + 3, lbl, 12.5, [244, 246, 255, 255]);
            canvas.draw_text_px(54, ry + 18, sub, 10.5, [158, 166, 192, 230]);
            state.overlays.hit_regions.push((row_rect, act));

            if let Some((is_exp, exp_act)) = expand_opt {
                let exp_x = cc_w - 46;
                let exp_rect = Rectangle::new(
                    (output_geo.loc.x + cc_x + exp_x, output_geo.loc.y + cc_y + ry + 3).into(),
                    (30, 26).into(),
                );
                let exp_bg = if is_exp {
                    [accent[0], accent[1], accent[2], 78]
                } else if rect_contains(exp_rect, ptr_x, ptr_y) {
                    [255, 255, 255, 32]
                } else {
                    [255, 255, 255, 16]
                };
                canvas.fill_rounded_rect_aa(exp_x, ry + 3, 30, 26, 7, exp_bg);
                let glyph = if is_exp { "v" } else { ">" };
                canvas.draw_text_px(exp_x + 11, ry + 8, glyph, 12.0, [238, 240, 252, 255]);
                state.overlays.hit_regions.push((exp_rect, exp_act));
            }
        }

        // Expanded Wi-Fi Networks or Bluetooth Devices sub-panel
        if wifi_expanded || bt_expanded {
            let sub_y = 136;
            let sub_h = extra_h - 8;
            canvas.fill_rounded_rect_aa(12, sub_y, cc_w - 24, sub_h, 12, [255, 255, 255, 12]);
            let header_txt = if wifi_expanded {
                &state.settings.wifi_status
            } else {
                &state.settings.bt_status
            };
            let short_hdr: String = header_txt.chars().take(34).collect();
            canvas.draw_text_px(22, sub_y + 8, &short_hdr, 11.5, [accent[0], accent[1], accent[2], 245]);

            if wifi_expanded {
                if state.settings.wifi_networks.is_empty() {
                    canvas.draw_text_px(22, sub_y + 34, "Scanning Wi-Fi networks...", 11.5, [168, 174, 200, 235]);
                } else {
                    for (idx, net) in state.settings.wifi_networks.iter().take(5).enumerate() {
                        let iy = sub_y + 28 + idx as i32 * 32;
                        let item_rect = Rectangle::new(
                            (output_geo.loc.x + cc_x + 18, output_geo.loc.y + cc_y + iy).into(),
                            (cc_w - 36, 28).into(),
                        );
                        let hovered = rect_contains(item_rect, ptr_x, ptr_y);
                        let row_bg = if net.in_use {
                            [accent[0], accent[1], accent[2], 54]
                        } else if hovered {
                            [255, 255, 255, 22]
                        } else {
                            [255, 255, 255, 10]
                        };
                        canvas.fill_rounded_rect_aa(18, iy, cc_w - 36, 28, 7, row_bg);
                        let short_ssid: String = net.ssid.chars().take(20).collect();
                        canvas.draw_text_px(26, iy + 7, &short_ssid, 12.0, [240, 242, 252, 255]);
                        let sig = if net.in_use {
                            format!("{}% *", net.signal)
                        } else {
                            format!("{}%", net.signal)
                        };
                        let sw = canvas.measure_text_px(&sig, 11.0);
                        canvas.draw_text_px(cc_w - 26 - sw, iy + 8, &sig, 11.0, [178, 186, 212, 240]);
                        state.overlays.hit_regions.push((item_rect, OverlayClick::ControlConnectWifi(idx)));
                    }
                }
            } else if state.settings.bt_devices.is_empty() {
                canvas.draw_text_px(22, sub_y + 34, "No paired Bluetooth devices", 11.5, [168, 174, 200, 235]);
            } else {
                for (idx, dev) in state.settings.bt_devices.iter().take(5).enumerate() {
                    let iy = sub_y + 28 + idx as i32 * 32;
                    let item_rect = Rectangle::new(
                        (output_geo.loc.x + cc_x + 18, output_geo.loc.y + cc_y + iy).into(),
                        (cc_w - 36, 28).into(),
                    );
                    let hovered = rect_contains(item_rect, ptr_x, ptr_y);
                    let row_bg = if dev.connected {
                        [accent[0], accent[1], accent[2], 54]
                    } else if hovered {
                        [255, 255, 255, 22]
                    } else {
                        [255, 255, 255, 10]
                    };
                    canvas.fill_rounded_rect_aa(18, iy, cc_w - 36, 28, 7, row_bg);
                    let short_name: String = dev.name.chars().take(20).collect();
                    canvas.draw_text_px(26, iy + 7, &short_name, 12.0, [240, 242, 252, 255]);
                    let st = if dev.connected { "On" } else { "Pair" };
                    let sw = canvas.measure_text_px(st, 11.0);
                    canvas.draw_text_px(cc_w - 26 - sw, iy + 8, st, 11.0, [178, 186, 212, 240]);
                    state.overlays.hit_regions.push((item_rect, OverlayClick::ControlConnectBt(idx)));
                }
            }
        }

        // Sound Volume Slider Card
        let vol_y = 136 + extra_h;
        canvas.fill_rounded_rect_aa(12, vol_y, cc_w - 24, 64, 12, [255, 255, 255, 14]);
        let vol_lbl = if state.panel.volume_muted {
            "Sound  (Muted)".to_string()
        } else {
            format!("Sound  ({}%)", state.panel.volume_percent)
        };
        canvas.draw_text_px(24, vol_y + 8, &vol_lbl, 12.0, [240, 242, 252, 255]);
        let track_x = 24;
        let track_y = vol_y + 35;
        let track_w = cc_w - 48;
        canvas.fill_rounded_rect_aa(track_x, track_y, track_w, 10, 5, [255, 255, 255, 28]);
        let fill_w = ((track_w as f32 * state.panel.volume_percent as f32 / 100.0).round() as i32).max(10);
        canvas.fill_rounded_rect_aa(track_x, track_y, fill_w, 10, 5, [accent[0], accent[1], accent[2], 235]);
        let knob_x = (track_x + fill_w).clamp(track_x + 6, track_x + track_w - 6);
        canvas.fill_circle_aa(knob_x as f32, (track_y + 5) as f32, 7.0, [248, 250, 255, 255]);
        for step in [0, 25, 50, 75, 100] {
            let sx = track_x + (step * track_w / 100) - (track_w / 10);
            state.overlays.hit_regions.push((
                Rectangle::new(
                    (output_geo.loc.x + cc_x + sx.max(track_x), output_geo.loc.y + cc_y + vol_y + 24).into(),
                    ((track_w / 5).max(36), 28).into(),
                ),
                OverlayClick::ControlSetVolume(step),
            ));
        }

        // Bottom quick actions: Minimize Effect + Open System Settings
        let bot_y = 208 + extra_h;
        let min_lbl = match state.animations.minimize_effect {
            crate::animation::MinimizeEffect::MagicLamp => "FX: Genie",
            crate::animation::MinimizeEffect::ScaleGlide => "FX: Scale",
            crate::animation::MinimizeEffect::Off => "FX: Off",
        };
        let fx_rect = Rectangle::new(
            (output_geo.loc.x + cc_x + 12, output_geo.loc.y + cc_y + bot_y).into(),
            (136, 42).into(),
        );
        let fx_bg = if rect_contains(fx_rect, ptr_x, ptr_y) {
            [255, 255, 255, 32]
        } else {
            [255, 255, 255, 16]
        };
        canvas.fill_rounded_rect_aa(12, bot_y, 136, 42, 10, fx_bg);
        canvas.draw_text_px(28, bot_y + 14, min_lbl, 12.0, [240, 242, 252, 255]);
        state
            .overlays
            .hit_regions
            .push((fx_rect, OverlayClick::ControlCycleMinimize));

        let set_rect = Rectangle::new(
            (output_geo.loc.x + cc_x + 156, output_geo.loc.y + cc_y + bot_y).into(),
            (142, 42).into(),
        );
        let set_alpha = if rect_contains(set_rect, ptr_x, ptr_y) { 250 } else { 225 };
        canvas.fill_rounded_rect_aa(156, bot_y, 142, 42, 10, [accent[0], accent[1], accent[2], set_alpha]);
        canvas.draw_text_px(184, bot_y + 14, "Settings...", 12.0, contrast_fg(accent));
        state
            .overlays
            .hit_regions
            .push((set_rect, OverlayClick::ControlOpenSettings));

        if let Some(elem) = upload_canvas(&canvas, (cc_x, cc_y), renderer, scale) {
            elements.push(elem);
        }
    }

    // 7. Active Toast Notifications in Top-Right Corner
    for (idx, toast) in state.overlays.notifications.iter().take(3).enumerate() {
        let tw = 316;
        let th = 64;
        let tx = (output_geo.size.w - tw - 14).max(8);
        let top_offset = if state.panel.dock_mode || state.panel.panel_top {
            PANEL_HEIGHT + 12
        } else {
            14
        };
        let ty = top_offset + idx as i32 * (th + 8);

        let mut canvas = Canvas::new(tw, th);
        canvas.fill_glass_panel(0, 0, tw, th, 12);
        canvas.fill_rounded_rect_aa(6, 10, 3, th - 20, 2, [accent[0], accent[1], accent[2], 235]);
        let st: String = toast.title.chars().take(32).collect();
        let sb: String = toast.body.chars().take(38).collect();
        canvas.draw_text_px(16, 12, &st, 13.0, [246, 248, 255, 255]);
        canvas.draw_text_px(16, 33, &sb, 11.5, [186, 194, 218, 240]);

        state.overlays.hit_regions.push((
            Rectangle::new(
                (output_geo.loc.x + tx, output_geo.loc.y + ty).into(),
                (tw, th).into(),
            ),
            OverlayClick::DismissToast(idx),
        ));

        if let Some(elem) = upload_canvas(&canvas, (tx, ty), renderer, scale) {
            elements.push(elem);
        }
    }

    elements
}

/// Renders the Desktop Shortcut/File Icons Grid directly above the desktop wallpaper.
pub fn render_desktop_icons(
    state: &mut OkayState,
    renderer: &mut GlesRenderer,
    output: &Output,
    scale: Scale<f64>,
) -> Vec<OkayRenderElements> {
    if !state.overlays.show_desktop_icons {
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

    let mut elements = Vec::new();
    let top_margin = if state.panel.dock_mode || state.panel.panel_top {
        PANEL_HEIGHT + 18
    } else {
        20
    };

    for (idx, icon) in state.overlays.desktop_icons.iter().enumerate() {
        let ix = 20 + icon.grid_col * 98;
        let iy = top_margin + icon.grid_row * 94;
        let iw = 90;
        let ih = 86;

        let mut canvas = Canvas::new(iw, ih);
        let is_selected = state.overlays.selected_icons.contains(&idx);
        if is_selected {
            canvas.fill_rounded_rect_aa(2, 0, iw - 4, ih - 2, 10, [100, 160, 255, 68]);
            canvas.stroke_rect(2, 0, iw - 4, ih - 2, [130, 185, 255, 195]);
        }

        // Soft drop shadow behind icon
        canvas.fill_rounded_rect_aa(20, 7, 50, 50, 12, [0, 0, 0, 72]);
        crate::icons::draw_app_icon(
            &mut canvas,
            21,
            5,
            48,
            icon.icon_name.as_deref(),
            icon.kind,
        );

        let short: String = icon.label.chars().take(11).collect();
        let lw = canvas.measure_text_px(&short, 11.5);
        if is_selected {
            let pill_w = (lw + 12).min(iw - 6);
            let pill_x = ((iw - pill_w) / 2).max(3);
            canvas.fill_rounded_rect_aa(pill_x, 58, pill_w, 19, 6, [52, 120, 246, 225]);
        }
        canvas.draw_text_shadowed(((iw - lw) / 2).max(4), 60, &short, 11.5, [246, 248, 255, 255]);

        state.overlays.hit_regions.push((
            Rectangle::new(
                (output_geo.loc.x + ix, output_geo.loc.y + iy).into(),
                (iw, ih).into(),
            ),
            OverlayClick::DesktopIconClick(idx),
        ));

        if let Some(elem) = upload_canvas(&canvas, (ix, iy), renderer, scale) {
            elements.push(elem);
        }
    }

    // Inline "New Folder" icon + editable name input field
    if let Some(ref cf) = state.overlays.creating_folder {
        let ix = 20 + cf.grid_col * 98 - 7;
        let iy = top_margin + cf.grid_row * 94;
        let iw = 104;
        let ih = 88;
        let accent = state.panel.accent_rgb();

        let mut canvas = Canvas::new(iw, ih);
        canvas.fill_rounded_rect_aa(2, 0, iw - 4, ih - 2, 10, [accent[0], accent[1], accent[2], 44]);
        canvas.stroke_rect(2, 0, iw - 4, ih - 2, [accent[0], accent[1], accent[2], 210]);

        canvas.fill_rounded_rect_aa(27, 7, 50, 50, 12, [0, 0, 0, 72]);
        crate::icons::draw_app_icon(
            &mut canvas,
            28,
            5,
            48,
            Some("folder"),
            crate::icons::VectorIconKind::Folder,
        );

        // Crisp inline text input box for folder name
        canvas.fill_rounded_rect_aa(4, 57, iw - 8, 22, 6, [18, 20, 32, 242]);
        canvas.stroke_rect(4, 57, iw - 8, 22, [accent[0], accent[1], accent[2], 235]);

        let max_chars = 11;
        let display_name: String = if cf.name.chars().count() > max_chars {
            cf.name
                .chars()
                .rev()
                .take(max_chars)
                .collect::<String>()
                .chars()
                .rev()
                .collect()
        } else {
            cf.name.clone()
        };
        let label_with_caret = format!("{}|", display_name);
        let lw = canvas.measure_text_px(&label_with_caret, 11.5);
        let tx = ((iw - lw) / 2).max(8);
        canvas.draw_text_px(tx, 61, &label_with_caret, 11.5, [246, 248, 255, 255]);

        if let Some(elem) = upload_canvas(&canvas, (ix, iy), renderer, scale) {
            elements.push(elem);
        }
    }

    elements
}

fn upload_canvas(
    canvas: &Canvas,
    local_pos: (i32, i32),
    renderer: &mut GlesRenderer,
    scale: Scale<f64>,
) -> Option<OkayRenderElements> {
    let buf = MemoryRenderBuffer::from_slice(
        &canvas.pixels,
        Fourcc::Abgr8888,
        (canvas.width, canvas.height),
        1,
        Transform::Normal,
        None,
    );
    let phys_loc: Point<f64, Physical> = Point::<i32, Logical>::from(local_pos)
        .to_f64()
        .to_physical(scale);
    MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        phys_loc,
        &buf,
        None,
        None,
        Some((canvas.width, canvas.height).into()),
        Kind::Unspecified,
    )
    .ok()
    .map(OkayRenderElements::Memory)
}

fn current_date_info() -> (String, i32) {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        let months = [
            "January", "February", "March", "April", "May", "June",
            "July", "August", "September", "October", "November", "December",
        ];
        let m = months.get(tm.tm_mon as usize).copied().unwrap_or("Month");
        (
            format!("{} {}, {}", m, tm.tm_mday, 1900 + tm.tm_year),
            tm.tm_mday,
        )
    }
}
