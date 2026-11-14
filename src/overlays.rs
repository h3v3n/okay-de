use std::{
    fs::{self, File},
    io::BufWriter,
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
    panel::{rect_contains, window_title_str, Canvas, PANEL_HEIGHT},
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
    pub badge: String,
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

#[derive(Debug)]
pub struct OverlaysState {
    pub notifications: Vec<NotificationToast>,
    pub history: Vec<(String, String)>,
    pub notif_drawer_open: bool,
    pub calendar_open: bool,
    pub clipboard_open: bool,
    pub clipboard_items: Vec<String>,
    pub alttab_visible_until: Option<Instant>,
    pub locked: bool,
    pub lock_input: String,
    pub context_menu_pos: Option<Point<i32, Logical>>,
    pub show_desktop_icons: bool,
    pub desktop_icons: Vec<DesktopIcon>,
    pub dragging_icon: Option<usize>,
    hit_regions: Vec<(Rectangle<i32, Logical>, OverlayClick)>,
}

#[derive(Debug, Clone)]
enum OverlayClick {
    DismissToast(usize),
    ClearNotifications,
    CloseNotifDrawer,
    SelectClipboard(usize),
    CloseCalendar,
    DesktopIconClick(usize),
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
            clipboard_items: vec![
                "Welcome to OKay DE (Super+V Clipboard)".to_string(),
                "~/.config/okay-de/config.toml".to_string(),
            ],
            alttab_visible_until: None,
            locked: false,
            lock_input: String::new(),
            context_menu_pos: None,
            show_desktop_icons: true,
            desktop_icons: Vec::new(),
            dragging_icon: None,
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
                badge: "HM".to_string(),
                grid_col: 0,
                grid_row: 0,
                action: DesktopIconAction::OpenHome,
            },
            DesktopIcon {
                label: "Terminal".to_string(),
                badge: ">_".to_string(),
                grid_col: 0,
                grid_row: 1,
                action: DesktopIconAction::OpenTerminal,
            },
            DesktopIcon {
                label: "Settings".to_string(),
                badge: "OS".to_string(),
                grid_col: 0,
                grid_row: 2,
                action: DesktopIconAction::OpenSettings,
            },
            DesktopIcon {
                label: "Screenshot".to_string(),
                badge: "SC".to_string(),
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
                for entry in entries.flatten().take(8) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with('.') {
                        continue;
                    }
                    icons.push(DesktopIcon {
                        label: name,
                        badge: "FL".to_string(),
                        grid_col: col,
                        grid_row: row,
                        action: DesktopIconAction::OpenPath(entry.path()),
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

    pub fn poll_clipboard_now(&mut self) {
        if let Ok(out) = Command::new("wl-paste").arg("--no-newline").output() {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !text.is_empty() && self.clipboard_items.first() != Some(&text) {
                    self.clipboard_items.retain(|x| x != &text);
                    self.clipboard_items.insert(0, text);
                    if self.clipboard_items.len() > 10 {
                        self.clipboard_items.truncate(10);
                    }
                }
            }
        }
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

    // Try grim first if installed on host/session; otherwise write a crisp compositor snapshot PNG
    let grim_ok = Command::new("grim")
        .env("WAYLAND_DISPLAY", &state.socket_name)
        .arg(&path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !grim_ok {
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

        if let Ok(file) = File::create(&path) {
            let w = BufWriter::new(file);
            let mut encoder = png::Encoder::new(w, 640, 360);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            if let Ok(mut writer) = encoder.write_header() {
                let _ = writer.write_image_data(&canvas.pixels);
            }
        }
    }

    state.overlays.push_notification(
        "Screenshot Saved",
        format!("Saved to ~/Pictures/Screenshots/{}", filename),
    );
}

/// Handles right-click on the desktop background to open the desktop context menu.
pub fn handle_desktop_right_click(state: &mut OkayState, pos: Point<f64, Logical>) -> bool {
    if state.overlays.locked {
        return true;
    }
    if state.surface_under(pos).is_some() || state.window_under(pos).is_some() {
        return false;
    }
    state.overlays.context_menu_pos = Some(Point::from((pos.x.round() as i32, pos.y.round() as i32)));
    true
}

/// Handles dragging a desktop icon across the desktop grid.
pub fn handle_overlays_motion(state: &mut OkayState, pos: Point<f64, Logical>) -> bool {
    if let Some(idx) = state.overlays.dragging_icon {
        let col = ((pos.x.round() as i32 - 20).max(0) / 96).clamp(0, 14);
        let row = ((pos.y.round() as i32 - 24).max(0) / 96).clamp(0, 8);
        if let Some(icon) = state.overlays.desktop_icons.get_mut(idx) {
            icon.grid_col = col;
            icon.grid_row = row;
        }
        return true;
    }
    false
}

pub fn handle_overlays_release(state: &mut OkayState) -> bool {
    if state.overlays.dragging_icon.take().is_some() {
        return true;
    }
    false
}

/// Handles left-click on any active overlay (context menu, notifications, calendar, clipboard, desktop icons).
pub fn handle_overlays_click(state: &mut OkayState, pos: Point<f64, Logical>) -> bool {
    if state.overlays.locked {
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
                    let _ = Command::new("wl-copy").arg(&text).spawn();
                    state
                        .overlays
                        .push_notification("Clipboard", "Copied selected entry to clipboard");
                }
                state.overlays.clipboard_open = false;
            }
            OverlayClick::CloseCalendar => {
                state.overlays.calendar_open = false;
            }
            OverlayClick::DesktopIconClick(idx) => {
                // Only interact with desktop icons if no window covers this point
                if state.window_under(pos).is_some() {
                    return false;
                }
                state.overlays.dragging_icon = Some(idx);
                if let Some(icon) = state.overlays.desktop_icons.get(idx).cloned() {
                    match icon.action {
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
                            let _ = Command::new("xdg-open")
                                .arg(p)
                                .env("WAYLAND_DISPLAY", &state.socket_name)
                                .spawn();
                        }
                    }
                }
            }
            OverlayClick::ContextNewFolder => {
                state.overlays.context_menu_pos = None;
                if let Ok(home) = std::env::var("HOME") {
                    let folder = PathBuf::from(home).join("Desktop/New_Folder");
                    let _ = fs::create_dir_all(&folder);
                    state.overlays.refresh_desktop_icons();
                    state
                        .overlays
                        .push_notification("Desktop", "Created ~/Desktop/New_Folder");
                }
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

    if state.overlays.context_menu_pos.is_some() {
        state.overlays.context_menu_pos = None;
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

    false
}

/// Renders topmost overlays: Lockscreen, Alt+Tab switcher, Toasts, Notification Drawer,
/// Calendar Popup, Clipboard Manager, and Desktop Right-Click Context Menu.
pub fn render_top_overlays(
    state: &mut OkayState,
    renderer: &mut GlesRenderer,
    output: &Output,
    scale: Scale<f64>,
) -> Vec<OkayRenderElements<GlesRenderer>> {
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
        canvas.fill_rounded_rect(cx, cy, card_w, card_h, 12, [24, 24, 38, 255]);
        canvas.stroke_rect(cx, cy, card_w, card_h, [accent[0], accent[1], accent[2], 220]);

        canvas.draw_text_px(
            cx + 115,
            cy + 24,
            "OKay DE Locked",
            18.0,
            [accent[0], accent[1], accent[2], 255],
        );
        canvas.draw_text_px(
            cx + 52,
            cy + 64,
            "Type password or press Enter to unlock session",
            13.0,
            [200, 205, 228, 255],
        );

        canvas.fill_rounded_rect(cx + 40, cy + 104, card_w - 80, 36, 6, [16, 16, 26, 255]);
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

    // 2. Right-Click Desktop Context Menu
    if let Some(menu_pos) = state.overlays.context_menu_pos {
        let mw = 230;
        let mh = 216;
        let mx = (menu_pos.x - output_geo.loc.x).clamp(8, (output_geo.size.w - mw - 8).max(8));
        let my = (menu_pos.y - output_geo.loc.y).clamp(8, (output_geo.size.h - mh - 8).max(8));

        let mut canvas = Canvas::new(mw, mh);
        canvas.fill_rounded_rect(0, 0, mw, mh, 8, [22, 22, 34, 250]);
        canvas.stroke_rect(0, 0, mw, mh, [accent[0], accent[1], accent[2], 200]);

        let items: [(&str, OverlayClick); 6] = [
            ("New Folder on Desktop", OverlayClick::ContextNewFolder),
            ("Open Terminal Here", OverlayClick::ContextOpenTerminal),
            ("Reload Wallpaper", OverlayClick::ContextReloadWallpaper),
            ("Toggle Desktop Icons", OverlayClick::ContextToggleIcons),
            ("Display Settings", OverlayClick::ContextDisplaySettings),
            ("OKay Settings (Super+I)", OverlayClick::ContextOkaySettings),
        ];

        for (i, (label, act)) in items.into_iter().enumerate() {
            let iy = 8 + i as i32 * 34;
            canvas.fill_rounded_rect(8, iy, mw - 16, 30, 5, [34, 34, 52, 240]);
            canvas.draw_text_px(18, iy + 7, label, 13.0, [238, 240, 252, 255]);
            state.overlays.hit_regions.push((
                Rectangle::new(
                    (output_geo.loc.x + mx + 8, output_geo.loc.y + my + iy).into(),
                    (mw - 16, 30).into(),
                ),
                act,
            ));
        }

        if let Some(elem) = upload_canvas(&canvas, (mx, my), renderer, scale) {
            elements.push(elem);
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
            canvas.fill_rounded_rect(0, 0, sw, sh, 10, [20, 20, 32, 248]);
            canvas.stroke_rect(0, 0, sw, sh, [accent[0], accent[1], accent[2], 220]);
            canvas.draw_text_px(
                16,
                12,
                "Window Switcher (Alt+Tab)",
                14.0,
                [accent[0], accent[1], accent[2], 255],
            );

            for (i, (title, focused)) in wins.iter().enumerate() {
                let ry = 40 + i as i32 * 34;
                let bg = if *focused {
                    [accent[0], accent[1], accent[2], 255]
                } else {
                    [34, 34, 52, 240]
                };
                let fg = if *focused {
                    [16, 16, 24, 255]
                } else {
                    [235, 238, 250, 255]
                };
                canvas.fill_rounded_rect(10, ry, sw - 20, 30, 6, bg);
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
        let cw = 360;
        let ch = 64 + (state.overlays.clipboard_items.len().max(1) as i32 * 36);
        let cx = (output_geo.size.w - cw) / 2;
        let cy = (output_geo.size.h - ch) / 2;

        let mut canvas = Canvas::new(cw, ch);
        canvas.fill_rounded_rect(0, 0, cw, ch, 10, [20, 20, 32, 250]);
        canvas.stroke_rect(0, 0, cw, ch, [accent[0], accent[1], accent[2], 210]);
        canvas.draw_text_px(
            16,
            14,
            "Clipboard History (Super+V)",
            14.5,
            [accent[0], accent[1], accent[2], 255],
        );

        for (idx, item) in state.overlays.clipboard_items.iter().enumerate() {
            let iy = 48 + idx as i32 * 34;
            canvas.fill_rounded_rect(10, iy, cw - 20, 30, 5, [34, 34, 50, 240]);
            let short: String = item.chars().take(40).collect();
            canvas.draw_text_px(18, iy + 7, &short, 13.0, [235, 238, 250, 255]);
            state.overlays.hit_regions.push((
                Rectangle::new(
                    (output_geo.loc.x + cx + 10, output_geo.loc.y + cy + iy).into(),
                    (cw - 20, 30).into(),
                ),
                OverlayClick::SelectClipboard(idx),
            ));
        }

        if let Some(elem) = upload_canvas(&canvas, (cx, cy), renderer, scale) {
            elements.push(elem);
        }
    }

    // 5. Calendar Popup (when clicking taskbar clock)
    if state.overlays.calendar_open {
        let cal_w = 270;
        let cal_h = 235;
        let cal_x = (output_geo.size.w - cal_w - 12).max(8);
        let cal_y = if state.panel.panel_top {
            PANEL_HEIGHT + 8
        } else {
            (output_geo.size.h - PANEL_HEIGHT - cal_h - 8).max(8)
        };

        let mut canvas = Canvas::new(cal_w, cal_h);
        canvas.fill_rounded_rect(0, 0, cal_w, cal_h, 10, [20, 20, 32, 250]);
        canvas.stroke_rect(0, 0, cal_w, cal_h, [accent[0], accent[1], accent[2], 200]);

        let (date_header, cur_day) = current_date_info();
        canvas.draw_text_px(16, 14, &date_header, 14.5, [accent[0], accent[1], accent[2], 255]);
        canvas.draw_text_px(
            16,
            42,
            "Mo   Tu   We   Th   Fr   Sa   Su",
            12.5,
            [165, 170, 195, 255],
        );

        for day in 1..=30 {
            let col = (day - 1) % 7;
            let row = (day - 1) / 7;
            let dx = 14 + col * 35;
            let dy = 68 + row * 30;
            if day == cur_day {
                canvas.fill_rounded_rect(dx - 4, dy - 3, 28, 24, 5, [accent[0], accent[1], accent[2], 255]);
                canvas.draw_text_px(dx + 2, dy, &format!("{:2}", day), 12.5, [16, 16, 24, 255]);
            } else {
                canvas.draw_text_px(dx + 2, dy, &format!("{:2}", day), 12.5, [230, 232, 246, 255]);
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
        let nw = 330;
        let nh = 300;
        let nx = (output_geo.size.w - nw - 12).max(8);
        let ny = if state.panel.panel_top {
            PANEL_HEIGHT + 8
        } else {
            (output_geo.size.h - PANEL_HEIGHT - nh - 8).max(8)
        };

        let mut canvas = Canvas::new(nw, nh);
        canvas.fill_rounded_rect(0, 0, nw, nh, 10, [20, 20, 32, 250]);
        canvas.stroke_rect(0, 0, nw, nh, [accent[0], accent[1], accent[2], 200]);
        canvas.draw_text_px(
            14,
            14,
            "Notification Center",
            14.5,
            [accent[0], accent[1], accent[2], 255],
        );

        canvas.fill_rounded_rect(nw - 78, 10, 66, 24, 5, [48, 48, 72, 255]);
        canvas.draw_text_px(nw - 66, 14, "Clear", 12.0, [235, 238, 250, 255]);
        state.overlays.hit_regions.push((
            Rectangle::new(
                (output_geo.loc.x + nx + nw - 78, output_geo.loc.y + ny + 10).into(),
                (66, 24).into(),
            ),
            OverlayClick::ClearNotifications,
        ));

        if state.overlays.history.is_empty() {
            canvas.draw_text_px(16, 60, "No recent notifications.", 13.0, [160, 165, 190, 255]);
        } else {
            for (i, (title, body)) in state.overlays.history.iter().take(4).enumerate() {
                let iy = 46 + i as i32 * 60;
                canvas.fill_rounded_rect(10, iy, nw - 20, 52, 6, [32, 32, 48, 240]);
                let st: String = title.chars().take(34).collect();
                let sb: String = body.chars().take(38).collect();
                canvas.draw_text_px(18, iy + 8, &st, 13.0, [accent[0], accent[1], accent[2], 255]);
                canvas.draw_text_px(18, iy + 28, &sb, 12.0, [220, 224, 240, 255]);
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

    // 7. Active Toast Notifications in Top-Right Corner
    for (idx, toast) in state.overlays.notifications.iter().take(3).enumerate() {
        let tw = 310;
        let th = 62;
        let tx = (output_geo.size.w - tw - 14).max(8);
        let top_offset = if state.panel.panel_top { PANEL_HEIGHT + 12 } else { 14 };
        let ty = top_offset + idx as i32 * (th + 8);

        let mut canvas = Canvas::new(tw, th);
        canvas.fill_rounded_rect(0, 0, tw, th, 8, [24, 24, 38, 246]);
        canvas.stroke_rect(0, 0, tw, th, [accent[0], accent[1], accent[2], 210]);
        let st: String = toast.title.chars().take(32).collect();
        let sb: String = toast.body.chars().take(38).collect();
        canvas.draw_text_px(14, 10, &st, 13.5, [accent[0], accent[1], accent[2], 255]);
        canvas.draw_text_px(14, 32, &sb, 12.0, [230, 232, 246, 255]);

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
) -> Vec<OkayRenderElements<GlesRenderer>> {
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

    let accent = state.panel.accent_rgb();
    let mut elements = Vec::new();
    let top_margin = if state.panel.panel_top { PANEL_HEIGHT + 16 } else { 20 };

    for (idx, icon) in state.overlays.desktop_icons.iter().enumerate() {
        let ix = 20 + icon.grid_col * 96;
        let iy = top_margin + icon.grid_row * 92;
        let iw = 84;
        let ih = 78;

        let mut canvas = Canvas::new(iw, ih);
        canvas.fill_rounded_rect(18, 4, 48, 44, 9, [32, 32, 48, 230]);
        canvas.stroke_rect(18, 4, 48, 44, [accent[0], accent[1], accent[2], 170]);
        canvas.draw_text_px(
            31,
            16,
            &icon.badge,
            14.5,
            [accent[0], accent[1], accent[2], 255],
        );

        let short: String = icon.label.chars().take(10).collect();
        canvas.draw_text_px(8, 54, &short, 12.0, [238, 240, 252, 255]);

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

    elements
}

fn upload_canvas(
    canvas: &Canvas,
    local_pos: (i32, i32),
    renderer: &mut GlesRenderer,
    scale: Scale<f64>,
) -> Option<OkayRenderElements<GlesRenderer>> {
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
