use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    path::PathBuf,
    time::Instant,
};

use calloop::LoopSignal;
use smithay::{
    desktop::{layer_map_for_output, PopupManager, Space, Window, WindowSurfaceType},
    input::{
        keyboard::XkbConfig,
        pointer::CursorImageStatus,
        Seat, SeatState,
    },
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            backend::{ClientData, ClientId, DisconnectReason, ObjectId},
            protocol::wl_surface::WlSurface,
            Display, DisplayHandle, Resource,
        },
    },
    utils::{Logical, Point, Rectangle, Size, SERIAL_COUNTER},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        output::OutputManagerState,
        selection::{
            data_device::{set_data_device_selection, DataDeviceState},
            wlr_data_control::DataControlState,
        },
        shell::{
            wlr_layer::{Layer, WlrLayerShellState},
            xdg::{decoration::XdgDecorationState, XdgShellState},
        },
        shm::ShmState,
    },
};

pub const TITLEBAR_HEIGHT: i32 = 28;

/// Visual theme and rendering configuration for OKay DE.
/// Decoupled from the renderer so custom GLSL shaders, animations, and
/// user settings can mutate this state at runtime.
#[derive(Debug, Clone)]
pub struct CompositorTheme {
    /// Background clear color `[r, g, b, a]` (linear RGBA).
    pub background_color: [f32; 4],
    /// Border color for the currently focused window.
    pub active_border_color: [f32; 4],
    /// Border color for unfocused windows.
    pub inactive_border_color: [f32; 4],
    /// Border thickness in logical pixels.
    pub border_width: i32,
}

impl Default for CompositorTheme {
    fn default() -> Self {
        Self {
            // Deep obsidian slate (#0d0d14)
            background_color: [0.051, 0.051, 0.078, 1.0],
            // Soft lavender accent (#c9a0dc)
            active_border_color: [0.788, 0.627, 0.863, 1.0],
            // Muted slate (#2a2a3a)
            inactive_border_color: [0.165, 0.165, 0.227, 1.0],
            border_width: 2,
        }
    }
}

/// Per-workspace or global window layout mode (`Super + T` cycles through these).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    Floating,
    MasterStack,
    Dwindle,
}

impl LayoutMode {
    pub fn next(self) -> Self {
        match self {
            Self::Floating => Self::MasterStack,
            Self::MasterStack => Self::Dwindle,
            Self::Dwindle => Self::Floating,
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Floating => "FLT",
            Self::MasterStack => "TILE",
            Self::Dwindle => "DWND",
        }
    }
}

/// Edge / corner snap target when dragging windows to screen edges or pressing `Super + Arrows`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapZone {
    LeftHalf,
    RightHalf,
    BottomHalf,
    TopLeftQuarter,
    TopRightQuarter,
    BottomLeftQuarter,
    BottomRightQuarter,
    Maximize,
}

/// Manages virtual workspaces (1..=9), per-window states, and tiling/floating layout.
#[derive(Debug)]
pub struct WorkspaceManager {
    pub active: usize,
    pub count: usize,
    pub window_map: HashMap<ObjectId, usize>,
    pub minimized: HashSet<ObjectId>,
    pub fullscreen: HashSet<ObjectId>,
    pub maximized: HashSet<ObjectId>,
    pub always_on_top: HashSet<ObjectId>,
    pub saved_geometries: HashMap<ObjectId, Rectangle<i32, Logical>>,
    pub layout_mode: LayoutMode,
    pub per_workspace_layouts: HashMap<usize, LayoutMode>,
    pub master_ratio: f32,
    pub tile_order: Vec<ObjectId>,
}

impl Default for WorkspaceManager {
    fn default() -> Self {
        Self {
            active: 1,
            count: 9,
            window_map: HashMap::new(),
            minimized: HashSet::new(),
            fullscreen: HashSet::new(),
            maximized: HashSet::new(),
            always_on_top: HashSet::new(),
            saved_geometries: HashMap::new(),
            layout_mode: LayoutMode::Floating,
            per_workspace_layouts: HashMap::new(),
            master_ratio: 0.50,
            tile_order: Vec::new(),
        }
    }
}

impl WorkspaceManager {
    pub fn active_layout(&self) -> LayoutMode {
        self.per_workspace_layouts
            .get(&self.active)
            .copied()
            .unwrap_or(self.layout_mode)
    }

    pub fn set_active_layout(&mut self, mode: LayoutMode) {
        self.layout_mode = mode;
        self.per_workspace_layouts.insert(self.active, mode);
    }

    pub fn tiling_enabled(&self) -> bool {
        self.active_layout() != LayoutMode::Floating
    }

    pub fn assign_window(&mut self, surface_id: ObjectId, workspace: usize) {
        let ws = workspace.clamp(1, self.count);
        self.window_map.insert(surface_id.clone(), ws);
        if !self.tile_order.contains(&surface_id) {
            self.tile_order.push(surface_id);
        }
    }

    pub fn remove_window(&mut self, surface_id: &ObjectId) {
        self.window_map.remove(surface_id);
        self.minimized.remove(surface_id);
        self.fullscreen.remove(surface_id);
        self.maximized.remove(surface_id);
        self.always_on_top.remove(surface_id);
        self.saved_geometries.remove(surface_id);
        self.tile_order.retain(|x| x != surface_id);
    }

    pub fn window_workspace(&self, surface_id: &ObjectId) -> usize {
        self.window_map.get(surface_id).copied().unwrap_or(self.active)
    }

    pub fn is_on_active_workspace(&self, window: &Window) -> bool {
        if let Some(toplevel) = window.toplevel() {
            let id = toplevel.wl_surface().id();
            return self.window_workspace(&id) == self.active;
        }
        true
    }

    pub fn is_minimized(&self, window: &Window) -> bool {
        window
            .toplevel()
            .map(|t| self.minimized.contains(&t.wl_surface().id()))
            .unwrap_or(false)
    }

    pub fn is_fullscreen(&self, window: &Window) -> bool {
        window
            .toplevel()
            .map(|t| self.fullscreen.contains(&t.wl_surface().id()))
            .unwrap_or(false)
    }

    pub fn is_pinned(&self, window: &Window) -> bool {
        window
            .toplevel()
            .map(|t| self.always_on_top.contains(&t.wl_surface().id()))
            .unwrap_or(false)
    }

    pub fn is_visible(&self, window: &Window) -> bool {
        self.is_on_active_workspace(window) && !self.is_minimized(window)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResizeEdges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

/// Interactive window move / resize grab state.
#[derive(Debug, Clone)]
pub enum PointerGrabMode {
    Move {
        window: Window,
        initial_pointer: Point<f64, Logical>,
        initial_window_loc: Point<i32, Logical>,
    },
    Resize {
        window: Window,
        initial_pointer: Point<f64, Logical>,
        initial_window_loc: Point<i32, Logical>,
        initial_size: Size<i32, Logical>,
        edges: ResizeEdges,
    },
}

/// Per-client Wayland state required by Smithay's `CompositorHandler`.
#[derive(Debug, Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

/// Central compositor state for OKay DE.
pub struct OkayState {
    pub display_handle: DisplayHandle,
    pub loop_signal: LoopSignal,
    pub running: bool,
    pub start_time: Instant,
    pub socket_name: OsString,

    // Wayland protocol states
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    #[allow(dead_code)]
    pub xdg_decoration_state: XdgDecorationState,
    pub layer_shell_state: WlrLayerShellState,
    pub shm_state: ShmState,
    #[allow(dead_code)]
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Self>,
    pub data_device_state: DataDeviceState,
    #[allow(dead_code)]
    pub data_control_state: DataControlState,

    // Seat & input tracking
    pub seat: Seat<Self>,
    pub pointer_location: Point<f64, Logical>,
    pub cursor_status: CursorImageStatus,
    pub pointer_grab: Option<PointerGrabMode>,
    pub super_pressed: bool,
    pub pointer_sensitivity: f64,
    pub natural_scroll: bool,
    pub active_clipboard_text: String,
    pub pending_screencopies: Vec<crate::screencopy::PendingScreencopy>,
    pub pending_native_screenshot: Option<(PathBuf, Option<Rectangle<i32, Logical>>)>,
    pub last_titlebar_click: Option<(ObjectId, Instant)>,
    pub last_status_poll: Instant,

    // Desktop, window registry & workspaces
    pub space: Space<Window>,
    pub popups: PopupManager,
    pub workspaces: WorkspaceManager,
    pub focus_stack: Vec<ObjectId>,
    pub next_spawn_offset: i32,

    // Visual theme & future shader/animation hooks
    pub theme: CompositorTheme,

    // Native desktop wallpaper, taskbar/launcher panel, settings window, animations & overlays
    pub desktop: crate::desktop::DesktopState,
    pub panel: crate::panel::PanelState,
    pub settings: crate::settings::SettingsState,
    pub animations: crate::animation::AnimationManager,
    pub overlays: crate::overlays::OverlaysState,
}

impl OkayState {
    pub fn new(
        display: &Display<Self>,
        loop_signal: LoopSignal,
        socket_name: OsString,
    ) -> Self {
        let display_handle = display.handle();

        let compositor_state = CompositorState::new::<Self>(&display_handle);
        let xdg_shell_state = XdgShellState::new::<Self>(&display_handle);
        let xdg_decoration_state = XdgDecorationState::new::<Self>(&display_handle);
        let layer_shell_state = WlrLayerShellState::new::<Self>(&display_handle);
        let shm_state = ShmState::new::<Self>(&display_handle, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&display_handle);
        let mut seat_state = SeatState::new();
        let data_device_state = DataDeviceState::new::<Self>(&display_handle);
        let data_control_state = DataControlState::new::<Self, _>(&display_handle, None, |_| true);
        crate::screencopy::init_screencopy_global(&display_handle);

        let mut seat = seat_state.new_wl_seat(&display_handle, "seat-0");
        if let Err(err) = seat.add_keyboard(XkbConfig::default(), 250, 35) {
            tracing::warn!(?err, "Failed to initialize XKB keyboard on seat-0");
        }
        seat.add_pointer();

        let mut panel = crate::panel::PanelState::new();
        let cfg = panel.load_config_file();
        let mut theme = CompositorTheme::default();
        theme.active_border_color = panel.accent_f32();

        let mut animations = crate::animation::AnimationManager::new();
        animations.enabled = cfg.anim_enabled;
        animations.speed = cfg.anim_speed;
        animations.minimize_effect = cfg.minimize_effect;

        let mut settings = crate::settings::SettingsState::new();
        settings.xkb_layout = cfg.xkb_layout;

        let mut overlays = crate::overlays::OverlaysState::new();
        overlays.lock_pin = cfg.lock_pin;

        Self {
            display_handle,
            loop_signal,
            running: true,
            start_time: Instant::now(),
            socket_name,
            compositor_state,
            xdg_shell_state,
            xdg_decoration_state,
            layer_shell_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            data_control_state,
            seat,
            pointer_location: Point::from((0.0, 0.0)),
            cursor_status: CursorImageStatus::default_named(),
            pointer_grab: None,
            super_pressed: false,
            pointer_sensitivity: cfg.pointer_speed,
            natural_scroll: cfg.natural_scroll,
            active_clipboard_text: String::new(),
            pending_screencopies: Vec::new(),
            pending_native_screenshot: None,
            last_titlebar_click: None,
            last_status_poll: Instant::now(),
            space: Space::default(),
            popups: PopupManager::default(),
            workspaces: WorkspaceManager::default(),
            focus_stack: Vec::new(),
            next_spawn_offset: 0,
            theme,
            desktop: crate::desktop::DesktopState::new(),
            panel,
            settings,
            animations,
            overlays,
        }
    }

    /// Sets the compositor-owned Wayland clipboard selection so any Wayland app can paste it.
    pub fn set_compositor_clipboard(&mut self, text: String) {
        self.active_clipboard_text = text;
        set_data_device_selection(
            &self.display_handle,
            &self.seat,
            vec![
                "text/plain;charset=utf-8".to_string(),
                "text/plain".to_string(),
                "UTF8_STRING".to_string(),
            ],
            (),
        );
    }

    /// Spawns user-configured `autostart` commands and `~/.config/okay-de/autostart.sh`
    /// in the background after `WAYLAND_DISPLAY` is set.
    pub fn spawn_autostart(&self) {
        for cmd in &self.panel.autostart_cmds {
            let trimmed = cmd.trim();
            if trimmed.is_empty() {
                continue;
            }
            tracing::info!(cmd = %trimmed, "Launching autostart command");
            let _ = std::process::Command::new("sh")
                .arg("-c")
                .arg(trimmed)
                .env("WAYLAND_DISPLAY", &self.socket_name)
                .spawn();
        }
        if let Ok(home) = std::env::var("HOME") {
            let script = PathBuf::from(home).join(".config/okay-de/autostart.sh");
            if script.is_file() {
                tracing::info!(?script, "Launching autostart.sh");
                let _ = std::process::Command::new("sh")
                    .arg(&script)
                    .env("WAYLAND_DISPLAY", &self.socket_name)
                    .spawn();
            }
        }
    }

    /// Returns the currently focused `Window`, if any is visible on the active workspace.
    pub fn focused_window(&self) -> Option<Window> {
        let keyboard = self.seat.get_keyboard()?;
        let focused_surface = keyboard.current_focus()?;
        self.space
            .elements()
            .find(|w| {
                self.workspaces.is_visible(w)
                    && w.toplevel()
                        .map(|t| t.wl_surface().id() == focused_surface.id())
                        .unwrap_or(false)
            })
            .cloned()
    }

    /// Focuses and raises a window, updating keyboard focus and the focus stack.
    pub fn focus_window(&mut self, window: &Window) {
        self.space.raise_element(window, true);
        if let Some(toplevel) = window.toplevel() {
            let surface = toplevel.wl_surface().clone();
            let id = surface.id();
            self.focus_stack.retain(|x| x != &id);
            self.focus_stack.push(id);

            if let Some(keyboard) = self.seat.get_keyboard() {
                keyboard.set_focus(self, Some(surface), SERIAL_COUNTER.next_serial());
            }
        }
    }

    /// Clears keyboard focus or focuses the topmost visible window on the active workspace.
    pub fn refocus_topmost_visible(&mut self) {
        let topmost = self
            .space
            .elements()
            .rev()
            .find(|w| self.workspaces.is_visible(w))
            .cloned();

        if let Some(win) = topmost {
            self.focus_window(&win);
        } else if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, None, SERIAL_COUNTER.next_serial());
        }
    }

    /// Switches the active workspace (1..=9) with a smooth horizontal slide animation.
    pub fn switch_workspace(&mut self, index: usize) {
        let target = index.clamp(1, self.workspaces.count);
        let old = self.workspaces.active;
        if old == target {
            return;
        }
        let out_w = self
            .output_under_pointer()
            .and_then(|o| self.space.output_geometry(&o))
            .map(|g| g.size.w)
            .unwrap_or(1280);
        self.workspaces.active = target;
        self.animations.on_workspace_switched(old, target, out_w);
        tracing::info!(workspace = target, "Switched workspace");
        self.retile_workspace();
        self.refocus_topmost_visible();
    }

    /// Moves the currently focused window to another workspace.
    pub fn move_focused_to_workspace(&mut self, index: usize) {
        let target = index.clamp(1, self.workspaces.count);
        if let Some(win) = self.focused_window() {
            if let Some(toplevel) = win.toplevel() {
                self.workspaces
                    .assign_window(toplevel.wl_surface().id(), target);
                tracing::info!(workspace = target, "Moved focused window to workspace");
                self.retile_workspace();
                self.refocus_topmost_visible();
            }
        }
    }

    /// Sends a close request to the currently focused window.
    pub fn close_focused_window(&mut self) {
        if let Some(win) = self.focused_window() {
            if let Some(toplevel) = win.toplevel() {
                tracing::debug!("Sending close request to focused toplevel");
                toplevel.send_close();
            }
        }
    }

    /// Toggles fullscreen state (`Super + F`) for the currently focused window.
    pub fn toggle_fullscreen_focused(&mut self) {
        let Some(win) = self.focused_window() else {
            return;
        };
        let Some(toplevel) = win.toplevel().cloned() else {
            return;
        };
        let id = toplevel.wl_surface().id();
        let output_geo = self
            .output_under_pointer()
            .and_then(|o| self.space.output_geometry(&o))
            .unwrap_or_else(|| Rectangle::from_size((1280, 720).into()));

        if self.workspaces.fullscreen.remove(&id) {
            let restore = self
                .workspaces
                .saved_geometries
                .remove(&id)
                .unwrap_or_else(|| Rectangle::new((output_geo.loc.x + 80, output_geo.loc.y + 60).into(), (800, 600).into()));
            self.space.map_element(win, restore.loc, true);
            toplevel.with_pending_state(|state| {
                state.states.unset(xdg_toplevel::State::Fullscreen);
                state.size = Some(restore.size);
            });
            toplevel.send_configure();
            self.retile_workspace();
        } else {
            if let Some(loc) = self.space.element_location(&win) {
                self.workspaces
                    .saved_geometries
                    .insert(id.clone(), Rectangle::new(loc, win.geometry().size));
            }
            self.workspaces.fullscreen.insert(id);
            self.space.map_element(win, output_geo.loc, true);
            toplevel.with_pending_state(|state| {
                state.states.set(xdg_toplevel::State::Fullscreen);
                state.size = Some(output_geo.size);
            });
            toplevel.send_configure();
        }
    }

    /// Toggles maximized state for `window` (used by titlebar `[+]` button and XDG requests).
    pub fn toggle_maximize_window(&mut self, window: &Window) {
        let Some(toplevel) = window.toplevel().cloned() else {
            return;
        };
        let id = toplevel.wl_surface().id();
        let output_geo = self
            .output_under_pointer()
            .and_then(|o| self.space.output_geometry(&o))
            .unwrap_or_else(|| Rectangle::from_size((1280, 720).into()));

        if self.workspaces.maximized.remove(&id) {
            let restore = self
                .workspaces
                .saved_geometries
                .remove(&id)
                .unwrap_or_else(|| Rectangle::new((output_geo.loc.x + 80, output_geo.loc.y + 60).into(), (800, 600).into()));
            self.space.map_element(window.clone(), restore.loc, true);
            toplevel.with_pending_state(|state| {
                state.states.unset(xdg_toplevel::State::Maximized);
                state.size = Some(restore.size);
            });
            toplevel.send_configure();
        } else {
            if let Some(loc) = self.space.element_location(window) {
                self.workspaces
                    .saved_geometries
                    .insert(id.clone(), Rectangle::new(loc, window.geometry().size));
            }
            self.workspaces.maximized.insert(id);
            let top_pad = if self.panel.dock_mode || self.panel.panel_top {
                crate::panel::PANEL_HEIGHT
            } else {
                0
            };
            let bot_pad = if self.panel.dock_mode {
                crate::panel::DOCK_HEIGHT + crate::panel::DOCK_MARGIN + 4
            } else if !self.panel.panel_top {
                36
            } else {
                0
            };
            let target_loc = Point::from((output_geo.loc.x, output_geo.loc.y + top_pad + TITLEBAR_HEIGHT));
            let target_size = Size::from((
                output_geo.size.w,
                (output_geo.size.h - top_pad - bot_pad - TITLEBAR_HEIGHT).max(120),
            ));
            self.space.map_element(window.clone(), target_loc, true);
            toplevel.with_pending_state(|state| {
                state.states.set(xdg_toplevel::State::Maximized);
                state.size = Some(target_size);
            });
            toplevel.send_configure();
        }
    }

    /// Toggles minimized state for `window` with **Magic Lamp (Genie)** or ScaleGlide animation!
    pub fn toggle_minimize_window(&mut self, window: &Window) {
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        let id = toplevel.wl_surface().id();
        let win_loc_opt = self.space.element_location(window);
        let output_geo = win_loc_opt
            .and_then(|loc| {
                self.space
                    .outputs()
                    .filter_map(|o| self.space.output_geometry(o))
                    .find(|g| g.contains(loc))
            })
            .or_else(|| {
                self.output_under_pointer()
                    .and_then(|o| self.space.output_geometry(&o))
            })
            .or_else(|| {
                self.space
                    .outputs()
                    .next()
                    .and_then(|o| self.space.output_geometry(o))
            })
            .unwrap_or_else(|| Rectangle::from_size((1280, 720).into()));
        let win_loc = win_loc_opt
            .unwrap_or_else(|| Point::from((output_geo.loc.x + 120, output_geo.loc.y + 80)));
        let win_geo = window.geometry();
        let local_rect = Rectangle::new(
            win_loc - output_geo.loc + win_geo.loc - Point::from((0, TITLEBAR_HEIGHT)),
            (win_geo.size.w.max(120), win_geo.size.h + TITLEBAR_HEIGHT).into(),
        );
        let accent = self.panel.accent_rgb();

        if self.workspaces.minimized.remove(&id) {
            self.animations.start_unminimize(
                id,
                local_rect,
                output_geo.loc,
                output_geo.size,
                accent,
            );
            self.focus_window(window);
            self.retile_workspace();
        } else {
            self.animations.start_minimize(
                id.clone(),
                local_rect,
                output_geo.loc,
                output_geo.size,
                accent,
            );
            self.workspaces.minimized.insert(id);
            self.retile_workspace();
            self.refocus_topmost_visible();
        }
    }

    /// Toggles Always-on-Top (`Super + A`) for the currently focused window.
    pub fn toggle_always_on_top_focused(&mut self) {
        if let Some(win) = self.focused_window() {
            if let Some(toplevel) = win.toplevel() {
                let id = toplevel.wl_surface().id();
                if !self.workspaces.always_on_top.remove(&id) {
                    self.workspaces.always_on_top.insert(id);
                    self.space.raise_element(&win, true);
                }
            }
        }
    }

    /// Computes the exact logical rectangle for `zone` inside `output_geo`, accounting
    /// for the Top Bar, Dock, titlebar height, and configured window gaps.
    pub fn compute_snap_rect(
        &self,
        output_geo: Rectangle<i32, Logical>,
        zone: SnapZone,
    ) -> Rectangle<i32, Logical> {
        let gap = self.panel.window_gaps;
        let top_bar_pad = if self.panel.dock_mode || self.panel.panel_top {
            crate::panel::PANEL_HEIGHT
        } else {
            0
        };
        let bot_bar_pad = if self.panel.dock_mode {
            crate::panel::DOCK_HEIGHT + crate::panel::DOCK_MARGIN + 4
        } else if !self.panel.panel_top {
            36
        } else {
            0
        };
        if zone == SnapZone::Maximize {
            return Rectangle::new(
                (output_geo.loc.x, output_geo.loc.y + top_bar_pad + TITLEBAR_HEIGHT).into(),
                (
                    output_geo.size.w,
                    (output_geo.size.h - top_bar_pad - bot_bar_pad - TITLEBAR_HEIGHT).max(120),
                )
                    .into(),
            );
        }
        let ax = output_geo.loc.x + gap;
        let ay = output_geo.loc.y + top_bar_pad + gap + TITLEBAR_HEIGHT;
        let aw = (output_geo.size.w - gap * 2).max(200);
        let ah =
            (output_geo.size.h - top_bar_pad - bot_bar_pad - gap * 2 - TITLEBAR_HEIGHT).max(200);
        let half_w = (aw - gap) / 2;
        let half_h = (ah - gap - TITLEBAR_HEIGHT) / 2;

        let (nx, ny, nw, nh) = match zone {
            SnapZone::LeftHalf => (ax, ay, half_w, ah),
            SnapZone::RightHalf => (ax + half_w + gap, ay, half_w, ah),
            SnapZone::BottomHalf => (ax, ay + half_h + gap + TITLEBAR_HEIGHT, aw, half_h),
            SnapZone::TopLeftQuarter => (ax, ay, half_w, half_h),
            SnapZone::TopRightQuarter => (ax + half_w + gap, ay, half_w, half_h),
            SnapZone::BottomLeftQuarter => (ax, ay + half_h + gap + TITLEBAR_HEIGHT, half_w, half_h),
            SnapZone::BottomRightQuarter => {
                (ax + half_w + gap, ay + half_h + gap + TITLEBAR_HEIGHT, half_w, half_h)
            }
            SnapZone::Maximize => unreachable!(),
        };
        Rectangle::new((nx, ny).into(), (nw.max(120), nh.max(90)).into())
    }

    /// Detects if `pos` is near a screen edge or corner to trigger snap-to-zone + live preview.
    pub fn detect_snap_zone(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(SnapZone, Rectangle<i32, Logical>)> {
        let out = self.output_under_pointer()?;
        let geo = self.space.output_geometry(&out)?;
        let px = pos.x.round() as i32;
        let py = pos.y.round() as i32;
        let top_thresh = geo.loc.y + crate::panel::PANEL_HEIGHT + 12;
        let left = px <= geo.loc.x + 14;
        let right = px >= geo.loc.x + geo.size.w - 16;
        let top = py <= top_thresh;
        let bot = py >= geo.loc.y + geo.size.h - 16;

        let zone = if left && top {
            Some(SnapZone::TopLeftQuarter)
        } else if right && top {
            Some(SnapZone::TopRightQuarter)
        } else if left && bot {
            Some(SnapZone::BottomLeftQuarter)
        } else if right && bot {
            Some(SnapZone::BottomRightQuarter)
        } else if left {
            Some(SnapZone::LeftHalf)
        } else if right {
            Some(SnapZone::RightHalf)
        } else if top {
            Some(SnapZone::Maximize)
        } else {
            None
        }?;

        let rect = self.compute_snap_rect(geo, zone);
        Some((zone, rect))
    }

    /// Unmaximizes `window` cleanly if the user starts dragging or resizing it.
    pub fn unmaximize_if_needed(&mut self, window: &Window) {
        if let Some(toplevel) = window.toplevel() {
            let id = toplevel.wl_surface().id();
            if self.workspaces.maximized.remove(&id) {
                self.workspaces.saved_geometries.remove(&id);
                toplevel.with_pending_state(|s| {
                    s.states.unset(xdg_toplevel::State::Maximized);
                });
                toplevel.send_configure();
            }
        }
    }

    /// Returns visible window and resize edges if `pos` hovers within 6px of a window border/corner.
    pub fn window_resize_edges_at(&self, pos: Point<f64, Logical>) -> Option<(Window, ResizeEdges)> {
        let px = pos.x.round() as i32;
        let py = pos.y.round() as i32;
        let border = 6;

        for window in self.space.elements().rev() {
            if !self.workspaces.is_visible(window) || self.workspaces.is_fullscreen(window) {
                continue;
            }
            let Some(win_loc) = self.space.element_location(window) else {
                continue;
            };
            let geo = window.geometry();
            let x0 = win_loc.x + geo.loc.x;
            let y0 = win_loc.y + geo.loc.y - TITLEBAR_HEIGHT;
            let w = geo.size.w.max(120);
            let h = geo.size.h + TITLEBAR_HEIGHT;
            let x1 = x0 + w;
            let y1 = y0 + h;

            if px < x0 - border || px > x1 + border || py < y0 - border || py > y1 + border {
                continue;
            }

            let left = (px - x0).abs() <= border;
            let right = (px - x1).abs() <= border;
            let bottom = (py - y1).abs() <= border;
            let top = (py - y0).abs() <= border;

            // Only match outer rim (not interior titlebar traffic lights or client surface)
            if left || right || bottom {
                return Some((
                    window.clone(),
                    ResizeEdges {
                        left,
                        right,
                        top: top && (left || right),
                        bottom,
                    },
                ));
            }

            // If point is inside a higher window's body, stop searching lower windows
            if px >= x0 && px < x1 && py >= y0 && py < y1 {
                return None;
            }
        }
        None
    }

    /// Snaps `window` to screen halves, quarters, or full maximized work area.
    pub fn snap_window(&mut self, window: &Window, zone: SnapZone) {
        if zone == SnapZone::Maximize {
            self.toggle_maximize_window(window);
            return;
        }
        let Some(toplevel) = window.toplevel().cloned() else {
            return;
        };
        let id = toplevel.wl_surface().id();
        self.workspaces.maximized.remove(&id);
        let output_geo = self
            .output_under_pointer()
            .and_then(|o| self.space.output_geometry(&o))
            .unwrap_or_else(|| Rectangle::from_size((1280, 720).into()));

        let rect = self.compute_snap_rect(output_geo, zone);
        if let Some(old_loc) = self.space.element_location(window) {
            self.animations.on_window_moved(id, old_loc, rect.loc);
        }
        self.space.map_element(window.clone(), rect.loc, true);
        toplevel.with_pending_state(|s| {
            s.states.unset(xdg_toplevel::State::Maximized);
            s.size = Some(rect.size);
        });
        toplevel.send_configure();
    }

    /// Cycles layout mode (`Floating -> MasterStack -> Dwindle -> Floating`) (`Super + T`).
    pub fn toggle_tiling(&mut self) {
        let next = self.workspaces.active_layout().next();
        self.workspaces.set_active_layout(next);
        tracing::info!(mode = ?next, "Cycled workspace layout mode");
        self.overlays
            .show_osd("Layout", next.short_label(), None);
        self.retile_workspace();
    }

    /// Returns visible non-fullscreen windows on the active workspace in stable tiling order.
    pub fn ordered_visible_windows(&self) -> Vec<Window> {
        let mut wins: Vec<Window> = self
            .space
            .elements()
            .filter(|w| self.workspaces.is_visible(w) && !self.workspaces.is_fullscreen(w))
            .cloned()
            .collect();
        wins.sort_by_key(|w| {
            w.toplevel()
                .and_then(|t| {
                    let id = t.wl_surface().id();
                    self.workspaces.tile_order.iter().position(|x| x == &id)
                })
                .unwrap_or(usize::MAX)
        });
        wins
    }

    /// Arranges visible non-fullscreen windows on the active workspace in Master-Stack or Dwindle
    /// spiral layout when tiling is active.
    pub fn retile_workspace(&mut self) {
        if !self.workspaces.tiling_enabled() {
            return;
        }
        let Some(output) = self
            .output_under_pointer()
            .or_else(|| self.space.outputs().next().cloned())
        else {
            return;
        };
        let Some(output_geo) = self.space.output_geometry(&output) else {
            return;
        };

        let windows = self.ordered_visible_windows();
        let n = windows.len();
        if n == 0 {
            return;
        }

        let gap = self.panel.window_gaps;
        let top_bar_pad = if self.panel.dock_mode || self.panel.panel_top {
            crate::panel::PANEL_HEIGHT
        } else {
            0
        };
        let bot_bar_pad = if self.panel.dock_mode {
            crate::panel::DOCK_HEIGHT + crate::panel::DOCK_MARGIN + 4
        } else if !self.panel.panel_top {
            36
        } else {
            0
        };
        let area_x = output_geo.loc.x + gap;
        let area_y = output_geo.loc.y + top_bar_pad + gap + TITLEBAR_HEIGHT;
        let area_w = (output_geo.size.w - gap * 2).max(200);
        let area_h =
            (output_geo.size.h - top_bar_pad - bot_bar_pad - gap * 2 - TITLEBAR_HEIGHT).max(200);

        if n == 1 {
            let win = &windows[0];
            if let Some(old_loc) = self.space.element_location(win) {
                if let Some(t) = win.toplevel() {
                    self.animations.on_window_moved(
                        t.wl_surface().id(),
                        old_loc,
                        Point::from((area_x, area_y)),
                    );
                }
            }
            self.space.map_element(win.clone(), (area_x, area_y), false);
            if let Some(toplevel) = win.toplevel() {
                toplevel.with_pending_state(|s| {
                    s.size = Some((area_w, area_h).into());
                });
                toplevel.send_configure();
            }
            return;
        }

        if self.workspaces.active_layout() == LayoutMode::Dwindle {
            let mut rx = area_x;
            let mut ry = area_y;
            let mut rw = area_w;
            let mut rh = area_h;

            for (i, win) in windows.iter().enumerate() {
                let is_last = i + 1 == n;
                let (wx, wy, ww, wh) = if is_last {
                    (rx, ry, rw, rh)
                } else if i % 2 == 0 {
                    let ratio = if i == 0 {
                        self.workspaces.master_ratio
                    } else {
                        0.5
                    };
                    let w_half = ((rw - gap) as f32 * ratio).round() as i32;
                    let res = (rx, ry, w_half.max(120), rh);
                    rx += w_half + gap;
                    rw = (rw - w_half - gap).max(120);
                    res
                } else {
                    let h_half = ((rh - gap - TITLEBAR_HEIGHT).max(80)) / 2;
                    let res = (rx, ry, rw, h_half);
                    ry += h_half + gap + TITLEBAR_HEIGHT;
                    rh = (rh - h_half - gap - TITLEBAR_HEIGHT).max(80);
                    res
                };

                if let Some(old_loc) = self.space.element_location(win) {
                    if let Some(t) = win.toplevel() {
                        self.animations.on_window_moved(
                            t.wl_surface().id(),
                            old_loc,
                            Point::from((wx, wy)),
                        );
                    }
                }
                self.space.map_element(win.clone(), (wx, wy), false);
                if let Some(toplevel) = win.toplevel() {
                    toplevel.with_pending_state(|s| {
                        s.size = Some((ww.max(100), wh.max(80)).into());
                    });
                    toplevel.send_configure();
                }
            }
            return;
        }

        // Master-Stack layout with adjustable master_ratio
        let master_w = (((area_w - gap) as f32) * self.workspaces.master_ratio)
            .round()
            .clamp(160.0, (area_w - gap - 160).max(160) as f32) as i32;
        let stack_w = (area_w - master_w - gap).max(120);
        let stack_count = (n - 1) as i32;

        for (i, win) in windows.iter().enumerate() {
            let (wx, wy, ww, wh) = if i == 0 {
                (area_x, area_y, master_w, area_h)
            } else {
                let idx = (i - 1) as i32;
                let cell_h = (area_h + gap) / stack_count;
                let ch = (cell_h - gap - TITLEBAR_HEIGHT).max(80);
                let actual_y = area_y + idx * (ch + TITLEBAR_HEIGHT + gap);
                (area_x + master_w + gap, actual_y, stack_w, ch)
            };

            if let Some(old_loc) = self.space.element_location(win) {
                if let Some(t) = win.toplevel() {
                    self.animations.on_window_moved(
                        t.wl_surface().id(),
                        old_loc,
                        Point::from((wx, wy)),
                    );
                }
            }
            self.space.map_element(win.clone(), (wx, wy), false);
            if let Some(toplevel) = win.toplevel() {
                toplevel.with_pending_state(|s| {
                    s.size = Some((ww.max(100), wh.max(80)).into());
                });
                toplevel.send_configure();
            }
        }
    }

    /// Focuses the next (`forward = true`) or previous (`forward = false`) visible window (`Super+J/K`).
    pub fn focus_next_prev(&mut self, forward: bool) {
        let windows = self.ordered_visible_windows();
        if windows.is_empty() {
            return;
        }
        let focused_id = self
            .seat
            .get_keyboard()
            .and_then(|k| k.current_focus())
            .map(|s| s.id());
        let cur = focused_id
            .and_then(|fid| {
                windows.iter().position(|w| {
                    w.toplevel()
                        .map(|t| t.wl_surface().id() == fid)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(0);
        let next_idx = if forward {
            (cur + 1) % windows.len()
        } else {
            (cur + windows.len() - 1) % windows.len()
        };
        self.focus_window(&windows[next_idx]);
    }

    /// Swaps the focused window in the workspace tiling order (`Super+Shift+J/K`).
    pub fn swap_tiled_window(&mut self, forward: bool) {
        let windows = self.ordered_visible_windows();
        if windows.len() < 2 {
            return;
        }
        let Some(focused) = self.focused_window() else {
            return;
        };
        let Some(fid) = focused.toplevel().map(|t| t.wl_surface().id()) else {
            return;
        };
        let Some(cur_vis) = windows.iter().position(|w| {
            w.toplevel()
                .map(|t| t.wl_surface().id() == fid)
                .unwrap_or(false)
        }) else {
            return;
        };
        let other_vis = if forward {
            (cur_vis + 1) % windows.len()
        } else {
            (cur_vis + windows.len() - 1) % windows.len()
        };
        let Some(oid) = windows[other_vis].toplevel().map(|t| t.wl_surface().id()) else {
            return;
        };
        if let (Some(a), Some(b)) = (
            self.workspaces.tile_order.iter().position(|x| x == &fid),
            self.workspaces.tile_order.iter().position(|x| x == &oid),
        ) {
            self.workspaces.tile_order.swap(a, b);
            self.retile_workspace();
        }
    }

    /// Adjusts the Master-Stack split ratio (`Super + [` / `Super + ]`) and retiles.
    pub fn adjust_master_ratio(&mut self, delta: f32) {
        self.workspaces.master_ratio = (self.workspaces.master_ratio + delta).clamp(0.25, 0.75);
        let pct = (self.workspaces.master_ratio * 100.0).round() as i32;
        self.overlays
            .show_osd("Master Ratio", &format!("{}%", pct), Some(pct));
        self.retile_workspace();
    }

    /// Minimizes the currently focused window (`Super + M`).
    pub fn minimize_focused_window(&mut self) {
        if let Some(win) = self.focused_window() {
            self.toggle_minimize_window(&win);
        }
    }

    /// Restores the most recently minimized window on the active workspace (`Super + Shift + M`).
    pub fn restore_last_minimized(&mut self) {
        let target = self
            .space
            .elements()
            .rev()
            .find(|w| {
                self.workspaces.is_on_active_workspace(w) && self.workspaces.is_minimized(w)
            })
            .cloned();
        if let Some(win) = target {
            self.toggle_minimize_window(&win);
        }
    }

    /// Centers the currently focused floating window inside the usable work area (`Super + C`).
    pub fn center_focused_window(&mut self) {
        let Some(win) = self.focused_window() else {
            return;
        };
        let Some(toplevel) = win.toplevel() else {
            return;
        };
        let output_geo = self
            .output_under_pointer()
            .and_then(|o| self.space.output_geometry(&o))
            .unwrap_or_else(|| Rectangle::from_size((1280, 720).into()));
        let top_pad = if self.panel.dock_mode || self.panel.panel_top {
            crate::panel::PANEL_HEIGHT
        } else {
            0
        };
        let bot_pad = crate::panel::DOCK_HEIGHT + crate::panel::DOCK_MARGIN + 4;
        let win_geo = win.geometry();
        let usable_h = (output_geo.size.h - top_pad - bot_pad).max(160);
        let nx = output_geo.loc.x + (output_geo.size.w - win_geo.size.w) / 2;
        let ny = (output_geo.loc.y + top_pad + TITLEBAR_HEIGHT + (usable_h - win_geo.size.h - TITLEBAR_HEIGHT) / 2)
            .max(output_geo.loc.y + top_pad + TITLEBAR_HEIGHT);
        let id = toplevel.wl_surface().id();
        self.workspaces.maximized.remove(&id);
        if let Some(old_loc) = self.space.element_location(&win) {
            self.animations
                .on_window_moved(id, old_loc, Point::from((nx, ny)));
        }
        self.space.map_element(win, (nx, ny), true);
    }

    /// Returns the topmost visible `Window` under `pos` (including its server-side titlebar).
    pub fn window_under(&self, pos: Point<f64, Logical>) -> Option<Window> {
        let px = pos.x.round() as i32;
        let py = pos.y.round() as i32;

        for window in self.space.elements().rev() {
            if !self.workspaces.is_visible(window) {
                continue;
            }
            if let Some(win_loc) = self.space.element_location(window) {
                let geo = window.geometry();
                let x0 = win_loc.x + geo.loc.x;
                let y0 = win_loc.y + geo.loc.y - TITLEBAR_HEIGHT;
                let w = geo.size.w.max(120);
                let h = geo.size.h + TITLEBAR_HEIGHT;
                if px >= x0 && px < x0 + w && py >= y0 && py < y0 + h {
                    return Some(window.clone());
                }
            }
        }
        None
    }

    /// Checks if `pos` clicked a window's server-side titlebar or its `[-]`, `[+]`, `[X]` buttons.
    /// Returns `true` if handled (either clicked a button or started a titlebar move drag).
    pub fn handle_titlebar_click(&mut self, pos: Point<f64, Logical>) -> bool {
        let px = pos.x.round() as i32;
        let py = pos.y.round() as i32;

        let mut hit: Option<(Window, Point<i32, Logical>, Rectangle<i32, Logical>)> = None;
        for window in self.space.elements().rev() {
            if !self.workspaces.is_visible(window) || self.workspaces.is_fullscreen(window) {
                continue;
            }
            let Some(win_loc) = self.space.element_location(window) else {
                continue;
            };
            let geo = window.geometry();
            let tb_x = win_loc.x + geo.loc.x;
            let tb_y = win_loc.y + geo.loc.y - TITLEBAR_HEIGHT;
            let tb_w = geo.size.w.max(120);

            // If point is inside the window surface below the titlebar, a higher window covers this point
            if px >= tb_x && px < tb_x + tb_w && py >= win_loc.y + geo.loc.y && py < win_loc.y + geo.loc.y + geo.size.h {
                return false;
            }

            if px >= tb_x && px < tb_x + tb_w && py >= tb_y && py < tb_y + TITLEBAR_HEIGHT {
                hit = Some((window.clone(), win_loc, Rectangle::new((tb_x, tb_y).into(), (tb_w, TITLEBAR_HEIGHT).into())));
                break;
            }
        }

        let Some((window, win_loc, tb_rect)) = hit else {
            return false;
        };

        self.focus_window(&window);
        let rel_x = px - tb_rect.loc.x;

        // Top-left macOS Traffic Light buttons: Close (#ff5f57), Minimize (#febc2e), Maximize (#28c840)
        if (8..=24).contains(&rel_x) {
            if let Some(toplevel) = window.toplevel() {
                toplevel.send_close();
            }
            return true;
        }
        if (28..=44).contains(&rel_x) {
            self.toggle_minimize_window(&window);
            return true;
        }
        if (48..=64).contains(&rel_x) {
            self.toggle_maximize_window(&window);
            return true;
        }

        // Double-click titlebar -> Toggle Maximize / Restore
        if let Some(win_id) = window.toplevel().map(|t| t.wl_surface().id()) {
            let now = Instant::now();
            let is_double = self
                .last_titlebar_click
                .as_ref()
                .map(|(lid, lt)| *lid == win_id && now.duration_since(*lt).as_millis() <= 380)
                .unwrap_or(false);
            if is_double {
                self.last_titlebar_click = None;
                self.toggle_maximize_window(&window);
                return true;
            }
            self.last_titlebar_click = Some((win_id, now));
        }

        self.unmaximize_if_needed(&window);

        // Dragging the titlebar initiates a window move grab
        self.pointer_grab = Some(PointerGrabMode::Move {
            window,
            initial_pointer: pos,
            initial_window_loc: win_loc,
        });
        true
    }

    /// Handles middle-click on a window titlebar (toggles Always-on-Top) or right-click (Window Menu).
    pub fn handle_titlebar_alt_click(&mut self, pos: Point<f64, Logical>, is_right_click: bool) -> bool {
        let px = pos.x.round() as i32;
        let py = pos.y.round() as i32;

        for window in self.space.elements().rev() {
            if !self.workspaces.is_visible(window) || self.workspaces.is_fullscreen(window) {
                continue;
            }
            let Some(win_loc) = self.space.element_location(window) else {
                continue;
            };
            let geo = window.geometry();
            let tb_x = win_loc.x + geo.loc.x;
            let tb_y = win_loc.y + geo.loc.y - TITLEBAR_HEIGHT;
            let tb_w = geo.size.w.max(120);

            if px >= tb_x && px < tb_x + tb_w && py >= win_loc.y + geo.loc.y && py < win_loc.y + geo.loc.y + geo.size.h {
                return false;
            }

            if px >= tb_x && px < tb_x + tb_w && py >= tb_y && py < tb_y + TITLEBAR_HEIGHT {
                let win_clone = window.clone();
                self.focus_window(&win_clone);
                if let Some(id) = win_clone.toplevel().map(|t| t.wl_surface().id()) {
                    if is_right_click {
                        self.overlays.window_context_menu = Some((Point::from((px, py)), id));
                    } else {
                        if !self.workspaces.always_on_top.remove(&id) {
                            self.workspaces.always_on_top.insert(id);
                            self.space.raise_element(&win_clone, true);
                        }
                        let pinned = self.workspaces.is_pinned(&win_clone);
                        self.overlays.show_osd(
                            "Always on Top",
                            if pinned { "Pinned" } else { "Unpinned" },
                            None,
                        );
                    }
                    return true;
                }
            }
        }
        false
    }

    /// Cycles focus through visible windows on the active workspace in MRU / stack order.
    pub fn cycle_focus(&mut self) {
        self.cycle_focus_dir(true);
    }

    /// Cycles focus forward (`forward = true`) or backward (`forward = false`) cleanly across all visible windows.
    pub fn cycle_focus_dir(&mut self, forward: bool) {
        let mut visible: Vec<Window> = self.ordered_visible_windows();
        if visible.is_empty() {
            return;
        }
        if visible.len() == 1 {
            self.focus_window(&visible[0]);
            return;
        }
        visible.sort_by_key(|w| {
            w.toplevel()
                .map(|t| t.wl_surface().id().protocol_id())
                .unwrap_or(0)
        });
        let focused_id = self
            .seat
            .get_keyboard()
            .and_then(|k| k.current_focus())
            .map(|s| s.id());
        let cur = focused_id
            .and_then(|fid| {
                visible.iter().position(|w| {
                    w.toplevel()
                        .map(|t| t.wl_surface().id() == fid)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(0);
        let target_idx = if forward {
            (cur + 1) % visible.len()
        } else {
            (cur + visible.len() - 1) % visible.len()
        };
        self.focus_window(&visible[target_idx]);
    }

    /// Finds the surface and its global origin under the given logical pointer coordinates.
    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        // 1. Check overlay / top layer-shell surfaces first
        for output in self.space.outputs() {
            let output_geo = self.space.output_geometry(output)?;
            if !output_geo.to_f64().contains(pos) {
                continue;
            }
            let local_pos = pos - output_geo.loc.to_f64();
            let layer_map = layer_map_for_output(output);

            for layer_type in [Layer::Overlay, Layer::Top] {
                for layer in layer_map.layers_on(layer_type).rev() {
                    if let Some(geo) = layer_map.layer_geometry(layer) {
                        if let Some((surf, surf_loc)) = layer
                            .surface_under(local_pos - geo.loc.to_f64(), WindowSurfaceType::ALL)
                        {
                            return Some((
                                surf,
                                output_geo.loc.to_f64() + geo.loc.to_f64() + surf_loc.to_f64(),
                            ));
                        }
                    }
                }
            }
        }

        // 2. Check visible XDG windows in top-to-bottom order
        for window in self.space.elements().rev() {
            if !self.workspaces.is_visible(window) {
                continue;
            }
            if let Some(win_loc) = self.space.element_location(window) {
                if let Some((surf, surf_loc)) =
                    window.surface_under(pos - win_loc.to_f64(), WindowSurfaceType::ALL)
                {
                    return Some((surf, (win_loc + surf_loc).to_f64()));
                }
            }
        }

        // 3. Check bottom / background layer-shell surfaces
        for output in self.space.outputs() {
            let output_geo = self.space.output_geometry(output)?;
            if !output_geo.to_f64().contains(pos) {
                continue;
            }
            let local_pos = pos - output_geo.loc.to_f64();
            let layer_map = layer_map_for_output(output);

            for layer_type in [Layer::Bottom, Layer::Background] {
                for layer in layer_map.layers_on(layer_type).rev() {
                    if let Some(geo) = layer_map.layer_geometry(layer) {
                        if let Some((surf, surf_loc)) = layer
                            .surface_under(local_pos - geo.loc.to_f64(), WindowSurfaceType::ALL)
                        {
                            return Some((
                                surf,
                                output_geo.loc.to_f64() + geo.loc.to_f64() + surf_loc.to_f64(),
                            ));
                        }
                    }
                }
            }
        }

        None
    }

    /// Returns the output currently containing the pointer, falling back to the primary output.
    pub fn output_under_pointer(&self) -> Option<smithay::output::Output> {
        let pos = self.pointer_location;
        self.space
            .outputs()
            .find(|o| {
                self.space
                    .output_geometry(o)
                    .map(|g| g.to_f64().contains(pos))
                    .unwrap_or(false)
            })
            .cloned()
            .or_else(|| self.space.outputs().next().cloned())
    }

    /// Computes a clean initial placement inside the active output for a new window,
    /// avoiding the native bottom taskbar.
    pub fn next_window_position(&mut self, win_size: smithay::utils::Size<i32, Logical>) -> Point<i32, Logical> {
        let output_geo = self
            .output_under_pointer()
            .and_then(|o| self.space.output_geometry(&o))
            .unwrap_or_else(|| Rectangle::from_size((1280, 720).into()));

        let usable_h = (output_geo.size.h - crate::panel::PANEL_HEIGHT).max(200);
        let offset = self.next_spawn_offset;
        self.next_spawn_offset = (self.next_spawn_offset + 32) % 192;

        let x = output_geo.loc.x + ((output_geo.size.w - win_size.w) / 2).max(24) + offset - 64;
        let y = output_geo.loc.y + ((usable_h - win_size.h) / 2).max(24 + TITLEBAR_HEIGHT) + offset - 64;
        let top_bar_h = if self.panel.dock_mode || self.panel.panel_top {
            crate::panel::PANEL_HEIGHT
        } else {
            0
        };
        let min_y = output_geo.loc.y + top_bar_h + 8 + TITLEBAR_HEIGHT;
        let max_y = (output_geo.loc.y + usable_h - win_size.h - 8).max(min_y);

        Point::from((
            x.max(output_geo.loc.x + 16),
            y.clamp(min_y, max_y),
        ))
    }
}
