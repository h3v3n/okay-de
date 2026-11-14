use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_data_device, delegate_layer_shell, delegate_output,
    delegate_seat, delegate_shm, delegate_xdg_decoration, delegate_xdg_shell,
    desktop::{
        layer_map_for_output, LayerSurface, PopupKind, Window,
    },
    input::{
        pointer::CursorImageStatus,
        Seat, SeatHandler, SeatState,
    },
    output::Output,
    reexports::{
        wayland_protocols::xdg::{
            decoration::zv1::server::zxdg_toplevel_decoration_v1,
            shell::server::xdg_toplevel,
        },
        wayland_server::{
            protocol::{
                wl_buffer::WlBuffer,
                wl_output::WlOutput,
                wl_seat::WlSeat,
                wl_surface::WlSurface,
            },
            Client, Resource,
        },
    },
    utils::{Rectangle, Serial},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            get_parent, is_sync_subsurface, with_states, CompositorClientState,
            CompositorHandler, CompositorState,
        },
        output::OutputHandler,
        selection::{
            data_device::{
                set_data_device_focus, ClientDndGrabHandler, DataDeviceHandler,
                DataDeviceState, ServerDndGrabHandler,
            },
            SelectionHandler,
        },
        shell::{
            wlr_layer::{
                Layer, LayerSurfaceData, WlrLayerShellHandler, WlrLayerShellState,
            },
            xdg::{
                decoration::XdgDecorationHandler,
                PopupSurface, PositionerState, ToplevelSurface,
                XdgShellHandler, XdgShellState, XdgToplevelSurfaceData,
            },
        },
        shm::{ShmHandler, ShmState},
    },
};

use crate::state::{ClientState, OkayState};

// ============================================================================
// Core Compositor & SHM Buffer Handling
// ============================================================================

impl BufferHandler for OkayState {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl CompositorHandler for OkayState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self
                .space
                .elements()
                .find(|w| w.toplevel().map(|t| t.wl_surface() == &root).unwrap_or(false))
                .cloned()
            {
                window.on_commit();
            }
        }

        self.popups.commit(surface);
        self.handle_xdg_commit(surface);
        self.handle_layer_commit(surface);
    }
}

impl ShmHandler for OkayState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

// ============================================================================
// XDG Shell (Toplevels & Popups) + XDG Decoration
// ============================================================================

impl XdgShellHandler for OkayState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface.clone());
        let id = surface.wl_surface().id();
        self.workspaces.assign_window(id, self.workspaces.active);

        let initial_pos = self.next_window_position((800, 600).into());
        self.space.map_element(window.clone(), initial_pos, true);
        self.focus_window(&window);

        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Activated);
            state.decoration_mode = Some(zxdg_toplevel_decoration_v1::Mode::ServerSide);
        });
        surface.send_configure();
        self.retile_workspace();
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        if let Err(err) = self.popups.track_popup(PopupKind::Xdg(surface)) {
            tracing::warn!(?err, "Failed to track xdg_popup");
        }
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: Serial) {}

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            let geometry = positioner.get_geometry();
            state.geometry = geometry;
            state.positioner = positioner;
        });
        surface.send_repositioned(token);
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let id = surface.wl_surface().id();
        self.workspaces.remove_window(&id);
        self.focus_stack.retain(|x| x != &id);

        let window = self
            .space
            .elements()
            .find(|w| w.toplevel().map(|t| t.wl_surface().id() == id).unwrap_or(false))
            .cloned();

        if let Some(window) = window {
            self.space.unmap_elem(&window);
        }

        self.retile_workspace();
        self.refocus_topmost_visible();
    }

    fn popup_destroyed(&mut self, _surface: PopupSurface) {
        self.popups.cleanup();
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        let output = self.space.outputs().next().cloned();
        if let Some(output) = output {
            let layer_map = layer_map_for_output(&output);
            let zone = layer_map.non_exclusive_zone();
            let output_geo = self
                .space
                .output_geometry(&output)
                .unwrap_or_else(|| Rectangle::from_size((1280, 720).into()));

            let target_loc = output_geo.loc + zone.loc;
            let target_size = zone.size;

            let window = self
                .space
                .elements()
                .find(|w| {
                    w.toplevel()
                        .map(|t| t.wl_surface() == surface.wl_surface())
                        .unwrap_or(false)
                })
                .cloned();

            if let Some(window) = window {
                self.space.map_element(window, target_loc, true);
            }

            surface.with_pending_state(|state| {
                state.states.set(xdg_toplevel::State::Maximized);
                state.size = Some(target_size);
            });
            surface.send_configure();
        }
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        surface.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Maximized);
            state.size = None;
        });
        surface.send_configure();
    }

    fn fullscreen_request(&mut self, surface: ToplevelSurface, output: Option<WlOutput>) {
        let target_output = output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| self.space.outputs().next().cloned());

        if let Some(out) = target_output {
            if let Some(geo) = self.space.output_geometry(&out) {
                let window = self
                    .space
                    .elements()
                    .find(|w| {
                        w.toplevel()
                            .map(|t| t.wl_surface() == surface.wl_surface())
                            .unwrap_or(false)
                    })
                    .cloned();

                if let Some(window) = window {
                    self.space.map_element(window, geo.loc, true);
                }
                surface.with_pending_state(|state| {
                    state.states.set(xdg_toplevel::State::Fullscreen);
                    state.size = Some(geo.size);
                });
                surface.send_configure();
            }
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        surface.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Fullscreen);
            state.size = None;
        });
        surface.send_configure();
    }
}

impl XdgDecorationHandler for OkayState {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(zxdg_toplevel_decoration_v1::Mode::ServerSide);
        });
        toplevel.send_configure();
    }

    fn request_mode(
        &mut self,
        toplevel: ToplevelSurface,
        _mode: zxdg_toplevel_decoration_v1::Mode,
    ) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(zxdg_toplevel_decoration_v1::Mode::ServerSide);
        });
        toplevel.send_configure();
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(zxdg_toplevel_decoration_v1::Mode::ServerSide);
        });
        toplevel.send_configure();
    }
}

impl OkayState {
    fn handle_xdg_commit(&mut self, surface: &WlSurface) {
        // Send initial configure if a newly created xdg_toplevel hasn't been configured yet.
        let is_toplevel = self.space.elements().any(|w| {
            w.toplevel()
                .map(|t| t.wl_surface() == surface)
                .unwrap_or(false)
        });

        if is_toplevel {
            let initial_configure_sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .map(|d| d.lock().unwrap().initial_configure_sent)
                    .unwrap_or(true)
            });

            if !initial_configure_sent {
                if let Some(win) = self
                    .space
                    .elements()
                    .find(|w| {
                        w.toplevel()
                            .map(|t| t.wl_surface() == surface)
                            .unwrap_or(false)
                    })
                    .cloned()
                {
                    if let Some(toplevel) = win.toplevel() {
                        toplevel.send_configure();
                    }
                }
            }
        }

        // Handle popup initial configure
        if let Some(PopupKind::Xdg(popup)) = self.popups.find_popup(surface) {
            let initial_configure_sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<smithay::wayland::shell::xdg::XdgPopupSurfaceData>()
                    .map(|d| d.lock().unwrap().initial_configure_sent)
                    .unwrap_or(true)
            });
            if !initial_configure_sent {
                if let Err(err) = popup.send_configure() {
                    tracing::warn!(?err, "Failed to send initial configure for xdg_popup");
                }
            }
        }
    }

    fn handle_layer_commit(&mut self, surface: &WlSurface) {
        let mut target_output: Option<Output> = None;
        for output in self.space.outputs() {
            let map = layer_map_for_output(output);
            if map.layers().any(|l| l.wl_surface() == surface) {
                target_output = Some(output.clone());
                break;
            }
        }

        if let Some(output) = target_output {
            let initial_configure_sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<LayerSurfaceData>()
                    .map(|d| d.lock().unwrap().initial_configure_sent)
                    .unwrap_or(true)
            });

            let mut map = layer_map_for_output(&output);
            map.arrange();

            if !initial_configure_sent {
                if let Some(layer) = map.layers().find(|l| l.wl_surface() == surface).cloned() {
                    layer.layer_surface().send_configure();
                }
            }
        }
    }
}

// ============================================================================
// wlr-layer-shell (Bars, Docks, Wallpapers, Launchers)
// ============================================================================

impl WlrLayerShellHandler for OkayState {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: smithay::wayland::shell::wlr_layer::LayerSurface,
        wl_output: Option<WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        let output = wl_output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| self.space.outputs().next().cloned());

        let Some(output) = output else {
            tracing::warn!(%namespace, "No output available for new layer surface");
            return;
        };

        let layer_surface = LayerSurface::new(surface, namespace);
        let mut map = layer_map_for_output(&output);
        if let Err(err) = map.map_layer(&layer_surface) {
            tracing::warn!(?err, "Failed to map layer surface");
            return;
        }
        map.arrange();
    }

    fn new_popup(&mut self, _parent: smithay::wayland::shell::wlr_layer::LayerSurface, popup: PopupSurface) {
        if let Err(err) = self.popups.track_popup(PopupKind::Xdg(popup)) {
            tracing::warn!(?err, "Failed to track layer popup");
        }
    }

    fn layer_destroyed(&mut self, surface: smithay::wayland::shell::wlr_layer::LayerSurface) {
        for output in self.space.outputs() {
            let mut map = layer_map_for_output(output);
            let found = map
                .layers()
                .find(|l| l.layer_surface() == &surface)
                .cloned();
            if let Some(layer) = found {
                map.unmap_layer(&layer);
                map.arrange();
                break;
            }
        }
    }
}

// ============================================================================
// Seat, Output & Data Device (Clipboard / DnD)
// ============================================================================

impl SeatHandler for OkayState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = &self.display_handle;
        let client = focused.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, client);
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
    }
}

impl OutputHandler for OkayState {}

impl SelectionHandler for OkayState {
    type SelectionUserData = ();
}

impl DataDeviceHandler for OkayState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for OkayState {}
impl ServerDndGrabHandler for OkayState {}

delegate_compositor!(OkayState);
delegate_shm!(OkayState);
delegate_xdg_shell!(OkayState);
delegate_xdg_decoration!(OkayState);
delegate_layer_shell!(OkayState);
delegate_seat!(OkayState);
delegate_output!(OkayState);
delegate_data_device!(OkayState);
