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
    reexports::wayland_server::{backend::ObjectId, Resource},
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

#[derive(Debug, Clone)]
pub struct OsdState {
    pub title: String,
    pub value: String,
    pub progress: Option<i32>,
    pub expires_at: Instant,
}

#[derive(Debug, Clone)]
pub struct PropertiesInfo {
    pub name: String,
    pub kind_label: String,
    pub path: String,
    pub size_label: String,
}

#[derive(Debug)]
pub struct OverlaysState {
    pub notifications: Vec<NotificationToast>,
    pub history: Vec<(String, String)>,
    pub notif_drawer_open: bool,
    pub calendar_open: bool,
    pub cal_month_offset: i32,
    pub clipboard_open: bool,
    pub control_center_open: bool,
    pub overview_open: bool,
    pub cc_wifi_expanded: bool,
    pub cc_bt_expanded: bool,
    pub cc_volume_track: Option<Rectangle<i32, Logical>>,
    pub cc_dragging_volume: bool,
    pub clipboard_items: Vec<String>,
    pub alttab_visible_until: Option<Instant>,
    pub osd: Option<OsdState>,
    pub snap_preview: Option<Rectangle<i32, Logical>>,
    pub locked: bool,
    pub lock_input: String,
    pub lock_pin: String,
    pub lock_error: bool,
    pub screenshot_selecting: bool,
    pub screenshot_start: Option<Point<f64, Logical>>,
    pub screenshot_current: Option<Point<f64, Logical>>,
    pub context_menu_pos: Option<Point<i32, Logical>>,
    pub icon_context_menu: Option<(Point<i32, Logical>, usize)>,
    pub window_context_menu: Option<(Point<i32, Logical>, ObjectId)>,
    pub properties_modal: Option<PropertiesInfo>,
    pub show_desktop_icons: bool,
    pub desktop_icons: Vec<DesktopIcon>,
    pub selected_icons: Vec<usize>,
    pub last_icon_click: Option<(usize, Instant)>,
    pub rubberband_start: Option<Point<f64, Logical>>,
    pub rubberband_current: Option<Point<f64, Logical>>,
    pub dragging_icon: Option<usize>,
    pub creating_folder: Option<CreatingFolderState>,
    pub renaming_icon: Option<(usize, String)>,
    hit_regions: Vec<(Rectangle<i32, Logical>, OverlayClick)>,
}

#[derive(Debug, Clone)]
enum OverlayClick {
    DismissToast(usize),
    ClearNotifications,
    SelectClipboard(usize),
    DeleteClipboard(usize),
    ClearClipboard,
    CloseCalendar,
    CalPrevMonth,
    CalNextMonth,
    CalToday,
    CloseProperties,
    ControlToggleWifi,
    ControlToggleBt,
    ControlExpandWifi,
    ControlExpandBt,
    ControlConnectWifi(usize),
    ControlConnectBt(usize),
    ControlToggleTiling,
    ControlCycleMinimize,
    ControlOpenSettings,
    OverviewFocus(ObjectId),
    OverviewClose(ObjectId),
    OverviewDismiss,
    WinCtxMinimize(ObjectId),
    WinCtxMaximize(ObjectId),
    WinCtxPin(ObjectId),
    WinCtxCenter(ObjectId),
    WinCtxNextWs(ObjectId),
    WinCtxClose(ObjectId),
    DesktopIconClick(usize),
    IconCtxOpen(usize),
    IconCtxRename(usize),
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
            cal_month_offset: 0,
            clipboard_open: false,
            control_center_open: false,
            overview_open: false,
            cc_wifi_expanded: false,
            cc_bt_expanded: false,
            cc_volume_track: None,
            cc_dragging_volume: false,
            clipboard_items: vec![
                "Welcome to OKay DE (Super+V Clipboard)".to_string(),
                "~/.config/okay-de/config.toml".to_string(),
            ],
            alttab_visible_until: None,
            osd: None,
            snap_preview: None,
            locked: false,
            lock_input: String::new(),
            lock_pin: String::new(),
            lock_error: false,
            screenshot_selecting: false,
            screenshot_start: None,
            screenshot_current: None,
            context_menu_pos: None,
            icon_context_menu: None,
            window_context_menu: None,
            properties_modal: None,
            show_desktop_icons: true,
            desktop_icons: Vec::new(),
            selected_icons: Vec::new(),
            last_icon_click: None,
            rubberband_start: None,
            rubberband_current: None,
            dragging_icon: None,
            creating_folder: None,
            renaming_icon: None,
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
        let row;
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

    /// Shows a brief translucent HUD/OSD pill near the bottom-center of the screen.
    pub fn show_osd(&mut self, title: impl Into<String>, value: impl Into<String>, progress: Option<i32>) {
        self.osd = Some(OsdState {
            title: title.into(),
            value: value.into(),
            progress: progress.map(|p| p.clamp(0, 100)),
            expires_at: Instant::now() + Duration::from_millis(1450),
        });
    }

    /// Attempts to unlock the lockscreen; checks `lock_pin` if configured in `config.toml`.
    pub fn attempt_unlock(&mut self) {
        if self.lock_pin.is_empty() || self.lock_input == self.lock_pin {
            self.locked = false;
            self.lock_input.clear();
            self.lock_error = false;
        } else {
            self.lock_input.clear();
            self.lock_error = true;
        }
    }

    /// Starts inline renaming for a desktop file/folder icon.
    pub fn begin_renaming_icon(&mut self, idx: usize) {
        if let Some(icon) = self.desktop_icons.get(idx) {
            if matches!(icon.action, DesktopIconAction::OpenPath(_)) {
                self.renaming_icon = Some((idx, icon.label.clone()));
            } else {
                self.push_notification("System Shortcut", "Built-in shortcuts cannot be renamed");
            }
        }
    }

    /// Confirms inline desktop icon rename on disk.
    pub fn confirm_renaming_icon(&mut self) {
        let Some((idx, new_name)) = self.renaming_icon.take() else {
            return;
        };
        let sanitized: String = new_name
            .trim()
            .chars()
            .map(|c| if c == '/' || c == '\0' { '_' } else { c })
            .collect();
        if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
            return;
        }
        if let Some(icon) = self.desktop_icons.get(idx).cloned() {
            if let DesktopIconAction::OpenPath(old_path) = icon.action {
                if let Some(parent) = old_path.parent() {
                    let target = parent.join(&sanitized);
                    if target != old_path && !target.exists() && fs::rename(&old_path, &target).is_ok() {
                        self.refresh_desktop_icons();
                        self.push_notification("Renamed", format!("Renamed to '{}'", sanitized));
                    }
                }
            }
        }
    }

    pub fn cancel_renaming_icon(&mut self) {
        self.renaming_icon = None;
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
        if let Some(ref osd) = self.osd {
            if now >= osd.expires_at {
                self.osd = None;
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

fn build_screenshot_path(prefix: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let dir = PathBuf::from(home).join("Pictures/Screenshots");
    let _ = fs::create_dir_all(&dir);

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    dir.join(format!("{}-{}.png", prefix, ts))
}

/// Captures a real full-output screenshot directly from the live GLES compositor framebuffer
/// (and saves to `~/Pictures/Screenshots/` on the background worker thread).
pub fn take_screenshot(state: &mut OkayState) {
    let path = build_screenshot_path("okay-screenshot");
    state.pending_native_screenshot = Some((path, None));
}

/// Captures a real region screenshot directly from the live GLES compositor framebuffer.
pub fn take_area_screenshot(state: &mut OkayState, region: Rectangle<i32, Logical>) {
    let path = build_screenshot_path("okay-region");
    state.pending_native_screenshot = Some((path, Some(region)));
}

/// Enters interactive drag-to-select area screenshot mode (`Super + Shift + S`).
pub fn begin_area_screenshot(state: &mut OkayState) {
    state.overlays.screenshot_selecting = true;
    state.overlays.screenshot_start = None;
    state.overlays.screenshot_current = None;
    state.overlays.context_menu_pos = None;
    state.overlays.icon_context_menu = None;
}

/// Handles right-click on the desktop background or on a desktop icon.
pub fn handle_desktop_right_click(state: &mut OkayState, pos: Point<f64, Logical>) -> bool {
    if state.overlays.locked || state.overlays.screenshot_selecting {
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
    if state.overlays.screenshot_selecting {
        if state.overlays.screenshot_start.is_some() {
            state.overlays.screenshot_current = Some(pos);
        }
        return true;
    }
    if state.overlays.cc_dragging_volume {
        if let Some(track) = state.overlays.cc_volume_track {
            let px = pos.x.round() as i32;
            let pct = (((px - track.loc.x) * 100) / track.size.w.max(1)).clamp(0, 100);
            state.panel.set_volume(pct);
            return true;
        }
    }
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
    if state.overlays.screenshot_selecting {
        if let (Some(s), Some(c)) = (
            state.overlays.screenshot_start.take(),
            state.overlays.screenshot_current.take(),
        ) {
            state.overlays.screenshot_selecting = false;
            let sx = s.x.round() as i32;
            let sy = s.y.round() as i32;
            let cx = c.x.round() as i32;
            let cy = c.y.round() as i32;
            let rw = (sx - cx).abs();
            let rh = (sy - cy).abs();
            if rw >= 8 && rh >= 8 {
                let rect = Rectangle::new((sx.min(cx), sy.min(cy)).into(), (rw, rh).into());
                take_area_screenshot(state, rect);
            }
            return true;
        }
        return false;
    }
    if state.overlays.cc_dragging_volume {
        state.overlays.cc_dragging_volume = false;
        return true;
    }
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

    if state.overlays.screenshot_selecting {
        state.overlays.screenshot_start = Some(pos);
        state.overlays.screenshot_current = Some(pos);
        return true;
    }

    if state.overlays.creating_folder.is_some() {
        state.overlays.confirm_creating_folder();
        return true;
    }

    if state.overlays.renaming_icon.is_some() {
        state.overlays.confirm_renaming_icon();
        return true;
    }

    let px = pos.x.round() as i32;
    let py = pos.y.round() as i32;

    if state.overlays.control_center_open {
        if let Some(track) = state.overlays.cc_volume_track {
            if rect_contains(track, px, py) {
                let pct = (((px - track.loc.x) * 100) / track.size.w.max(1)).clamp(0, 100);
                state.panel.set_volume(pct);
                state.overlays.cc_dragging_volume = true;
                return true;
            }
        }
    }

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
            OverlayClick::DeleteClipboard(idx) => {
                if idx < state.overlays.clipboard_items.len() {
                    state.overlays.clipboard_items.remove(idx);
                }
            }
            OverlayClick::ClearClipboard => {
                state.overlays.clipboard_items.clear();
            }
            OverlayClick::CloseCalendar => {
                state.overlays.calendar_open = false;
            }
            OverlayClick::CalPrevMonth => {
                state.overlays.cal_month_offset = state.overlays.cal_month_offset.saturating_sub(1);
            }
            OverlayClick::CalNextMonth => {
                state.overlays.cal_month_offset = state.overlays.cal_month_offset.saturating_add(1);
            }
            OverlayClick::CalToday => {
                state.overlays.cal_month_offset = 0;
            }
            OverlayClick::CloseProperties => {
                state.overlays.properties_modal = None;
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
                        crate::worker::dispatch(crate::worker::WorkerRequest::DisconnectWifi {
                            ssid: net.ssid.clone(),
                        });
                        state.settings.wifi_status = format!("Disconnecting {}...", net.ssid);
                    } else {
                        crate::worker::dispatch(crate::worker::WorkerRequest::ConnectWifi {
                            ssid: net.ssid.clone(),
                            password: None,
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
            OverlayClick::ControlOpenSettings => {
                state.overlays.control_center_open = false;
                state.settings.open = true;
                state.settings.refresh_system_info();
            }
            OverlayClick::OverviewFocus(win_id) => {
                state.overlays.overview_open = false;
                let target = state
                    .space
                    .elements()
                    .find(|w| {
                        w.toplevel()
                            .map(|t| t.wl_surface().id() == win_id)
                            .unwrap_or(false)
                    })
                    .cloned();
                if let Some(win) = target {
                    if state.workspaces.is_minimized(&win) {
                        state.toggle_minimize_window(&win);
                    } else {
                        state.focus_window(&win);
                    }
                }
            }
            OverlayClick::OverviewClose(win_id) => {
                let target = state
                    .space
                    .elements()
                    .find(|w| {
                        w.toplevel()
                            .map(|t| t.wl_surface().id() == win_id)
                            .unwrap_or(false)
                    })
                    .cloned();
                if let Some(win) = target {
                    if let Some(t) = win.toplevel() {
                        t.send_close();
                    }
                }
            }
            OverlayClick::OverviewDismiss => {
                state.overlays.overview_open = false;
            }
            OverlayClick::WinCtxMinimize(win_id) => {
                state.overlays.window_context_menu = None;
                if let Some(win) = find_window_by_id(state, &win_id) {
                    state.toggle_minimize_window(&win);
                }
            }
            OverlayClick::WinCtxMaximize(win_id) => {
                state.overlays.window_context_menu = None;
                if let Some(win) = find_window_by_id(state, &win_id) {
                    state.toggle_maximize_window(&win);
                }
            }
            OverlayClick::WinCtxPin(win_id) => {
                state.overlays.window_context_menu = None;
                if let Some(win) = find_window_by_id(state, &win_id) {
                    if !state.workspaces.always_on_top.remove(&win_id) {
                        state.workspaces.always_on_top.insert(win_id);
                        state.space.raise_element(&win, true);
                    }
                }
            }
            OverlayClick::WinCtxCenter(win_id) => {
                state.overlays.window_context_menu = None;
                if let Some(win) = find_window_by_id(state, &win_id) {
                    state.focus_window(&win);
                    state.center_focused_window();
                }
            }
            OverlayClick::WinCtxNextWs(win_id) => {
                state.overlays.window_context_menu = None;
                let cur = state.workspaces.window_workspace(&win_id);
                let next_ws = if cur >= state.workspaces.count { 1 } else { cur + 1 };
                state.workspaces.assign_window(win_id, next_ws);
                state.retile_workspace();
                state.refocus_topmost_visible();
                state
                    .overlays
                    .show_osd("Moved Window", &format!("Workspace {}", next_ws), None);
            }
            OverlayClick::WinCtxClose(win_id) => {
                state.overlays.window_context_menu = None;
                if let Some(win) = find_window_by_id(state, &win_id) {
                    if let Some(t) = win.toplevel() {
                        t.send_close();
                    }
                }
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
            OverlayClick::IconCtxRename(idx) => {
                state.overlays.icon_context_menu = None;
                state.overlays.begin_renaming_icon(idx);
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
                    let (kind_label, path, size_label) = match &icon.action {
                        DesktopIconAction::OpenPath(p) => {
                            let meta = fs::metadata(p).ok();
                            let is_dir = p.is_dir();
                            let k = if is_dir {
                                "Folder (Directory)".to_string()
                            } else if p.extension().and_then(|e| e.to_str()) == Some("desktop") {
                                "Desktop Application Entry".to_string()
                            } else {
                                "File".to_string()
                            };
                            let sz = if is_dir {
                                let count = fs::read_dir(p).map(|rd| rd.count()).unwrap_or(0);
                                format!("{} items inside", count)
                            } else if let Some(m) = meta {
                                let b = m.len();
                                if b >= 1024 * 1024 {
                                    format!("{:.2} MiB ({} bytes)", b as f64 / (1024.0 * 1024.0), b)
                                } else if b >= 1024 {
                                    format!("{:.1} KiB ({} bytes)", b as f64 / 1024.0, b)
                                } else {
                                    format!("{} bytes", b)
                                }
                            } else {
                                "Unknown".to_string()
                            };
                            (k, p.to_string_lossy().into_owned(), sz)
                        }
                        _ => (
                            "Built-in System Shortcut".to_string(),
                            "OKay DE Virtual Desktop".to_string(),
                            "Built-in".to_string(),
                        ),
                    };
                    state.overlays.properties_modal = Some(PropertiesInfo {
                        name: icon.label,
                        kind_label,
                        path,
                        size_label,
                    });
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

    if state.overlays.context_menu_pos.is_some()
        || state.overlays.icon_context_menu.is_some()
        || state.overlays.window_context_menu.is_some()
        || state.overlays.properties_modal.is_some()
    {
        state.overlays.context_menu_pos = None;
        state.overlays.icon_context_menu = None;
        state.overlays.window_context_menu = None;
        state.overlays.properties_modal = None;
        return true;
    }
    if state.overlays.overview_open {
        state.overlays.overview_open = false;
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

    // 0. Area Screenshot Drag-to-Select Overlay (`Super + Shift + S`)
    if state.overlays.screenshot_selecting {
        let w = output_geo.size.w.max(320);
        let h = output_geo.size.h.max(240);
        let mut canvas = Canvas::new(w, h);
        canvas.fill_rect(0, 0, w, h, [10, 12, 20, 130]);

        if let (Some(s), Some(c)) = (
            state.overlays.screenshot_start,
            state.overlays.screenshot_current,
        ) {
            let sx = (s.x.round() as i32 - output_geo.loc.x).clamp(0, w);
            let sy = (s.y.round() as i32 - output_geo.loc.y).clamp(0, h);
            let cx = (c.x.round() as i32 - output_geo.loc.x).clamp(0, w);
            let cy = (c.y.round() as i32 - output_geo.loc.y).clamp(0, h);
            let rx = sx.min(cx);
            let ry = sy.min(cy);
            let rw = (sx - cx).abs().max(2);
            let rh = (sy - cy).abs().max(2);

            // Cut out darkened mask inside selection rectangle
            canvas.fill_rect(rx, ry, rw, rh, [0, 0, 0, 0]);
            canvas.stroke_rect(rx, ry, rw, rh, [accent[0], accent[1], accent[2], 245]);

            let dim_txt = format!("{} x {} px", rw, rh);
            let dw = canvas.measure_text_px(&dim_txt, 12.0) + 18;
            let dx = (rx + rw - dw).clamp(8, (w - dw - 8).max(8));
            let dy = if ry >= 34 { ry - 30 } else { ry + rh + 6 };
            canvas.fill_rounded_rect_aa(dx, dy, dw, 24, 7, [20, 22, 34, 235]);
            canvas.draw_text_px(dx + 9, dy + 5, &dim_txt, 12.0, [244, 246, 255, 255]);
        } else {
            let hint = "Drag to capture screen region  •  Esc to cancel";
            let hw = canvas.measure_text_px(hint, 13.0) + 32;
            let hx = (w - hw) / 2;
            canvas.fill_glass_panel(hx, 44, hw, 36, 10);
            canvas.draw_text_px(hx + 16, 54, hint, 13.0, [244, 246, 255, 255]);
        }

        if let Some(elem) = upload_canvas(&canvas, (0, 0), renderer, scale) {
            elements.push(elem);
        }
        return elements;
    }

    // 1. Full-screen Lockscreen (`Super + L`)
    if state.overlays.locked {
        let w = output_geo.size.w.max(320);
        let h = output_geo.size.h.max(240);
        let mut canvas = Canvas::new(w, h);
        canvas.fill_rect(0, 0, w, h, [10, 12, 20, 242]);

        let (time_str, date_str) = current_lock_clock();
        let tw = canvas.measure_text_px(&time_str, 52.0);
        canvas.draw_text_px((w - tw) / 2, (h / 2 - 185).max(32), &time_str, 52.0, [246, 248, 255, 255]);
        let dw = canvas.measure_text_px(&date_str, 16.0);
        canvas.draw_text_px((w - dw) / 2, (h / 2 - 120).max(96), &date_str, 16.0, [186, 194, 220, 235]);

        let card_w = 390;
        let card_h = 196;
        let cx = (w - card_w) / 2;
        let cy = (h - card_h) / 2 + 24;
        canvas.fill_glass_panel(cx, cy, card_w, card_h, 16);

        let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
        let user_lbl = format!("Session Locked  •  {}", user);
        let uw = canvas.measure_text_px(&user_lbl, 14.5);
        canvas.draw_text_px(cx + (card_w - uw) / 2, cy + 24, &user_lbl, 14.5, [246, 248, 255, 255]);

        let prompt = if state.overlays.lock_error {
            "Incorrect PIN — try again"
        } else if state.overlays.lock_pin.is_empty() {
            "Press Enter to unlock session"
        } else {
            "Enter PIN and press Enter to unlock"
        };
        let prompt_col = if state.overlays.lock_error {
            [248, 118, 130, 255]
        } else {
            [176, 184, 210, 240]
        };
        let pw = canvas.measure_text_px(prompt, 12.5);
        canvas.draw_text_px(cx + (card_w - pw) / 2, cy + 56, prompt, 12.5, prompt_col);

        canvas.fill_rounded_rect_aa(cx + 42, cy + 96, card_w - 84, 38, 10, [14, 16, 26, 245]);
        let border_col = if state.overlays.lock_error {
            [240, 88, 102, 235]
        } else {
            [accent[0], accent[1], accent[2], 190]
        };
        canvas.stroke_rect(cx + 42, cy + 96, card_w - 84, 38, border_col);

        if state.overlays.lock_input.is_empty() {
            let ph = "Password / PIN...";
            canvas.draw_text_px(cx + 58, cy + 108, ph, 12.5, [120, 126, 150, 210]);
        } else {
            let bullets: String = std::iter::repeat('*')
                .take(state.overlays.lock_input.chars().count().min(20))
                .collect();
            canvas.draw_text_px(cx + 58, cy + 105, &bullets, 17.0, [246, 248, 255, 255]);
        }

        if let Some(elem) = upload_canvas(&canvas, (0, 0), renderer, scale) {
            elements.push(elem);
        }
        return elements;
    }

    if !is_primary {
        return elements;
    }

    // 1b. Live Snap Preview Overlay when dragging window near screen edges/corners
    if let Some(snap_rect) = state.overlays.snap_preview {
        let lx = snap_rect.loc.x - output_geo.loc.x;
        let ly = snap_rect.loc.y - output_geo.loc.y - crate::state::TITLEBAR_HEIGHT;
        let lw = snap_rect.size.w.max(60);
        let lh = (snap_rect.size.h + crate::state::TITLEBAR_HEIGHT).max(60);
        let mut canvas = Canvas::new(lw, lh);
        canvas.fill_rounded_rect_aa(0, 0, lw, lh, 12, [accent[0], accent[1], accent[2], 52]);
        canvas.stroke_rect(1, 1, lw - 2, lh - 2, [accent[0], accent[1], accent[2], 220]);
        if let Some(elem) = upload_canvas(&canvas, (lx, ly), renderer, scale) {
            elements.push(elem);
        }
    }

    // 1c. Window Overview / Expose (`Super + O`)
    if state.overlays.overview_open {
        let w = output_geo.size.w.max(320);
        let h = output_geo.size.h.max(240);
        let ptr_x = state.pointer_location.x.round() as i32;
        let ptr_y = state.pointer_location.y.round() as i32;

        let mut canvas = Canvas::new(w, h);
        canvas.fill_rect(0, 0, w, h, [12, 14, 22, 185]);

        let wins: Vec<(ObjectId, String, usize, bool, bool)> = state
            .space
            .elements()
            .filter_map(|win| {
                let id = win.toplevel()?.wl_surface().id();
                let ws = state.workspaces.window_workspace(&id);
                let is_min = state.workspaces.is_minimized(win);
                let is_focused = !is_min && state.focused_window().as_ref() == Some(win);
                Some((id, window_title_str(win), ws, is_focused, is_min))
            })
            .collect();

        let header = format!("Window Overview  ({} open)  •  Click to focus or Esc to close", wins.len());
        let hw = canvas.measure_text_px(&header, 15.0);
        canvas.draw_text_px((w - hw) / 2, PANEL_HEIGHT + 24, &header, 15.0, [244, 246, 255, 255]);

        if wins.is_empty() {
            let empty_msg = "No windows currently open";
            let ew = canvas.measure_text_px(empty_msg, 14.0);
            canvas.draw_text_px((w - ew) / 2, h / 2, empty_msg, 14.0, [172, 180, 206, 235]);
        } else {
            let cols = (wins.len() as i32).clamp(1, 3);
            let card_w = 260;
            let card_h = 152;
            let gap = 18;
            let rows = (wins.len() as i32 + cols - 1) / cols;
            let total_w = cols * card_w + (cols - 1) * gap;
            let total_h = rows * card_h + (rows - 1) * gap;
            let x0 = (w - total_w) / 2;
            let y0 = ((h - total_h) / 2).max(PANEL_HEIGHT + 64);

            for (i, (wid, title, ws, is_focused, is_min)) in wins.into_iter().enumerate() {
                let c = i as i32 % cols;
                let r = i as i32 / cols;
                let cx = x0 + c * (card_w + gap);
                let cy = y0 + r * (card_h + gap);

                let card_rect = Rectangle::new(
                    (output_geo.loc.x + cx, output_geo.loc.y + cy).into(),
                    (card_w, card_h).into(),
                );
                let close_rect = Rectangle::new(
                    (output_geo.loc.x + cx + card_w - 30, output_geo.loc.y + cy + 10).into(),
                    (20, 20).into(),
                );

                let hovered = rect_contains(card_rect, ptr_x, ptr_y);
                let card_bg = if *(&is_focused) {
                    [accent[0], accent[1], accent[2], 62]
                } else if hovered {
                    [255, 255, 255, 34]
                } else {
                    [24, 26, 38, 232]
                };
                canvas.fill_glass_panel(cx, cy, card_w, card_h, 14);
                canvas.fill_rounded_rect_aa(cx + 2, cy + 2, card_w - 4, card_h - 4, 12, card_bg);
                if is_focused || hovered {
                    canvas.stroke_rect(cx, cy, card_w, card_h, [accent[0], accent[1], accent[2], 210]);
                }

                let kind = crate::icons::VectorIconKind::from_hint(&title, &title, false);
                crate::icons::draw_app_icon(&mut canvas, cx + 16, cy + 16, 40, Some(&title), kind);

                let ws_badge = if is_min {
                    format!("WS {} • Min", ws)
                } else {
                    format!("WS {}", ws)
                };
                let bw = canvas.measure_text_px(&ws_badge, 11.0) + 12;
                canvas.fill_rounded_rect_aa(cx + 68, cy + 20, bw, 20, 6, [255, 255, 255, 20]);
                canvas.draw_text_px(cx + 74, cy + 24, &ws_badge, 11.0, [196, 204, 228, 245]);

                let close_hovered = rect_contains(close_rect, ptr_x, ptr_y);
                let cbg = if close_hovered {
                    [230, 74, 92, 235]
                } else {
                    [255, 255, 255, 22]
                };
                canvas.fill_circle_aa((cx + card_w - 20) as f32, (cy + 20) as f32, 10.0, cbg);
                canvas.draw_text_px(cx + card_w - 23, cy + 13, "x", 11.5, [246, 248, 255, 255]);

                let short_t: String = title.chars().take(24).collect();
                canvas.draw_text_px(cx + 16, cy + 78, &short_t, 13.5, [244, 246, 255, 255]);
                let sub = if is_focused {
                    "Currently Focused"
                } else if is_min {
                    "Minimized in Dock"
                } else {
                    "Click to Switch"
                };
                canvas.draw_text_px(cx + 16, cy + 104, sub, 11.5, [164, 172, 198, 230]);

                state
                    .overlays
                    .hit_regions
                    .push((card_rect, OverlayClick::OverviewFocus(wid.clone())));
                state
                    .overlays
                    .hit_regions
                    .push((close_rect, OverlayClick::OverviewClose(wid)));
            }
        }

        state.overlays.hit_regions.insert(
            0,
            (output_geo, OverlayClick::OverviewDismiss),
        );

        if let Some(elem) = upload_canvas(&canvas, (0, 0), renderer, scale) {
            elements.push(elem);
        }
    }

    // 2. Right-Click Desktop Context Menu (Polished Dark Glass)
    if let Some(menu_pos) = state.overlays.context_menu_pos {
        let mw = 264;
        let mh = 232;
        let mx = (menu_pos.x - output_geo.loc.x).clamp(8, (output_geo.size.w - mw - 8).max(8));
        let my = (menu_pos.y - output_geo.loc.y).clamp(8, (output_geo.size.h - mh - 8).max(8));

        let mut canvas = Canvas::new(mw, mh);
        canvas.fill_glass_panel(0, 0, mw, mh, 16);

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
        let mh = 194;
        let mx = (menu_pos.x - output_geo.loc.x).clamp(8, (output_geo.size.w - mw - 8).max(8));
        let my = (menu_pos.y - output_geo.loc.y).clamp(8, (output_geo.size.h - mh - 8).max(8));

        let mut canvas = Canvas::new(mw, mh);
        canvas.fill_glass_panel(0, 0, mw, mh, 16);

        let items: [(&str, &str, OverlayClick, bool, bool); 5] = [
            ("Open", "DblClick", OverlayClick::IconCtxOpen(icon_idx), false, false),
            ("Rename", "F2", OverlayClick::IconCtxRename(icon_idx), false, false),
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
                canvas.fill_rounded_rect_aa(8, iy, mw - 16, 30, 7, hcol);
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
                canvas.fill_rect_blend(14, iy + 3, mw - 28, 1, [255, 255, 255, 24]);
                iy += 8;
            }
        }

        if let Some(elem) = upload_canvas(&canvas, (mx, my), renderer, scale) {
            elements.push(elem);
        }
    }

    // 2d. Window Titlebar Right-Click Action Menu
    if let Some((menu_pos, ref win_id)) = state.overlays.window_context_menu.clone() {
        let mw = 236;
        let mh = 224;
        let mx = (menu_pos.x - output_geo.loc.x).clamp(8, (output_geo.size.w - mw - 8).max(8));
        let my = (menu_pos.y - output_geo.loc.y).clamp(PANEL_HEIGHT + 4, (output_geo.size.h - mh - 8).max(8));

        let mut canvas = Canvas::new(mw, mh);
        canvas.fill_glass_panel(0, 0, mw, mh, 16);

        let is_pinned = state.workspaces.always_on_top.contains(win_id);
        let is_max = state.workspaces.maximized.contains(win_id);
        let items: [(&str, OverlayClick, bool, bool); 6] = [
            ("Minimize", OverlayClick::WinCtxMinimize(win_id.clone()), false, false),
            (
                if is_max { "Restore Size" } else { "Maximize" },
                OverlayClick::WinCtxMaximize(win_id.clone()),
                false,
                false,
            ),
            (
                if is_pinned { "Unpin Always on Top" } else { "Always on Top" },
                OverlayClick::WinCtxPin(win_id.clone()),
                false,
                true,
            ),
            ("Center on Screen", OverlayClick::WinCtxCenter(win_id.clone()), false, false),
            ("Move to Next Workspace", OverlayClick::WinCtxNextWs(win_id.clone()), false, true),
            ("Close Window", OverlayClick::WinCtxClose(win_id.clone()), true, false),
        ];

        let mut iy = 10;
        for (label, act, is_danger, sep_after) in items {
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
                canvas.fill_rounded_rect_aa(8, iy, mw - 16, 30, 7, hcol);
            }
            let fg = if hovered {
                if is_danger { [255, 255, 255, 255] } else { contrast_fg(accent) }
            } else if is_danger {
                [248, 128, 138, 255]
            } else {
                [238, 240, 252, 255]
            };
            canvas.draw_text_px(18, iy + 7, label, 12.5, fg);
            state.overlays.hit_regions.push((row_rect, act));
            iy += 32;
            if sep_after {
                canvas.fill_rect_blend(14, iy + 3, mw - 28, 1, [255, 255, 255, 24]);
                iy += 8;
            }
        }

        if let Some(elem) = upload_canvas(&canvas, (mx, my), renderer, scale) {
            elements.push(elem);
        }
    }

    // 2e. File / Folder Properties Modal Card
    if let Some(ref props) = state.overlays.properties_modal {
        let pw = 380;
        let ph = 226;
        let px = (output_geo.size.w - pw) / 2;
        let py = (output_geo.size.h - ph) / 2;
        let ptr_x = state.pointer_location.x.round() as i32;
        let ptr_y = state.pointer_location.y.round() as i32;

        let mut canvas = Canvas::new(pw, ph);
        canvas.fill_glass_panel(0, 0, pw, ph, 18);
        canvas.draw_text_px(18, 14, "Properties", 15.0, [246, 248, 255, 255]);

        let close_rect = Rectangle::new(
            (output_geo.loc.x + px + pw - 74, output_geo.loc.y + py + 12).into(),
            (58, 24).into(),
        );
        let cbg = if rect_contains(close_rect, ptr_x, ptr_y) {
            [accent[0], accent[1], accent[2], 74]
        } else {
            [255, 255, 255, 20]
        };
        canvas.fill_rounded_rect_aa(pw - 74, 12, 58, 24, 7, cbg);
        canvas.draw_text_px(pw - 60, 16, "Close", 11.5, [240, 242, 252, 255]);
        state
            .overlays
            .hit_regions
            .push((close_rect, OverlayClick::CloseProperties));

        let rows = [
            ("Name", props.name.as_str()),
            ("Type", props.kind_label.as_str()),
            ("Size", props.size_label.as_str()),
            ("Location", props.path.as_str()),
        ];
        for (i, (k, v)) in rows.into_iter().enumerate() {
            let ry = 52 + i as i32 * 40;
            canvas.fill_rounded_rect_aa(14, ry, pw - 28, 34, 8, [255, 255, 255, 14]);
            canvas.draw_text_px(24, ry + 10, k, 12.0, [164, 172, 198, 245]);
            let val: String = v.chars().take(34).collect();
            canvas.draw_text_px(108, ry + 10, &val, 12.0, [242, 244, 255, 255]);
        }

        if let Some(elem) = upload_canvas(&canvas, (px, py), renderer, scale) {
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

    // 3. Alt+Tab Window Switcher Overlay (with app icons & workspace badges)
    if state.overlays.alttab_visible_until.is_some() {
        let mut wins: Vec<(String, usize, bool, bool)> = state
            .space
            .elements()
            .filter(|w| state.workspaces.is_on_active_workspace(w))
            .map(|w| {
                let is_min = state.workspaces.is_minimized(w);
                let is_focused = !is_min && state.focused_window().as_ref() == Some(w);
                let ws = w
                    .toplevel()
                    .map(|t| state.workspaces.window_workspace(&t.wl_surface().id()))
                    .unwrap_or(state.workspaces.active);
                (window_title_str(w), ws, is_focused, is_min)
            })
            .collect();
        wins.reverse();

        if !wins.is_empty() {
            let sw = 420;
            let sh = 54 + wins.len() as i32 * 40;
            let sx = (output_geo.size.w - sw) / 2;
            let sy = (output_geo.size.h - sh) / 2;
            let mut canvas = Canvas::new(sw, sh);
            canvas.fill_glass_panel(0, 0, sw, sh, 14);
            canvas.draw_text_px(
                16,
                14,
                "Window Switcher (Alt+Tab)",
                14.0,
                [244, 246, 254, 255],
            );

            for (i, (title, ws, focused, is_min)) in wins.iter().enumerate() {
                let ry = 44 + i as i32 * 38;
                let bg = if *focused {
                    [accent[0], accent[1], accent[2], 230]
                } else {
                    [255, 255, 255, 14]
                };
                let fg = if *focused {
                    contrast_fg(accent)
                } else {
                    [235, 238, 250, 255]
                };
                canvas.fill_rounded_rect_aa(12, ry, sw - 24, 34, 9, bg);
                let kind = crate::icons::VectorIconKind::from_hint(title, title, false);
                crate::icons::draw_app_icon(&mut canvas, 18, ry + 5, 24, Some(title), kind);

                let short: String = title.chars().take(30).collect();
                canvas.draw_text_px(50, ry + 9, &short, 13.0, fg);

                let tag = if *is_min {
                    format!("WS {} • Min", ws)
                } else {
                    format!("WS {}", ws)
                };
                let tw = canvas.measure_text_px(&tag, 11.0);
                canvas.draw_text_px(sw - 22 - tw, ry + 10, &tag, 11.0, fg);
            }

            if let Some(elem) = upload_canvas(&canvas, (sx, sy), renderer, scale) {
                elements.push(elem);
            }
        }
    }

    // 4. Clipboard History Manager (`Super + V`) with Delete & Clear All
    if state.overlays.clipboard_open {
        let cw = 400;
        let ch = 72 + (state.overlays.clipboard_items.len().max(1) as i32 * 36);
        let cx = (output_geo.size.w - cw) / 2;
        let cy = (output_geo.size.h - ch) / 2;
        let ptr_x = state.pointer_location.x.round() as i32;
        let ptr_y = state.pointer_location.y.round() as i32;

        let mut canvas = Canvas::new(cw, ch);
        canvas.fill_glass_panel(0, 0, cw, ch, 18);
        canvas.draw_text_px(
            16,
            14,
            "Clipboard History",
            14.5,
            [244, 246, 254, 255],
        );

        let clear_rect = Rectangle::new(
            (output_geo.loc.x + cx + cw - 82, output_geo.loc.y + cy + 12).into(),
            (68, 22).into(),
        );
        let clear_bg = if rect_contains(clear_rect, ptr_x, ptr_y) {
            [225, 76, 92, 185]
        } else {
            [255, 255, 255, 18]
        };
        canvas.fill_rounded_rect_aa(cw - 82, 12, 68, 22, 6, clear_bg);
        canvas.draw_text_px(cw - 71, 16, "Clear All", 11.0, [242, 244, 255, 255]);
        state
            .overlays
            .hit_regions
            .push((clear_rect, OverlayClick::ClearClipboard));

        if state.overlays.clipboard_items.is_empty() {
            canvas.draw_text_px(18, 56, "Clipboard history is empty", 12.5, [156, 164, 190, 235]);
        } else {
            for (idx, item) in state.overlays.clipboard_items.iter().enumerate() {
                let iy = 48 + idx as i32 * 36;
                let row_rect = Rectangle::new(
                    (output_geo.loc.x + cx + 12, output_geo.loc.y + cy + iy).into(),
                    (cw - 52, 30).into(),
                );
                let del_rect = Rectangle::new(
                    (output_geo.loc.x + cx + cw - 36, output_geo.loc.y + cy + iy + 3).into(),
                    (24, 24).into(),
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
                let short: String = item.chars().take(38).collect();
                canvas.draw_text_px(24, iy + 7, &short, 12.5, [238, 240, 252, 255]);

                let del_hovered = rect_contains(del_rect, ptr_x, ptr_y);
                let dbg = if del_hovered {
                    [225, 76, 92, 220]
                } else {
                    [255, 255, 255, 16]
                };
                canvas.fill_rounded_rect_aa(cw - 36, iy + 3, 24, 24, 6, dbg);
                canvas.draw_text_px(cw - 27, iy + 8, "x", 11.5, [236, 240, 252, 245]);

                state
                    .overlays
                    .hit_regions
                    .push((row_rect, OverlayClick::SelectClipboard(idx)));
                state
                    .overlays
                    .hit_regions
                    .push((del_rect, OverlayClick::DeleteClipboard(idx)));
            }
        }

        if let Some(elem) = upload_canvas(&canvas, (cx, cy), renderer, scale) {
            elements.push(elem);
        }
    }

    // 5. Real Calendar Popup (leap-year accurate weekday layout + month navigation)
    if state.overlays.calendar_open {
        let cal_w = 286;
        let cal_h = 282;
        let cal_x = (output_geo.size.w - cal_w - 12).max(8);
        let cal_y = if state.panel.dock_mode || state.panel.panel_top {
            PANEL_HEIGHT + 8
        } else {
            (output_geo.size.h - 36 - cal_h - 8).max(8)
        };
        let ptr_x = state.pointer_location.x.round() as i32;
        let ptr_y = state.pointer_location.y.round() as i32;

        let mut canvas = Canvas::new(cal_w, cal_h);
        canvas.fill_glass_panel(0, 0, cal_w, cal_h, 18);

        let (date_header, start_mon_0, days_in_month, today_opt) =
            calendar_grid_info(state.overlays.cal_month_offset);
        canvas.draw_text_px(16, 14, &date_header, 14.0, [246, 248, 255, 255]);

        let prev_rect = Rectangle::new(
            (output_geo.loc.x + cal_x + cal_w - 94, output_geo.loc.y + cal_y + 11).into(),
            (24, 22).into(),
        );
        let today_rect = Rectangle::new(
            (output_geo.loc.x + cal_x + cal_w - 66, output_geo.loc.y + cal_y + 11).into(),
            (26, 22).into(),
        );
        let next_rect = Rectangle::new(
            (output_geo.loc.x + cal_x + cal_w - 36, output_geo.loc.y + cal_y + 11).into(),
            (24, 22).into(),
        );

        for (r, x_off, lbl, act) in [
            (prev_rect, cal_w - 94, "<", OverlayClick::CalPrevMonth),
            (today_rect, cal_w - 66, "•", OverlayClick::CalToday),
            (next_rect, cal_w - 36, ">", OverlayClick::CalNextMonth),
        ] {
            let bg = if rect_contains(r, ptr_x, ptr_y) {
                [accent[0], accent[1], accent[2], 78]
            } else {
                [255, 255, 255, 18]
            };
            canvas.fill_rounded_rect_aa(x_off, 11, r.size.w, 22, 6, bg);
            canvas.draw_text_px(x_off + (r.size.w - 6) / 2, 15, lbl, 11.5, [240, 242, 255, 255]);
            state.overlays.hit_regions.push((r, act));
        }

        canvas.fill_rounded_rect_aa(12, 42, cal_w - 24, 24, 6, [255, 255, 255, 12]);
        canvas.draw_text_px(
            20,
            47,
            "Mo   Tu   We   Th   Fr   Sa   Su",
            12.0,
            [168, 174, 200, 245],
        );

        for day in 1..=days_in_month {
            let slot = start_mon_0 + (day - 1);
            let col = slot % 7;
            let row = slot / 7;
            let dx = 18 + col * 36;
            let dy = 74 + row * 31;
            let is_today = today_opt == Some(day);
            if is_today {
                canvas.fill_rounded_rect_aa(dx - 4, dy - 3, 28, 24, 7, [accent[0], accent[1], accent[2], 240]);
                canvas.draw_text_px(dx + 2, dy, &format!("{:2}", day), 12.5, contrast_fg(accent));
            } else {
                let col_fg = if col >= 5 {
                    [172, 180, 208, 225]
                } else {
                    [232, 236, 248, 250]
                };
                canvas.draw_text_px(dx + 2, dy, &format!("{:2}", day), 12.5, col_fg);
            }
        }

        state.overlays.hit_regions.insert(
            0,
            (
                Rectangle::new(
                    (output_geo.loc.x + cal_x, output_geo.loc.y + cal_y).into(),
                    (cal_w, cal_h).into(),
                ),
                OverlayClick::CloseCalendar,
            ),
        );

        let anim_t = state.animations.calendar_open_anim;
        if state.animations.enabled && anim_t < 0.995 {
            if let Some(elem) = crate::animation::render_topbar_genie_canvas(
                &canvas.pixels,
                cal_w,
                cal_h,
                Point::from((cal_x, cal_y)),
                cal_w - 64,
                anim_t,
                renderer,
                scale,
            ) {
                elements.push(elem);
            }
        } else if let Some(elem) = upload_canvas(&canvas, (cal_x, cal_y), renderer, scale) {
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
        canvas.fill_glass_panel(0, 0, nw, nh, 18);
        canvas.draw_text_px(
            16,
            14,
            "Notification Center",
            14.5,
            [246, 248, 255, 255],
        );

        let clear_rect = Rectangle::new(
            (output_geo.loc.x + nx + nw - 90, output_geo.loc.y + ny + 11).into(),
            (76, 24).into(),
        );
        let clear_bg = if rect_contains(clear_rect, ptr_x, ptr_y) {
            [accent[0], accent[1], accent[2], 78]
        } else {
            [255, 255, 255, 22]
        };
        canvas.fill_rounded_rect_aa(nw - 90, 11, 76, 24, 8, clear_bg);
        canvas.draw_text_px(nw - 79, 15, "Clear All", 12.0, [238, 240, 252, 255]);
        state
            .overlays
            .hit_regions
            .push((clear_rect, OverlayClick::ClearNotifications));

        if state.overlays.history.is_empty() {
            canvas.draw_text_px(16, 62, "No recent notifications.", 12.5, [156, 162, 188, 235]);
        } else {
            for (i, (title, body)) in state.overlays.history.iter().take(4).enumerate() {
                let iy = 48 + i as i32 * 60;
                canvas.fill_rounded_rect_aa(12, iy, nw - 24, 52, 12, [255, 255, 255, 14]);
                canvas.fill_rounded_rect_aa(14, iy + 8, 3, 36, 2, [accent[0], accent[1], accent[2], 235]);
                let st: String = title.chars().take(33).collect();
                let sb: String = body.chars().take(37).collect();
                canvas.draw_text_px(24, iy + 8, &st, 12.5, [242, 244, 255, 255]);
                canvas.draw_text_px(24, iy + 28, &sb, 11.5, [178, 184, 208, 240]);
            }
        }

        let anim_t = state.animations.notif_open_anim;
        if state.animations.enabled && anim_t < 0.995 {
            if let Some(elem) = crate::animation::render_topbar_genie_canvas(
                &canvas.pixels,
                nw,
                nh,
                Point::from((nx, ny)),
                nw - 110,
                anim_t,
                renderer,
                scale,
            ) {
                elements.push(elem);
            }
        } else if let Some(elem) = upload_canvas(&canvas, (nx, ny), renderer, scale) {
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
        canvas.fill_glass_panel(0, 0, cc_w, cc_h, 18);

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
            match i {
                0 => crate::icons::draw_wifi_glyph(&mut canvas, 34.0, (ry + 21) as f32, 0.9, pill_fg),
                1 => crate::icons::draw_bluetooth_glyph(&mut canvas, 34.0, (ry + 16) as f32, 0.95, pill_fg),
                _ => crate::icons::draw_tiling_glyph(&mut canvas, 34.0, (ry + 16) as f32, 0.95, pill_fg),
            }
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
        state.overlays.cc_volume_track = Some(Rectangle::new(
            (output_geo.loc.x + cc_x + track_x, output_geo.loc.y + cc_y + track_y - 8).into(),
            (track_w, 26).into(),
        ));

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

        let anim_t = state.animations.control_center_open_anim;
        if state.animations.enabled && anim_t < 0.995 {
            if let Some(elem) = crate::animation::render_topbar_genie_canvas(
                &canvas.pixels,
                cc_w,
                cc_h,
                Point::from((cc_x, cc_y)),
                cc_w - 115,
                anim_t,
                renderer,
                scale,
            ) {
                elements.push(elem);
            }
        } else if let Some(elem) = upload_canvas(&canvas, (cc_x, cc_y), renderer, scale) {
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

    // 8. Centered On-Screen Display (OSD) Pill above Dock
    if let Some(ref osd) = state.overlays.osd {
        let ow = 240;
        let oh = if osd.progress.is_some() { 58 } else { 44 };
        let ox = (output_geo.size.w - ow) / 2;
        let oy = (output_geo.size.h - 140).max(PANEL_HEIGHT + 24);
        let mut canvas = Canvas::new(ow, oh);
        canvas.fill_glass_panel(0, 0, ow, oh, 14);
        canvas.draw_text_px(16, 12, &osd.title, 12.5, [176, 184, 212, 245]);
        let vw = canvas.measure_text_px(&osd.value, 13.0);
        canvas.draw_text_px(ow - 16 - vw, 12, &osd.value, 13.0, [246, 248, 255, 255]);
        if let Some(pct) = osd.progress {
            let tw = ow - 32;
            canvas.fill_rounded_rect_aa(16, 38, tw, 8, 4, [255, 255, 255, 28]);
            let fw = ((tw * pct) / 100).max(6);
            canvas.fill_rounded_rect_aa(16, 38, fw, 8, 4, [accent[0], accent[1], accent[2], 240]);
        }
        if let Some(elem) = upload_canvas(&canvas, (ox, oy), renderer, scale) {
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
        let is_renaming = state
            .overlays
            .renaming_icon
            .as_ref()
            .map(|(ri, _)| *ri == idx)
            .unwrap_or(false);

        if let Some((_, ref edit_name)) = state.overlays.renaming_icon.as_ref().filter(|(ri, _)| *ri == idx) {
            let accent = state.panel.accent_rgb();
            canvas.fill_rounded_rect_aa(4, 57, iw - 8, 22, 6, [18, 20, 32, 242]);
            canvas.stroke_rect(4, 57, iw - 8, 22, [accent[0], accent[1], accent[2], 235]);
            let tail: String = if edit_name.chars().count() > 10 {
                edit_name.chars().rev().take(10).collect::<String>().chars().rev().collect()
            } else {
                edit_name.clone()
            };
            let with_caret = format!("{}|", tail);
            let tw = canvas.measure_text_px(&with_caret, 11.5);
            canvas.draw_text_px(((iw - tw) / 2).max(6), 61, &with_caret, 11.5, [246, 248, 255, 255]);
        } else {
            let lw = canvas.measure_text_px(&short, 11.5);
            if is_selected {
                let pill_w = (lw + 12).min(iw - 6);
                let pill_x = ((iw - pill_w) / 2).max(3);
                canvas.fill_rounded_rect_aa(pill_x, 58, pill_w, 19, 6, [52, 120, 246, 225]);
            }
            canvas.draw_text_shadowed(((iw - lw) / 2).max(4), 60, &short, 11.5, [246, 248, 255, 255]);
        }
        let _ = is_renaming;

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

fn find_window_by_id(state: &OkayState, id: &ObjectId) -> Option<smithay::desktop::Window> {
    state
        .space
        .elements()
        .find(|w| w.toplevel().map(|t| t.wl_surface().id() == *id).unwrap_or(false))
        .cloned()
}

fn current_lock_clock() -> (String, String) {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        const DAYS: [&str; 7] = [
            "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday",
        ];
        const MONTHS: [&str; 12] = [
            "January", "February", "March", "April", "May", "June",
            "July", "August", "September", "October", "November", "December",
        ];
        let d = DAYS[(tm.tm_wday as usize).min(6)];
        let m = MONTHS[(tm.tm_mon as usize).min(11)];
        (
            format!("{:02}:{:02}", tm.tm_hour, tm.tm_min),
            format!("{}, {} {} {}", d, tm.tm_mday, m, 1900 + tm.tm_year),
        )
    }
}

fn calendar_grid_info(month_offset: i32) -> (String, i32, i32, Option<i32>) {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        let cur_year = 1900 + tm.tm_year;
        let cur_mon0 = tm.tm_mon;
        let cur_day = tm.tm_mday;

        let total_months = cur_year * 12 + cur_mon0 + month_offset;
        let year = total_months.div_euclid(12);
        let mon0 = total_months.rem_euclid(12);

        let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let days_in_month = match mon0 {
            0 | 2 | 4 | 6 | 7 | 9 | 11 => 31,
            3 | 5 | 8 | 10 => 30,
            1 => if is_leap { 29 } else { 28 },
            _ => 30,
        };

        // Sakamoto's weekday algorithm (0 = Sun .. 6 = Sat -> convert to 0 = Mon .. 6 = Sun)
        let y = if mon0 + 1 < 3 { year - 1 } else { year };
        const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let sun0 = (y + y / 4 - y / 100 + y / 400 + T[mon0 as usize] + 1).rem_euclid(7);
        let mon_start = (sun0 + 6) % 7;

        const MONTHS: [&str; 12] = [
            "January", "February", "March", "April", "May", "June",
            "July", "August", "September", "October", "November", "December",
        ];
        let m_name = MONTHS[(mon0 as usize).min(11)];
        let today = if month_offset == 0 { Some(cur_day) } else { None };
        (format!("{} {}", m_name, year), mon_start, days_in_month, today)
    }
}
