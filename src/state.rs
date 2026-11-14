use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
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
        selection::data_device::DataDeviceState,
        shell::{
            wlr_layer::{Layer, WlrLayerShellState},
            xdg::{decoration::XdgDecorationState, XdgShellState},
        },
        shm::ShmState,
    },
};

pub const TITLEBAR_HEIGHT: i32 = 26;

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

/// Manages virtual workspaces (1..=9), per-window states, and tiling/floating layout.
#[derive(Debug)]
pub struct WorkspaceManager {
    pub active: usize,
    pub count: usize,
    pub window_map: HashMap<ObjectId, usize>,
    pub minimized: HashSet<ObjectId>,
    pub fullscreen: HashSet<ObjectId>,
    pub maximized: HashSet<ObjectId>,
    pub saved_geometries: HashMap<ObjectId, Rectangle<i32, Logical>>,
    pub tiling_enabled: bool,
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
            saved_geometries: HashMap::new(),
            tiling_enabled: false,
        }
    }
}

impl WorkspaceManager {
    pub fn assign_window(&mut self, surface_id: ObjectId, workspace: usize) {
        let ws = workspace.clamp(1, self.count);
        self.window_map.insert(surface_id, ws);
    }

    pub fn remove_window(&mut self, surface_id: &ObjectId) {
        self.window_map.remove(surface_id);
        self.minimized.remove(surface_id);
        self.fullscreen.remove(surface_id);
        self.maximized.remove(surface_id);
        self.saved_geometries.remove(surface_id);
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

    pub fn is_visible(&self, window: &Window) -> bool {
        self.is_on_active_workspace(window) && !self.is_minimized(window)
    }
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
        initial_size: Size<i32, Logical>,
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

    // Seat & input tracking
    pub seat: Seat<Self>,
    pub pointer_location: Point<f64, Logical>,
    pub cursor_status: CursorImageStatus,
    pub pointer_grab: Option<PointerGrabMode>,
    pub super_pressed: bool,

    // Desktop, window registry & workspaces
    pub space: Space<Window>,
    pub popups: PopupManager,
    pub workspaces: WorkspaceManager,
    pub focus_stack: Vec<ObjectId>,
    pub next_spawn_offset: i32,

    // Visual theme & future shader/animation hooks
    pub theme: CompositorTheme,

    // Native desktop wallpaper & taskbar/launcher panel
    pub desktop: crate::desktop::DesktopState,
    pub panel: crate::panel::PanelState,
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

        let mut seat = seat_state.new_wl_seat(&display_handle, "seat-0");
        if let Err(err) = seat.add_keyboard(XkbConfig::default(), 250, 35) {
            tracing::warn!(?err, "Failed to initialize XKB keyboard on seat-0");
        }
        seat.add_pointer();

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
            seat,
            pointer_location: Point::from((0.0, 0.0)),
            cursor_status: CursorImageStatus::default_named(),
            pointer_grab: None,
            super_pressed: false,
            space: Space::default(),
            popups: PopupManager::default(),
            workspaces: WorkspaceManager::default(),
            focus_stack: Vec::new(),
            next_spawn_offset: 0,
            theme: CompositorTheme::default(),
            desktop: crate::desktop::DesktopState::new(),
            panel: crate::panel::PanelState::new(),
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

    /// Switches the active workspace (1..=9) and updates keyboard focus.
    pub fn switch_workspace(&mut self, index: usize) {
        let target = index.clamp(1, self.workspaces.count);
        if self.workspaces.active == target {
            return;
        }
        self.workspaces.active = target;
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
            let target_loc = Point::from((output_geo.loc.x, output_geo.loc.y + TITLEBAR_HEIGHT));
            let target_size = Size::from((
                output_geo.size.w,
                (output_geo.size.h - crate::panel::PANEL_HEIGHT - TITLEBAR_HEIGHT).max(120),
            ));
            self.space.map_element(window.clone(), target_loc, true);
            toplevel.with_pending_state(|state| {
                state.states.set(xdg_toplevel::State::Maximized);
                state.size = Some(target_size);
            });
            toplevel.send_configure();
        }
    }

    /// Toggles minimized state for `window` (used by titlebar `[-]` and taskbar buttons).
    pub fn toggle_minimize_window(&mut self, window: &Window) {
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        let id = toplevel.wl_surface().id();
        if self.workspaces.minimized.remove(&id) {
            self.focus_window(window);
            self.retile_workspace();
        } else {
            self.workspaces.minimized.insert(id);
            self.retile_workspace();
            self.refocus_topmost_visible();
        }
    }

    /// Toggles automatic master-stack tiling vs floating placement (`Super + T`).
    pub fn toggle_tiling(&mut self) {
        self.workspaces.tiling_enabled = !self.workspaces.tiling_enabled;
        tracing::info!(tiling = self.workspaces.tiling_enabled, "Toggled workspace tiling mode");
        self.retile_workspace();
    }

    /// Arranges visible non-fullscreen windows on the active workspace in a master-stack layout
    /// when `tiling_enabled` is active.
    pub fn retile_workspace(&mut self) {
        if !self.workspaces.tiling_enabled {
            return;
        }
        let Some(output) = self.output_under_pointer() else {
            return;
        };
        let Some(output_geo) = self.space.output_geometry(&output) else {
            return;
        };

        let windows: Vec<Window> = self
            .space
            .elements()
            .filter(|w| self.workspaces.is_visible(w) && !self.workspaces.is_fullscreen(w))
            .cloned()
            .collect();

        let n = windows.len();
        if n == 0 {
            return;
        }

        let gap = 8;
        let area_x = output_geo.loc.x + gap;
        let area_y = output_geo.loc.y + gap + TITLEBAR_HEIGHT;
        let area_w = (output_geo.size.w - gap * 2).max(200);
        let area_h = (output_geo.size.h - crate::panel::PANEL_HEIGHT - gap * 2 - TITLEBAR_HEIGHT).max(200);

        if n == 1 {
            let win = &windows[0];
            self.space.map_element(win.clone(), (area_x, area_y), false);
            if let Some(toplevel) = win.toplevel() {
                toplevel.with_pending_state(|s| {
                    s.size = Some((area_w, area_h).into());
                });
                toplevel.send_configure();
            }
            return;
        }

        let master_w = (area_w - gap) / 2;
        let stack_w = area_w - master_w - gap;
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

            self.space.map_element(win.clone(), (wx, wy), false);
            if let Some(toplevel) = win.toplevel() {
                toplevel.with_pending_state(|s| {
                    s.size = Some((ww.max(100), wh.max(80)).into());
                });
                toplevel.send_configure();
            }
        }
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
        let w = tb_rect.size.w;

        // Close button [X]: [w - 28 .. w - 6]
        if rel_x >= w - 28 && rel_x < w - 6 {
            if let Some(toplevel) = window.toplevel() {
                toplevel.send_close();
            }
            return true;
        }
        // Maximize button [+]: [w - 54 .. w - 32]
        if rel_x >= w - 54 && rel_x < w - 32 {
            self.toggle_maximize_window(&window);
            return true;
        }
        // Minimize button [-]: [w - 80 .. w - 58]
        if rel_x >= w - 80 && rel_x < w - 58 {
            self.toggle_minimize_window(&window);
            return true;
        }

        // Dragging the titlebar initiates a window move grab
        self.pointer_grab = Some(PointerGrabMode::Move {
            window,
            initial_pointer: pos,
            initial_window_loc: win_loc,
        });
        true
    }

    /// Cycles focus through visible windows on the active workspace.
    pub fn cycle_focus(&mut self) {
        let visible: Vec<Window> = self
            .space
            .elements()
            .filter(|w| self.workspaces.is_visible(w))
            .cloned()
            .collect();

        if visible.len() < 2 {
            if let Some(first) = visible.first() {
                self.focus_window(first);
            }
            return;
        }

        // The bottom-most visible window is brought to the top and focused.
        if let Some(next) = visible.first() {
            self.focus_window(next);
        }
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
        let max_y = output_geo.loc.y + (usable_h - win_size.h - 8).max(16 + TITLEBAR_HEIGHT);

        Point::from((
            x.max(output_geo.loc.x + 16),
            y.clamp(output_geo.loc.y + 16 + TITLEBAR_HEIGHT, max_y),
        ))
    }
}
