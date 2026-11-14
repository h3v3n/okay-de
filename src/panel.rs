use std::{ffi::OsStr, process::Command};

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
    reexports::wayland_server::Resource,
    utils::{Logical, Physical, Point, Rectangle, Scale, Transform},
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};

use crate::{render::OkayRenderElements, state::OkayState};

pub const PANEL_HEIGHT: i32 = 40;
const MENU_WIDTH: i32 = 220;
const MENU_ITEM_HEIGHT: i32 = 34;
const MENU_PADDING: i32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherItem {
    Terminal,
    FileManager,
    Browser,
    ReloadConfig,
    ExitSession,
}

impl LauncherItem {
    const ALL: [Self; 5] = [
        Self::Terminal,
        Self::FileManager,
        Self::Browser,
        Self::ReloadConfig,
        Self::ExitSession,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::FileManager => "File Manager",
            Self::Browser => "Browser",
            Self::ReloadConfig => "Reload Config",
            Self::ExitSession => "Exit Session",
        }
    }
}

#[derive(Debug, Clone)]
enum ClickTarget {
    ToggleLauncher,
    ToggleTiling,
    Workspace(usize),
    FocusWindow(usize),
    LaunchItem(LauncherItem),
}

#[derive(Debug, Default)]
pub struct PanelState {
    pub launcher_open: bool,
    hit_regions: Vec<(Rectangle<i32, Logical>, ClickTarget)>,
}

impl PanelState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Spawns the first available terminal emulator installed on the system.
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

fn spawn_first_available(candidates: &[&str], socket_name: &OsStr, kind: &str) {
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

fn execute_launcher_item(state: &mut OkayState, item: LauncherItem) {
    state.panel.launcher_open = false;
    match item {
        LauncherItem::Terminal => spawn_terminal(&state.socket_name),
        LauncherItem::FileManager => spawn_first_available(
            &["pcmanfm", "thunar", "nemo", "dolphin", "nautilus"],
            &state.socket_name,
            "file-manager",
        ),
        LauncherItem::Browser => spawn_first_available(
            &["firefox", "chromium", "brave", "floorp"],
            &state.socket_name,
            "browser",
        ),
        LauncherItem::ReloadConfig => {
            state.desktop.reload();
            state.retile_workspace();
        }
        LauncherItem::ExitSession => {
            tracing::info!("Exit Session selected from taskbar menu");
            state.running = false;
            state.loop_signal.stop();
        }
    }
}

/// Handles a left-click at global logical coordinates. Returns `true` if the panel or
/// launcher menu consumed the click.
pub fn handle_panel_click(state: &mut OkayState, global_pos: Point<f64, Logical>) -> bool {
    let px = global_pos.x.round() as i32;
    let py = global_pos.y.round() as i32;

    // Check popup menu items first, then taskbar buttons
    let mut matched: Option<ClickTarget> = None;
    for (rect, target) in state.panel.hit_regions.iter().rev() {
        if px >= rect.loc.x
            && px < rect.loc.x + rect.size.w
            && py >= rect.loc.y
            && py < rect.loc.y + rect.size.h
        {
            matched = Some(target.clone());
            break;
        }
    }

    if let Some(target) = matched {
        match target {
            ClickTarget::ToggleLauncher => {
                state.panel.launcher_open = !state.panel.launcher_open;
            }
            ClickTarget::ToggleTiling => {
                state.panel.launcher_open = false;
                state.toggle_tiling();
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
        }
        return true;
    }

    // If launcher was open and user clicked outside, close it.
    if state.panel.launcher_open {
        state.panel.launcher_open = false;
    }

    false
}

/// Rebuilds interactive hit-regions and renders the native taskbar (+ launcher popup if open)
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
    if output_geo.size.w <= 0 || output_geo.size.h <= PANEL_HEIGHT {
        return Vec::new();
    }

    // Only clear hit regions when rendering the primary (first) output, or accumulate per output
    let is_primary = state
        .space
        .outputs()
        .next()
        .map(|o| o == output)
        .unwrap_or(false);
    if is_primary {
        state.panel.hit_regions.clear();
    }

    let mut elements = Vec::new();
    let bar_w = output_geo.size.w;
    let bar_h = PANEL_HEIGHT;
    let bar_y_local = output_geo.size.h - bar_h;

    // Collect window info for the active workspace
    let focused_id = state
        .seat
        .get_keyboard()
        .and_then(|k| k.current_focus())
        .map(|s| s.id());

    let ws_windows: Vec<(String, bool, bool)> = state
        .space
        .elements()
        .filter(|w| state.workspaces.is_on_active_workspace(w))
        .map(|w| {
            let is_min = state.workspaces.is_minimized(w);
            let is_focused = !is_min
                && w.toplevel()
                    .map(|t| Some(t.wl_surface().id()) == focused_id)
                    .unwrap_or(false);
            let title = window_title(w);
            (title, is_focused, is_min)
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

    // 1. Render the taskbar RGBA buffer
    let mut canvas = Canvas::new(bar_w, bar_h);
    // Taskbar background (#14141e @ 94% alpha) + top accent hairline
    canvas.fill_rect(0, 0, bar_w, bar_h, [20, 20, 30, 240]);
    canvas.fill_rect(0, 0, bar_w, 1, [201, 160, 220, 110]);

    let mut cursor_x = 8;

    // "OKay" Start button
    let start_w = 64;
    let start_bg = if state.panel.launcher_open {
        [201, 160, 220, 255]
    } else {
        [45, 45, 64, 255]
    };
    let start_fg = if state.panel.launcher_open {
        [18, 18, 26, 255]
    } else {
        [235, 235, 245, 255]
    };
    canvas.fill_rect(cursor_x, 6, start_w, bar_h - 12, start_bg);
    canvas.draw_text(cursor_x + 12, 13, "OKay", 2, start_fg);
    state.panel.hit_regions.push((
        Rectangle::new(
            (output_geo.loc.x + cursor_x, output_geo.loc.y + bar_y_local + 6).into(),
            (start_w, bar_h - 12).into(),
        ),
        ClickTarget::ToggleLauncher,
    ));
    cursor_x += start_w + 10;

    // Workspace indicators (1..9)
    for ws in 1..=9 {
        let ws_w = 24;
        let is_active = state.workspaces.active == ws;
        let is_occupied = occupied_workspaces[ws - 1];

        let bg = if is_active {
            [201, 160, 220, 255]
        } else if is_occupied {
            [42, 42, 60, 255]
        } else {
            [28, 28, 40, 220]
        };
        let fg = if is_active {
            [18, 18, 26, 255]
        } else if is_occupied {
            [225, 225, 240, 255]
        } else {
            [130, 135, 160, 255]
        };

        canvas.fill_rect(cursor_x, 7, ws_w, bar_h - 14, bg);
        let label = ws.to_string();
        canvas.draw_text(cursor_x + 7, 13, &label, 2, fg);
        state.panel.hit_regions.push((
            Rectangle::new(
                (output_geo.loc.x + cursor_x, output_geo.loc.y + bar_y_local + 7).into(),
                (ws_w, bar_h - 14).into(),
            ),
            ClickTarget::Workspace(ws),
        ));
        cursor_x += ws_w + 4;
    }

    cursor_x += 8;

    // System status indicator (layout mode + window count) & live system clock on the right
    let clock_str = current_hh_mm();
    let clock_w = 72;
    let clock_x = (bar_w - clock_w - 8).max(cursor_x);
    canvas.fill_rect(clock_x, 6, clock_w, bar_h - 12, [34, 34, 50, 230]);
    canvas.draw_text(clock_x + 11, 13, &clock_str, 2, [230, 232, 245, 255]);

    let mode_label = if state.workspaces.tiling_enabled {
        "TILE"
    } else {
        "FLT"
    };
    let status_w = 58;
    let status_x = (clock_x - status_w - 6).max(cursor_x);
    if status_x + status_w <= clock_x - 4 {
        let status_bg = if state.workspaces.tiling_enabled {
            [56, 46, 78, 240]
        } else {
            [34, 34, 50, 230]
        };
        canvas.fill_rect(status_x, 6, status_w, bar_h - 12, status_bg);
        canvas.draw_text(status_x + 8, 13, mode_label, 2, [201, 160, 220, 255]);
        state.panel.hit_regions.push((
            Rectangle::new(
                (output_geo.loc.x + status_x, output_geo.loc.y + bar_y_local + 6).into(),
                (status_w, bar_h - 12).into(),
            ),
            ClickTarget::ToggleTiling,
        ));
    }

    // Open window task buttons (click to focus/minimize/raise)
    let avail_w = (status_x - cursor_x - 8).max(0);
    if !ws_windows.is_empty() && avail_w > 40 {
        let max_btn_w = 170;
        let btn_w = ((avail_w / ws_windows.len() as i32) - 6).clamp(48, max_btn_w);
        for (idx, (title, is_focused, is_min)) in ws_windows.iter().enumerate() {
            let bx = cursor_x + idx as i32 * (btn_w + 6);
            if bx + btn_w > status_x - 4 {
                break;
            }
            let bg = if *is_focused {
                [56, 50, 78, 255]
            } else if *is_min {
                [24, 24, 34, 200]
            } else {
                [32, 32, 46, 230]
            };
            let fg = if *is_min {
                [140, 142, 165, 255]
            } else {
                [235, 235, 248, 255]
            };
            canvas.fill_rect(bx, 6, btn_w, bar_h - 12, bg);
            if *is_focused {
                canvas.fill_rect(bx, bar_h - 8, btn_w, 2, [201, 160, 220, 255]);
            }
            let max_chars = ((btn_w - 16) / 12).max(1) as usize;
            let short: String = title.chars().take(max_chars).collect();
            canvas.draw_text(bx + 8, 13, &short, 2, fg);

            state.panel.hit_regions.push((
                Rectangle::new(
                    (output_geo.loc.x + bx, output_geo.loc.y + bar_y_local + 6).into(),
                    (btn_w, bar_h - 12).into(),
                ),
                ClickTarget::FocusWindow(idx),
            ));
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
        Point::<i32, Logical>::from((0, bar_y_local))
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

    // 2. Render Launcher Popup Menu (only on the primary output when open)
    if state.panel.launcher_open && is_primary {
        let menu_h = MENU_PADDING * 2 + LauncherItem::ALL.len() as i32 * MENU_ITEM_HEIGHT;
        let menu_x = 8;
        let menu_y = (bar_y_local - menu_h - 6).max(8);

        let mut menu_canvas = Canvas::new(MENU_WIDTH, menu_h);
        menu_canvas.fill_rect(0, 0, MENU_WIDTH, menu_h, [22, 22, 34, 248]);
        menu_canvas.stroke_rect(0, 0, MENU_WIDTH, menu_h, [201, 160, 220, 180]);

        for (idx, item) in LauncherItem::ALL.iter().enumerate() {
            let iy = MENU_PADDING + idx as i32 * MENU_ITEM_HEIGHT;
            let is_exit = *item == LauncherItem::ExitSession;
            let item_bg = if is_exit {
                [56, 30, 42, 230]
            } else {
                [34, 34, 50, 230]
            };
            let item_fg = if is_exit {
                [243, 139, 168, 255]
            } else {
                [235, 235, 248, 255]
            };

            menu_canvas.fill_rect(
                MENU_PADDING,
                iy + 2,
                MENU_WIDTH - MENU_PADDING * 2,
                MENU_ITEM_HEIGHT - 4,
                item_bg,
            );
            menu_canvas.draw_text(MENU_PADDING + 12, iy + 10, item.label(), 2, item_fg);

            state.panel.hit_regions.push((
                Rectangle::new(
                    (
                        output_geo.loc.x + menu_x + MENU_PADDING,
                        output_geo.loc.y + menu_y + iy + 2,
                    )
                        .into(),
                    (MENU_WIDTH - MENU_PADDING * 2, MENU_ITEM_HEIGHT - 4).into(),
                ),
                ClickTarget::LaunchItem(*item),
            ));
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

/// Renders a server-side titlebar (with title text and `[-]`, `[+]`, `[X]` buttons)
/// directly above `window` at output-local coordinates.
pub fn render_titlebar_element(
    window: &Window,
    local_win_loc: Point<i32, Logical>,
    is_focused: bool,
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
        [34, 30, 50, 250]
    } else {
        [24, 24, 36, 240]
    };
    let top_accent = if is_focused {
        [201, 160, 220, 255]
    } else {
        [60, 60, 82, 255]
    };
    canvas.fill_rect(0, 0, tb_w, tb_h, bg);
    canvas.fill_rect(0, 0, tb_w, 2, top_accent);

    let title = window_title(window);
    let max_chars = ((tb_w - 96) / 6).max(1) as usize;
    let short_title: String = title.chars().take(max_chars).collect();
    let title_fg = if is_focused {
        [238, 238, 250, 255]
    } else {
        [165, 168, 190, 255]
    };
    canvas.draw_text(10, 9, &short_title, 1, title_fg);

    // Minimize button [-]: [tb_w - 80 .. tb_w - 58]
    if tb_w >= 120 {
        canvas.fill_rect(tb_w - 80, 4, 22, tb_h - 8, [48, 48, 68, 255]);
        canvas.draw_text(tb_w - 71, 9, "-", 1, [225, 228, 245, 255]);

        // Maximize button [+]: [tb_w - 54 .. tb_w - 32]
        canvas.fill_rect(tb_w - 54, 4, 22, tb_h - 8, [48, 48, 68, 255]);
        canvas.draw_text(tb_w - 45, 9, "+", 1, [225, 228, 245, 255]);

        // Close button [X]: [tb_w - 28 .. tb_w - 6]
        canvas.fill_rect(tb_w - 28, 4, 22, tb_h - 8, [185, 65, 90, 255]);
        canvas.draw_text(tb_w - 19, 9, "X", 1, [255, 245, 248, 255]);
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

fn window_title(window: &Window) -> String {
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

fn current_hh_mm() -> String {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return "00:00".to_string();
        }
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    }
}

// ============================================================================
// Lightweight Software Canvas + Embedded 5x7 Pixel Font
// ============================================================================

struct Canvas {
    w: i32,
    h: i32,
    pixels: Vec<u8>,
}

impl Canvas {
    fn new(w: i32, h: i32) -> Self {
        Self {
            w,
            h,
            pixels: vec![0; (w * h * 4) as usize],
        }
    }

    fn fill_rect(&mut self, x: i32, y: i32, rw: i32, rh: i32, rgba: [u8; 4]) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + rw).min(self.w);
        let y1 = (y + rh).min(self.h);
        for py in y0..y1 {
            let row_off = (py * self.w) as usize;
            for px in x0..x1 {
                let idx = (row_off + px as usize) * 4;
                self.pixels[idx..idx + 4].copy_from_slice(&rgba);
            }
        }
    }

    fn stroke_rect(&mut self, x: i32, y: i32, rw: i32, rh: i32, rgba: [u8; 4]) {
        self.fill_rect(x, y, rw, 1, rgba);
        self.fill_rect(x, y + rh - 1, rw, 1, rgba);
        self.fill_rect(x, y, 1, rh, rgba);
        self.fill_rect(x + rw - 1, y, 1, rh, rgba);
    }

    fn draw_text(&mut self, mut x: i32, y: i32, text: &str, scale: i32, rgba: [u8; 4]) {
        for ch in text.chars() {
            let glyph = glyph_5x7(ch);
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..5 {
                    if (bits >> (4 - col)) & 1 == 1 {
                        self.fill_rect(
                            x + col * scale,
                            y + row as i32 * scale,
                            scale,
                            scale,
                            rgba,
                        );
                    }
                }
            }
            x += 6 * scale;
        }
    }
}

fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11100, 0b10010, 0b10001, 0b10001, 0b10001, 0b10010, 0b11100],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        'a' => [0b00000, 0b00000, 0b01110, 0b00001, 0b01111, 0b10001, 0b01111],
        'b' => [0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b11110],
        'c' => [0b00000, 0b00000, 0b01110, 0b10000, 0b10000, 0b10001, 0b01110],
        'd' => [0b00001, 0b00001, 0b01111, 0b10001, 0b10001, 0b10001, 0b01111],
        'e' => [0b00000, 0b00000, 0b01110, 0b10001, 0b11111, 0b10000, 0b01110],
        'f' => [0b00110, 0b01001, 0b01000, 0b11100, 0b01000, 0b01000, 0b01000],
        'g' => [0b00000, 0b01111, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110],
        'h' => [0b10000, 0b10000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001],
        'i' => [0b00100, 0b00000, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110],
        'j' => [0b00010, 0b00000, 0b00110, 0b00010, 0b00010, 0b10010, 0b01100],
        'k' => [0b10000, 0b10000, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010],
        'l' => [0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'm' => [0b00000, 0b00000, 0b11010, 0b10101, 0b10101, 0b10001, 0b10001],
        'n' => [0b00000, 0b00000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001],
        'o' => [0b00000, 0b00000, 0b01110, 0b10001, 0b10001, 0b10001, 0b01110],
        'p' => [0b00000, 0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000],
        'q' => [0b00000, 0b01111, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001],
        'r' => [0b00000, 0b00000, 0b10110, 0b11001, 0b10000, 0b10000, 0b10000],
        's' => [0b00000, 0b00000, 0b01111, 0b10000, 0b01110, 0b00001, 0b11110],
        't' => [0b01000, 0b01000, 0b11100, 0b01000, 0b01000, 0b01001, 0b00110],
        'u' => [0b00000, 0b00000, 0b10001, 0b10001, 0b10001, 0b10011, 0b01101],
        'v' => [0b00000, 0b00000, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'w' => [0b00000, 0b00000, 0b10001, 0b10001, 0b10101, 0b10101, 0b01010],
        'x' => [0b00000, 0b00000, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001],
        'y' => [0b00000, 0b10001, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110],
        'z' => [0b00000, 0b00000, 0b11111, 0b00010, 0b00100, 0b01000, 0b11111],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111],
        '3' => [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110],
        ':' => [0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000],
        '+' => [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '_' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100],
        '/' => [0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000],
        ' ' => [0b00000; 7],
        _ => [0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111],
    }
}
