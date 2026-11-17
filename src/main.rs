mod animation;
mod desktop;
mod icons;
mod input;
mod overlays;
mod panel;
mod render;
mod screencopy;
mod settings;
mod shell;
mod state;
mod system_backends;
mod worker;

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::Path,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use calloop::{EventLoop, LoopHandle};
use smithay::{
    backend::{
        allocator::{
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
            Fourcc,
        },
        drm::{
            compositor::{DrmCompositor, FrameFlags},
            exporter::gbm::GbmFramebufferExporter,
            DrmDevice, DrmDeviceFd, DrmEvent, DrmNode, NodeType,
        },
        egl::{EGLContext, EGLDisplay},
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            damage::OutputDamageTracker,
            gles::GlesRenderer,
        },
        session::{libseat::LibSeatSession, Event as SessionEvent, Session},
        udev::{self, UdevBackend, UdevEvent},
        winit::{self, WinitEvent},
    },
    desktop::layer_map_for_output,
    output::{Mode as OutputMode, Output, PhysicalProperties, Subpixel},
    reexports::{
        drm::control::{connector, crtc, Device as ControlDevice},
        input::Libinput,
        rustix::fs::OFlags,
        wayland_server::Display,
    },
    utils::{DeviceFd, Scale, Transform},
    wayland::socket::ListeningSocketSource,
};
use tracing_subscriber::EnvFilter;

use crate::{
    input::process_input_event,
    render::{render_output, send_frame_callbacks},
    state::{ClientState, OkayState},
};

const SUPPORTED_COLOR_FORMATS: &[Fourcc] = &[
    Fourcc::Argb8888,
    Fourcc::Xrgb8888,
    Fourcc::Abgr8888,
    Fourcc::Xbgr8888,
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let force_winit = std::env::args().any(|a| a == "--winit");
    let force_drm = std::env::args().any(|a| a == "--drm");
    let is_nested = std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var_os("DISPLAY").is_some();

    let use_winit = if force_drm {
        false
    } else if force_winit {
        true
    } else {
        is_nested
    };

    if use_winit {
        tracing::info!("Starting OKay DE with Winit nested backend");
        run_winit()?;
    } else {
        tracing::info!("Starting OKay DE with DRM/KMS + libseat + libinput backend");
        run_drm()?;
    }

    Ok(())
}

// ============================================================================
// Wayland Display + Calloop Socket Setup
// ============================================================================

fn init_wayland_loop() -> Result<
    (
        EventLoop<'static, OkayState>,
        Display<OkayState>,
        OkayState,
    ),
    Box<dyn std::error::Error>,
> {
    let event_loop: EventLoop<'static, OkayState> = EventLoop::try_new()?;
    let display: Display<OkayState> = Display::new()?;

    let socket = ListeningSocketSource::new_auto()?;
    let socket_name = socket.socket_name().to_os_string();
    tracing::info!(?socket_name, "Listening on Wayland socket");

    event_loop
        .handle()
        .insert_source(socket, |client_stream, _, state| {
            if let Err(err) = state
                .display_handle
                .insert_client(client_stream, Arc::new(ClientState::default()))
            {
                tracing::warn!(?err, "Failed to insert Wayland client");
            }
        })?;

    let (worker_tx, worker_rx) = calloop::channel::channel::<crate::worker::WorkerResponse>();
    crate::worker::init_worker(worker_tx);
    event_loop
        .handle()
        .insert_source(worker_rx, |event, _, state| {
            if let calloop::channel::Event::Msg(resp) = event {
                crate::worker::handle_response(state, resp);
            }
        })?;

    let state = OkayState::new(&display, event_loop.get_signal(), socket_name);

    Ok((event_loop, display, state))
}

// ============================================================================
// Winit Nested Backend (Development & Debugging inside Wayland/X11)
// ============================================================================

fn run_winit() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize Winit BEFORE overriding WAYLAND_DISPLAY so it connects to the host compositor.
    let (mut backend, winit_evt_loop) = winit::init::<GlesRenderer>()?;

    let (mut event_loop, mut display, mut state) = init_wayland_loop()?;
    std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);
    state.spawn_autostart();

    let size = backend.window_size();
    let mode = OutputMode {
        size,
        refresh: 60_000,
    };

    let output = Output::new(
        "okay-winit-0".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "OKay DE".into(),
            model: "Winit Virtual Output".into(),
        },
    );
    output.create_global::<OkayState>(&state.display_handle);
    output.change_current_state(
        Some(mode),
        Some(Transform::Flipped180),
        None,
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    state.space.map_output(&output, (0, 0));

    let mut damage_tracker = OutputDamageTracker::from_output(&output);

    // Render and submit the initial frame immediately so the nested Wayland window maps.
    {
        let age = backend.buffer_age().unwrap_or(0);
        let (renderer, mut framebuffer) = backend.bind()?;
        let _ = render_output(
            &mut state,
            renderer,
            &mut framebuffer,
            &mut damage_tracker,
            &output,
            age,
        );
    }
    if let Err(err) = backend.submit(None) {
        tracing::warn!(?err, "Initial Winit submit failed");
    }
    backend.window().request_redraw();

    event_loop
        .handle()
        .insert_source(winit_evt_loop, move |event, _, state| match event {
            WinitEvent::Resized { size, scale_factor } => {
                let new_mode = OutputMode {
                    size,
                    refresh: 60_000,
                };
                output.change_current_state(
                    Some(new_mode),
                    None,
                    Some(smithay::output::Scale::Fractional(scale_factor)),
                    None,
                );
                output.set_preferred(new_mode);
                layer_map_for_output(&output).arrange();
            }
            WinitEvent::Input(input_event) => {
                process_input_event(state, input_event);
            }
            WinitEvent::Redraw => {
                let age = backend.buffer_age().unwrap_or(0);
                let frame_ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let Ok((renderer, mut framebuffer)) = backend.bind() else {
                        tracing::warn!("Winit EGL bind failed; skipping frame");
                        return;
                    };
                    if let Err(err) = render_output(
                        state,
                        renderer,
                        &mut framebuffer,
                        &mut damage_tracker,
                        &output,
                        age,
                    ) {
                        tracing::warn!(?err, "Winit render_output failed");
                    }
                    if !state.pending_screencopies.is_empty() {
                        let scale = Scale::from(output.current_scale().fractional_scale());
                        crate::screencopy::process_pending_screencopies(
                            state,
                            renderer,
                            &output,
                            scale,
                        );
                    }
                    if state.pending_native_screenshot.is_some() {
                        let scale = Scale::from(output.current_scale().fractional_scale());
                        crate::screencopy::capture_native_screenshot(
                            state,
                            renderer,
                            &output,
                            scale,
                        );
                    }
                }));
                if frame_ok.is_err() {
                    tracing::error!("Recovered from panic during Winit frame render");
                }
                if let Err(err) = backend.submit(None) {
                    tracing::warn!(?err, "Winit submit failed");
                }
                send_frame_callbacks(state, &output);
                backend.window().request_redraw();
            }
            WinitEvent::CloseRequested => {
                state.running = false;
                state.loop_signal.stop();
            }
            WinitEvent::Focus(_) => {}
        })?;

    while state.running {
        event_loop.dispatch(Some(Duration::from_millis(16)), &mut state)?;
        if state.last_status_poll.elapsed() >= Duration::from_secs(5) {
            state.last_status_poll = Instant::now();
            crate::worker::dispatch(crate::worker::WorkerRequest::PollPanelStatus);
        }
        display.dispatch_clients(&mut state)?;
        display.flush_clients()?;
        state.space.refresh();
        state.popups.cleanup();
    }

    Ok(())
}

// ============================================================================
// DRM/KMS + GBM + EGL/GLES + libseat + libinput Backend (Bare-Metal TTY)
// ============================================================================

type OkayDrmCompositor = DrmCompositor<
    GbmAllocator<DrmDeviceFd>,
    GbmFramebufferExporter<DrmDeviceFd>,
    (),
    DrmDeviceFd,
>;

struct DrmOutputState {
    connector: connector::Handle,
    output: Output,
    compositor: OkayDrmCompositor,
    #[allow(dead_code)]
    damage_tracker: OutputDamageTracker,
}

struct DrmBackendData {
    session: LibSeatSession,
    devices: HashMap<DrmNode, DrmDeviceData>,
    dev_to_node: HashMap<smithay::reexports::rustix::fs::Dev, DrmNode>,
    primary_gpu: Option<DrmNode>,
    used_crtcs: HashSet<(DrmNode, crtc::Handle)>,
}

struct DrmDeviceData {
    drm_device: DrmDevice,
    gbm: GbmDevice<DrmDeviceFd>,
    renderer: GlesRenderer,
    outputs: HashMap<crtc::Handle, DrmOutputState>,
}

fn run_drm() -> Result<(), Box<dyn std::error::Error>> {
    let (mut event_loop, mut display, mut state) = init_wayland_loop()?;
    std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);
    state.spawn_autostart();

    let (session, notifier) = LibSeatSession::new()?;
    let seat_name = session.seat();

    let udev_backend = UdevBackend::new(&seat_name)?;
    let mut libinput_context =
        Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.clone().into());
    libinput_context
        .udev_assign_seat(&seat_name)
        .map_err(|_| "Failed to assign udev seat to libinput")?;

    let libinput_backend = LibinputInputBackend::new(libinput_context.clone());
    event_loop
        .handle()
        .insert_source(libinput_backend, |event, _, state| {
            process_input_event(state, event);
        })?;

    let drm_data = Rc::new(RefCell::new(DrmBackendData {
        session: session.clone(),
        devices: HashMap::new(),
        dev_to_node: HashMap::new(),
        primary_gpu: udev::primary_gpu(&seat_name)
            .ok()
            .flatten()
            .and_then(|p| DrmNode::from_path(p).ok()),
        used_crtcs: HashSet::new(),
    }));

    let loop_handle = event_loop.handle();
    for (dev_id, path) in udev_backend.device_list() {
        let mut data = drm_data.borrow_mut();
        if let Err(err) = on_drm_device_added(&loop_handle, &mut state, &mut data, dev_id, path) {
            tracing::warn!(?err, ?path, "Failed to initialize DRM device");
        }
    }
    rearrange_drm_outputs(&mut state, &drm_data.borrow());

    // Monitor hotplug udev events (Added / Changed / Removed)
    let drm_data_udev = Rc::clone(&drm_data);
    let udev_loop_handle = event_loop.handle();
    event_loop
        .handle()
        .insert_source(udev_backend, move |event, _, state| {
            let mut data = drm_data_udev.borrow_mut();
            match event {
                UdevEvent::Added { device_id, path } => {
                    tracing::info!(?path, "Udev DRM device added");
                    if let Err(err) =
                        on_drm_device_added(&udev_loop_handle, state, &mut data, device_id, &path)
                    {
                        tracing::warn!(?err, ?path, "Failed to hotplug-add DRM device");
                    } else {
                        rearrange_drm_outputs(state, &data);
                        state.retile_workspace();
                    }
                }
                UdevEvent::Changed { device_id } => {
                    tracing::info!(?device_id, "Udev DRM device changed (connector hotplug)");
                    if let Err(err) = on_drm_device_changed(state, &mut data, device_id) {
                        tracing::warn!(?err, "Failed to handle DRM connector hotplug");
                    } else {
                        rearrange_drm_outputs(state, &data);
                        state.retile_workspace();
                    }
                }
                UdevEvent::Removed { device_id } => {
                    tracing::info!(?device_id, "Udev DRM device removed");
                    on_drm_device_removed(state, &mut data, device_id);
                    rearrange_drm_outputs(state, &data);
                    state.retile_workspace();
                }
            }
        })?;

    // Monitor VT switching via libseat
    event_loop
        .handle()
        .insert_source(notifier, move |event, _, _state| match event {
            SessionEvent::PauseSession => {
                libinput_context.suspend();
                tracing::info!("Session paused (VT switch)");
            }
            SessionEvent::ActivateSession => {
                if let Err(err) = libinput_context.resume() {
                    tracing::error!(?err, "Failed to resume libinput after VT switch");
                }
                tracing::info!("Session resumed");
            }
        })?;

    // Main DRM frame & Wayland event loop
    while state.running {
        event_loop.dispatch(Some(Duration::from_millis(16)), &mut state)?;

        // Render active DRM outputs
        {
            let mut data = drm_data.borrow_mut();
            for dev in data.devices.values_mut() {
                for out_state in dev.outputs.values_mut() {
                    let scale = Scale::from(out_state.output.current_scale().fractional_scale());
                    let frame_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let elements = crate::render::collect_output_elements(
                            &mut state,
                            &mut dev.renderer,
                            &out_state.output,
                            scale,
                        );
                        let clear_color = smithay::backend::renderer::Color32F::from(
                            state.theme.background_color,
                        );

                        match out_state.compositor.render_frame(
                            &mut dev.renderer,
                            &elements,
                            clear_color,
                            FrameFlags::DEFAULT,
                        ) {
                            Ok(res) => {
                                if !res.is_empty {
                                    let _ = out_state.compositor.queue_frame(());
                                }
                                send_frame_callbacks(&state, &out_state.output);
                            }
                            Err(err) => {
                                tracing::debug!(?err, "DRM render_frame skipped/failed");
                            }
                        }

                        if !state.pending_screencopies.is_empty() {
                            crate::screencopy::process_pending_screencopies(
                                &mut state,
                                &mut dev.renderer,
                                &out_state.output,
                                scale,
                            );
                        }
                        if let Some((_, region_opt)) = &state.pending_native_screenshot {
                            let region_opt = *region_opt;
                            let matches_out = if let Some(r) = region_opt {
                                state
                                    .space
                                    .output_geometry(&out_state.output)
                                    .map(|g| g.overlaps(r))
                                    .unwrap_or(true)
                            } else {
                                state
                                    .output_under_pointer()
                                    .map(|o| o == out_state.output)
                                    .unwrap_or(true)
                            };
                            if matches_out {
                                crate::screencopy::capture_native_screenshot(
                                    &mut state,
                                    &mut dev.renderer,
                                    &out_state.output,
                                    scale,
                                );
                            }
                        }
                    }));
                    if frame_res.is_err() {
                        tracing::error!("Recovered from panic during DRM output frame render");
                    }
                }
            }
        }

        if state.last_status_poll.elapsed() >= Duration::from_secs(5) {
            state.last_status_poll = Instant::now();
            crate::worker::dispatch(crate::worker::WorkerRequest::PollPanelStatus);
        }

        display.dispatch_clients(&mut state)?;
        display.flush_clients()?;
        state.space.refresh();
        state.popups.cleanup();
    }

    Ok(())
}

fn init_drm_connector_output(
    state: &mut OkayState,
    node: DrmNode,
    drm_device: &mut DrmDevice,
    gbm: &GbmDevice<DrmDeviceFd>,
    renderer: &GlesRenderer,
    res_handles: &smithay::reexports::drm::control::ResourceHandles,
    conn_handle: connector::Handle,
    used_crtcs: &mut HashSet<(DrmNode, crtc::Handle)>,
) -> Result<Option<(crtc::Handle, DrmOutputState)>, Box<dyn std::error::Error>> {
    let conn = drm_device.get_connector(conn_handle, false)?;
    if conn.state() != connector::State::Connected {
        return Ok(None);
    }
    let Some(mode) = conn.modes().first().copied() else {
        return Ok(None);
    };

    let Some(encoder_handle) = conn
        .current_encoder()
        .or_else(|| conn.encoders().first().copied())
    else {
        return Ok(None);
    };
    let encoder = drm_device.get_encoder(encoder_handle)?;
    let crtc_handle = encoder
        .crtc()
        .filter(|c| !used_crtcs.contains(&(node, *c)))
        .or_else(|| {
            res_handles
                .filter_crtcs(encoder.possible_crtcs())
                .into_iter()
                .find(|c| !used_crtcs.contains(&(node, *c)))
        });
    let Some(crtc_handle) = crtc_handle else {
        tracing::warn!(connector = ?conn.interface(), "No free CRTC available for connected output");
        return Ok(None);
    };
    used_crtcs.insert((node, crtc_handle));

    let surface = drm_device.create_surface(crtc_handle, mode, &[conn.handle()])?;
    let allocator = GbmAllocator::new(
        gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    let exporter = GbmFramebufferExporter::new(gbm.clone(), Some(node));

    let phys_size = conn.size().unwrap_or((0, 0));
    let output_name = format!("{:?}-{}", conn.interface(), conn.interface_id());
    let output = Output::new(
        output_name.clone(),
        PhysicalProperties {
            size: (phys_size.0 as i32, phys_size.1 as i32).into(),
            subpixel: Subpixel::Unknown,
            make: "OKay DE".into(),
            model: "DRM/KMS Output".into(),
        },
    );
    let wl_mode = OutputMode::from(mode);
    for m in conn.modes() {
        output.add_mode(OutputMode::from(*m));
    }
    output.create_global::<OkayState>(&state.display_handle);
    output.change_current_state(
        Some(wl_mode),
        Some(Transform::Normal),
        None,
        Some((0, 0).into()),
    );
    output.set_preferred(wl_mode);
    state.space.map_output(&output, (0, 0));

    let renderer_formats = renderer
        .egl_context()
        .dmabuf_render_formats()
        .iter()
        .copied();

    let compositor = DrmCompositor::new(
        &output,
        surface,
        None,
        allocator,
        exporter,
        SUPPORTED_COLOR_FORMATS.iter().copied(),
        renderer_formats,
        drm_device.cursor_size(),
        Some(gbm.clone()),
    )?;

    let damage_tracker = OutputDamageTracker::from_output(&output);
    tracing::info!(
        output = %output_name,
        ?crtc_handle,
        w = wl_mode.size.w,
        h = wl_mode.size.h,
        "Initialized DRM/KMS output"
    );

    Ok(Some((
        crtc_handle,
        DrmOutputState {
            connector: conn_handle,
            output,
            compositor,
            damage_tracker,
        },
    )))
}

fn rearrange_drm_outputs(state: &mut OkayState, drm_data: &DrmBackendData) {
    let mut x = 0i32;
    let mut max_w = 1280i32;
    let mut max_h = 720i32;

    for dev in drm_data.devices.values() {
        for out_state in dev.outputs.values() {
            let mode_size = out_state
                .output
                .current_mode()
                .map(|m| m.size)
                .unwrap_or_else(|| (1280, 720).into());
            out_state
                .output
                .change_current_state(None, None, None, Some((x, 0).into()));
            state.space.map_output(&out_state.output, (x, 0));
            layer_map_for_output(&out_state.output).arrange();
            x += mode_size.w;
            max_w = x.max(1);
            max_h = max_h.max(mode_size.h);
        }
    }

    state.pointer_location.x = state.pointer_location.x.clamp(0.0, (max_w - 1) as f64);
    state.pointer_location.y = state.pointer_location.y.clamp(0.0, (max_h - 1) as f64);
}

fn on_drm_device_added(
    loop_handle: &LoopHandle<'static, OkayState>,
    state: &mut OkayState,
    drm_data: &mut DrmBackendData,
    dev_id: smithay::reexports::rustix::fs::Dev,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let node = match DrmNode::from_path(path) {
        Ok(n) if n.ty() == NodeType::Primary => n,
        Ok(_) => return Ok(()),
        Err(err) => return Err(Box::new(err)),
    };

    if drm_data.devices.contains_key(&node) {
        return Ok(());
    }

    if drm_data.primary_gpu.is_some() && drm_data.primary_gpu != Some(node) {
        tracing::info!(?node, "Skipping secondary GPU");
        return Ok(());
    }

    let fd = drm_data
        .session
        .open(path, OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK)?;
    let device_fd = DrmDeviceFd::new(DeviceFd::from(fd));

    let (mut drm_device, drm_notifier) = DrmDevice::new(device_fd.clone(), true)?;
    let gbm = GbmDevice::new(device_fd)?;

    let egl_display = unsafe { EGLDisplay::new(gbm.clone())? };
    let egl_context = EGLContext::new(&egl_display)?;
    let renderer = unsafe { GlesRenderer::new(egl_context)? };

    let mut outputs = HashMap::new();
    let res_handles = drm_device.resource_handles()?;

    for conn_handle in res_handles.connectors() {
        if let Some((crtc_handle, out_state)) = init_drm_connector_output(
            state,
            node,
            &mut drm_device,
            &gbm,
            &renderer,
            &res_handles,
            *conn_handle,
            &mut drm_data.used_crtcs,
        )? {
            outputs.insert(crtc_handle, out_state);
        }
    }

    loop_handle.insert_source(drm_notifier, |event, _meta, _state| match event {
        DrmEvent::VBlank(_crtc) => {}
        DrmEvent::Error(err) => {
            tracing::error!(?err, "DRM error event");
        }
    })?;

    drm_data.dev_to_node.insert(dev_id, node);
    drm_data.devices.insert(
        node,
        DrmDeviceData {
            drm_device,
            gbm,
            renderer,
            outputs,
        },
    );

    Ok(())
}

fn on_drm_device_changed(
    state: &mut OkayState,
    drm_data: &mut DrmBackendData,
    dev_id: smithay::reexports::rustix::fs::Dev,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(&node) = drm_data.dev_to_node.get(&dev_id) else {
        return Ok(());
    };
    let Some(dev) = drm_data.devices.get_mut(&node) else {
        return Ok(());
    };

    let res_handles = dev.drm_device.resource_handles()?;
    let mut disconnected_crtcs = Vec::new();
    let mut active_connectors = HashSet::new();

    for (&crtc_handle, out_state) in &dev.outputs {
        let conn = dev.drm_device.get_connector(out_state.connector, false)?;
        if conn.state() != connector::State::Connected {
            disconnected_crtcs.push(crtc_handle);
        } else {
            active_connectors.insert(out_state.connector);
        }
    }

    for crtc_handle in disconnected_crtcs {
        if let Some(out_state) = dev.outputs.remove(&crtc_handle) {
            tracing::info!(output = %out_state.output.name(), "Unplugged DRM output");
            state.space.unmap_output(&out_state.output);
            drm_data.used_crtcs.remove(&(node, crtc_handle));
        }
    }

    for &conn_handle in res_handles.connectors() {
        if active_connectors.contains(&conn_handle) {
            continue;
        }
        if let Some((crtc_handle, out_state)) = init_drm_connector_output(
            state,
            node,
            &mut dev.drm_device,
            &dev.gbm,
            &dev.renderer,
            &res_handles,
            conn_handle,
            &mut drm_data.used_crtcs,
        )? {
            dev.outputs.insert(crtc_handle, out_state);
        }
    }

    Ok(())
}

fn on_drm_device_removed(
    state: &mut OkayState,
    drm_data: &mut DrmBackendData,
    dev_id: smithay::reexports::rustix::fs::Dev,
) {
    let Some(node) = drm_data.dev_to_node.remove(&dev_id) else {
        return;
    };
    if let Some(mut dev) = drm_data.devices.remove(&node) {
        for (crtc_handle, out_state) in dev.outputs.drain() {
            tracing::info!(output = %out_state.output.name(), "Removed DRM output");
            state.space.unmap_output(&out_state.output);
            drm_data.used_crtcs.remove(&(node, crtc_handle));
        }
    }
}
