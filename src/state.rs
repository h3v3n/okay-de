use std::{
    collections::HashMap,
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
    reexports::wayland_server::{
        backend::{ClientData, ClientId, DisconnectReason, ObjectId},
        protocol::wl_surface::WlSurface,
        Display, DisplayHandle, Resource,
    },
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER},
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

/// Manages virtual workspaces (1..=9) and per-window workspace assignments.
#[derive(Debug)]
pub struct WorkspaceManager {
    pub active: usize,
    pub count: usize,
    pub window_map: HashMap<ObjectId, usize>,
}

impl Default for WorkspaceManager {
    fn default() -> Self {
        Self {
            active: 1,
            count: 9,
            window_map: HashMap::new(),
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
    }

    pub fn window_workspace(&self, surface_id: &ObjectId) -> usize {
        self.window_map.get(surface_id).copied().unwrap_or(self.active)
    }

    pub fn is_visible(&self, window: &Window) -> bool {
        if let Some(toplevel) = window.toplevel() {
            let id = toplevel.wl_surface().id();
            return self.window_workspace(&id) == self.active;
        }
        true
    }
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

    // Desktop, window registry & workspaces
    pub space: Space<Window>,
    pub popups: PopupManager,
    pub workspaces: WorkspaceManager,
    pub focus_stack: Vec<ObjectId>,
    pub next_spawn_offset: i32,

    // Visual theme & future shader/animation hooks
    pub theme: CompositorTheme,
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
            space: Space::default(),
            popups: PopupManager::default(),
            workspaces: WorkspaceManager::default(),
            focus_stack: Vec::new(),
            next_spawn_offset: 0,
            theme: CompositorTheme::default(),
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

    /// Computes a clean initial placement inside the primary output for a new window.
    pub fn next_window_position(&mut self, win_size: smithay::utils::Size<i32, Logical>) -> Point<i32, Logical> {
        let output_geo = self
            .space
            .outputs()
            .next()
            .and_then(|o| self.space.output_geometry(o))
            .unwrap_or_else(|| Rectangle::from_size((1280, 720).into()));

        let offset = self.next_spawn_offset;
        self.next_spawn_offset = (self.next_spawn_offset + 32) % 192;

        let x = output_geo.loc.x + ((output_geo.size.w - win_size.w) / 2).max(24) + offset - 64;
        let y = output_geo.loc.y + ((output_geo.size.h - win_size.h) / 2).max(40) + offset - 64;

        Point::from((
            x.max(output_geo.loc.x + 16),
            y.max(output_geo.loc.y + 16),
        ))
    }
}
