use std::{io::Write, os::fd::OwnedFd};

use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_data_control, delegate_data_device, delegate_fractional_scale,
    delegate_layer_shell, delegate_output, delegate_seat, delegate_session_lock, delegate_shm,
    delegate_xdg_decoration, delegate_xdg_shell,
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
    utils::{IsAlive, Rectangle, Serial},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            get_parent, is_sync_subsurface, with_states, CompositorClientState,
            CompositorHandler, CompositorState,
        },
        fractional_scale::{self, FractionalScaleHandler},
        output::OutputHandler,
        selection::{
            data_device::{
                request_data_device_client_selection, set_data_device_focus,
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
            },
            wlr_data_control::{DataControlHandler, DataControlState},
            SelectionHandler, SelectionSource, SelectionTarget,
        },
        session_lock::{LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker},
        shell::{
            wlr_layer::{
                KeyboardInteractivity, Layer, LayerSurfaceData, WlrLayerShellHandler,
                WlrLayerShellState,
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
                self.blur_dirty = true;
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
        self.animations.on_window_opened(id.clone(), self.panel.accent_rgb());
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
        self.unstack_window(&id);
        self.workspaces.remove_window(&id);
        self.focus_stack.retain(|x| x != &id);
        self.animations.windows.remove(&id);
        self.animations.taskbar_button_rects.remove(&id);
        crate::render::evict_window_cache(&id);

        if let Some(ref grab) = self.pointer_grab {
            let grab_id = match grab {
                crate::state::PointerGrabMode::Move { window, .. }
                | crate::state::PointerGrabMode::Resize { window, .. } => {
                    window.toplevel().map(|t| t.wl_surface().id())
                }
            };
            if grab_id.as_ref() == Some(&id) {
                self.pointer_grab = None;
            }
        }
        if self
            .overlays
            .window_context_menu
            .as_ref()
            .map(|(_, mid)| mid == &id)
            .unwrap_or(false)
        {
            self.overlays.window_context_menu = None;
        }
        if self
            .last_titlebar_click
            .as_ref()
            .map(|(lid, _)| lid == &id)
            .unwrap_or(false)
        {
            self.last_titlebar_click = None;
        }

        let window = self
            .space
            .elements()
            .find(|w| w.toplevel().map(|t| t.wl_surface().id() == id).unwrap_or(false))
            .cloned();

        if let Some(window) = window {
            self.space.unmap_elem(&window);
        }

        if let Some(pointer) = self.seat.get_pointer() {
            if let Some(under) = pointer.current_focus() {
                if !under.alive() || under.id() == id {
                    pointer.motion(
                        self,
                        None,
                        &smithay::input::pointer::MotionEvent {
                            location: self.pointer_location,
                            serial: smithay::utils::SERIAL_COUNTER.next_serial(),
                            time: 0,
                        },
                    );
                }
            }
        }

        self.retile_workspace();
        self.refocus_topmost_visible();
    }

    fn popup_destroyed(&mut self, _surface: PopupSurface) {
        self.popups.cleanup();
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
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
            let id = surface.wl_surface().id();
            if !self.workspaces.maximized.contains(&id) {
                self.toggle_maximize_window(&window);
            } else {
                surface.send_configure();
            }
        }
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
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
            let id = surface.wl_surface().id();
            if self.workspaces.maximized.contains(&id) {
                self.toggle_maximize_window(&window);
                return;
            }
        }
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
                let id = surface.wl_surface().id();
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
                    if !self.workspaces.fullscreen.contains(&id) {
                        if let Some(loc) = self.space.element_location(&window) {
                            self.workspaces
                                .saved_geometries
                                .insert(id.clone(), Rectangle::new(loc, window.geometry().size));
                        }
                        self.workspaces.fullscreen.insert(id);
                    }
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
        let id = surface.wl_surface().id();
        let was_fs = self.workspaces.fullscreen.remove(&id);
        let restore = if was_fs {
            self.workspaces.saved_geometries.remove(&id)
        } else {
            None
        };
        let window = self
            .space
            .elements()
            .find(|w| {
                w.toplevel()
                    .map(|t| t.wl_surface() == surface.wl_surface())
                    .unwrap_or(false)
            })
            .cloned();

        if let (Some(win), Some(rect)) = (window, restore) {
            self.space.map_element(win, rect.loc, true);
            surface.with_pending_state(|state| {
                state.states.unset(xdg_toplevel::State::Fullscreen);
                state.size = Some(rect.size);
            });
        } else {
            surface.with_pending_state(|state| {
                state.states.unset(xdg_toplevel::State::Fullscreen);
                state.size = None;
            });
        }
        surface.send_configure();
        self.retile_workspace();
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
                    .map(|d| d.lock().map(|g| g.initial_configure_sent).unwrap_or(true))
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
                    .map(|d| d.lock().map(|g| g.initial_configure_sent).unwrap_or(true))
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
                    .map(|d| d.lock().map(|g| g.initial_configure_sent).unwrap_or(true))
                    .unwrap_or(true)
            });

            let mut keyboard_focus_surface: Option<WlSurface> = None;
            {
                let mut map = layer_map_for_output(&output);
                map.arrange();

                let found_layer = map.layers().find(|l| l.wl_surface() == surface).cloned();
                if let Some(layer) = found_layer {
                    if !initial_configure_sent {
                        layer.layer_surface().send_configure();
                    }
                    if layer.cached_state().keyboard_interactivity == KeyboardInteractivity::Exclusive {
                        keyboard_focus_surface = Some(layer.wl_surface().clone());
                    }
                }
            }
            if let Some(focus_surf) = keyboard_focus_surface {
                if let Some(kb) = self.seat.get_keyboard() {
                    kb.set_focus(
                        self,
                        Some(focus_surf),
                        smithay::utils::SERIAL_COUNTER.next_serial(),
                    );
                }
            }
            self.retile_workspace();
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
        let exclusive_kb = layer_surface.cached_state().keyboard_interactivity
            == KeyboardInteractivity::Exclusive;
        let focus_surf = layer_surface.wl_surface().clone();
        {
            let mut map = layer_map_for_output(&output);
            if let Err(err) = map.map_layer(&layer_surface) {
                tracing::warn!(?err, "Failed to map layer surface");
                return;
            }
            map.arrange();
        }
        if exclusive_kb {
            if let Some(kb) = self.seat.get_keyboard() {
                kb.set_focus(
                    self,
                    Some(focus_surf),
                    smithay::utils::SERIAL_COUNTER.next_serial(),
                );
            }
        }
        self.retile_workspace();
    }

    fn new_popup(&mut self, _parent: smithay::wayland::shell::wlr_layer::LayerSurface, popup: PopupSurface) {
        if let Err(err) = self.popups.track_popup(PopupKind::Xdg(popup)) {
            tracing::warn!(?err, "Failed to track layer popup");
        }
    }

    fn layer_destroyed(&mut self, surface: smithay::wayland::shell::wlr_layer::LayerSurface) {
        let mut destroyed_id = None;
        for output in self.space.outputs() {
            let mut map = layer_map_for_output(output);
            let found = map
                .layers()
                .find(|l| l.layer_surface() == &surface)
                .cloned();
            if let Some(layer) = found {
                destroyed_id = Some(layer.wl_surface().id());
                map.unmap_layer(&layer);
                map.arrange();
                break;
            }
        }
        self.retile_workspace();
        if let Some(did) = destroyed_id {
            let need_refocus = self
                .seat
                .get_keyboard()
                .and_then(|k| k.current_focus())
                .map(|s| s.id() == did)
                .unwrap_or(false);
            if need_refocus {
                self.refocus_topmost_visible();
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

    fn new_selection(
        &mut self,
        ty: SelectionTarget,
        source: Option<SelectionSource>,
        seat: Seat<Self>,
    ) {
        if ty == SelectionTarget::Clipboard && source.is_some() {
            if let Ok((read_fd, write_fd)) = smithay::reexports::rustix::pipe::pipe() {
                if request_data_device_client_selection(
                    &seat,
                    "text/plain;charset=utf-8".to_string(),
                    write_fd,
                )
                .is_ok()
                {
                    crate::worker::dispatch(crate::worker::WorkerRequest::ReadClipboardPipe {
                        fd: read_fd,
                    });
                }
            }
        }
    }

    fn send_selection(
        &mut self,
        ty: SelectionTarget,
        _mime_type: String,
        fd: OwnedFd,
        _seat: Seat<Self>,
        _user_data: &Self::SelectionUserData,
    ) {
        if ty == SelectionTarget::Clipboard {
            let mut file = std::fs::File::from(fd);
            let _ = file.write_all(self.active_clipboard_text.as_bytes());
        }
    }
}

impl DataDeviceHandler for OkayState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl DataControlHandler for OkayState {
    fn data_control_state(&self) -> &DataControlState {
        &self.data_control_state
    }
}

impl ClientDndGrabHandler for OkayState {}
impl ServerDndGrabHandler for OkayState {}

impl FractionalScaleHandler for OkayState {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let scale = self
            .space
            .outputs()
            .next()
            .map(|o| o.current_scale().fractional_scale())
            .unwrap_or(1.0);
        with_states(&surface, |states| {
            fractional_scale::with_fractional_scale(states, |fs| {
                fs.set_preferred_scale(scale);
            });
        });
    }
}

impl SessionLockHandler for OkayState {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.session_lock_state
    }

    fn lock(&mut self, confirmation: SessionLocker) {
        self.ext_locked = true;
        self.overlays.locked = true;
        confirmation.lock();
    }

    fn unlock(&mut self) {
        self.ext_locked = false;
        self.overlays.locked = false;
        self.ext_lock_surfaces.clear();
        self.refocus_topmost_visible();
    }

    fn new_surface(&mut self, surface: LockSurface, wl_output: WlOutput) {
        let output = Output::from_resource(&wl_output)
            .or_else(|| self.space.outputs().next().cloned());
        if let Some(out) = output {
            let size = self
                .space
                .output_geometry(&out)
                .map(|g| g.size)
                .unwrap_or_else(|| (1280, 720).into());
            surface.with_pending_state(|s| {
                s.size = Some(smithay::utils::Size::from((size.w as u32, size.h as u32)));
            });
            surface.send_configure();
            let wl_surf = surface.wl_surface().clone();
            self.ext_lock_surfaces.retain(|(o, _)| o != &out);
            self.ext_lock_surfaces.push((out, surface));
            if let Some(kb) = self.seat.get_keyboard() {
                kb.set_focus(
                    self,
                    Some(wl_surf),
                    smithay::utils::SERIAL_COUNTER.next_serial(),
                );
            }
        }
    }
}

delegate_compositor!(OkayState);
delegate_shm!(OkayState);
delegate_xdg_shell!(OkayState);
delegate_xdg_decoration!(OkayState);
delegate_layer_shell!(OkayState);
delegate_seat!(OkayState);
delegate_output!(OkayState);
delegate_data_device!(OkayState);
delegate_data_control!(OkayState);
delegate_fractional_scale!(OkayState);
delegate_session_lock!(OkayState);
