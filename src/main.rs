mod desktop;
mod input;
mod panel;
mod render;
mod shell;
mod state;

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Arc,
    time::Duration,
};

use calloop::EventLoop;
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
                {
                    let (renderer, mut framebuffer) = backend.bind().expect("Failed to bind Winit EGL");
                    let _ = render_output(
                        state,
                        renderer,
                        &mut framebuffer,
                        &mut damage_tracker,
                        &output,
                        age,
                    );
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
    output: Output,
    compositor: OkayDrmCompositor,
    #[allow(dead_code)]
    damage_tracker: OutputDamageTracker,
}

struct DrmBackendData {
    session: LibSeatSession,
    devices: HashMap<DrmNode, DrmDeviceData>,
    primary_gpu: Option<DrmNode>,
    used_crtcs: HashSet<(DrmNode, crtc::Handle)>,
    next_output_x: i32,
}

struct DrmDeviceData {
    #[allow(dead_code)]
    drm_device: DrmDevice,
    #[allow(dead_code)]
    gbm: GbmDevice<DrmDeviceFd>,
    renderer: GlesRenderer,
    outputs: HashMap<crtc::Handle, DrmOutputState>,
}

fn run_drm() -> Result<(), Box<dyn std::error::Error>> {
    let (mut event_loop, mut display, mut state) = init_wayland_loop()?;
    std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);

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

    let mut drm_data = DrmBackendData {
        session: session.clone(),
        devices: HashMap::new(),
        primary_gpu: udev::primary_gpu(&seat_name)
            .ok()
            .flatten()
            .and_then(|p| DrmNode::from_path(p).ok()),
        used_crtcs: HashSet::new(),
        next_output_x: 0,
    };

    for (dev_id, path) in udev_backend.device_list() {
        if let Err(err) = on_drm_device_added(&event_loop, &mut state, &mut drm_data, dev_id, path) {
            tracing::warn!(?err, ?path, "Failed to initialize DRM device");
        }
    }

    // Monitor hotplug udev events
    let loop_handle = event_loop.handle();
    event_loop
        .handle()
        .insert_source(udev_backend, move |event, _, _state| match event {
            UdevEvent::Added { device_id: _, path: _ } => {
                tracing::info!("Udev device added");
            }
            UdevEvent::Changed { device_id: _ } => {
                tracing::info!("Udev device changed");
            }
            UdevEvent::Removed { device_id: _ } => {
                tracing::info!("Udev device removed");
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

    let _ = loop_handle;

    // Main DRM frame & Wayland event loop
    while state.running {
        event_loop.dispatch(Some(Duration::from_millis(16)), &mut state)?;

        // Render active DRM outputs
        for dev in drm_data.devices.values_mut() {
            for out_state in dev.outputs.values_mut() {
                let scale = Scale::from(out_state.output.current_scale().fractional_scale());
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
            }
        }

        display.dispatch_clients(&mut state)?;
        display.flush_clients()?;
        state.space.refresh();
        state.popups.cleanup();
    }

    Ok(())
}

fn on_drm_device_added(
    event_loop: &EventLoop<'static, OkayState>,
    state: &mut OkayState,
    drm_data: &mut DrmBackendData,
    _dev_id: smithay::reexports::rustix::fs::Dev,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let node = match DrmNode::from_path(path) {
        Ok(n) if n.ty() == NodeType::Primary => n,
        Ok(_) => return Ok(()),
        Err(err) => return Err(Box::new(err)),
    };

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
        let conn = drm_device.get_connector(*conn_handle, false)?;
        if conn.state() != connector::State::Connected {
            continue;
        }
        let Some(mode) = conn.modes().first().copied() else {
            continue;
        };

        let Some(encoder_handle) = conn.current_encoder().or_else(|| conn.encoders().first().copied()) else {
            continue;
        };
        let encoder = drm_device.get_encoder(encoder_handle)?;
        let crtc_handle = encoder
            .crtc()
            .filter(|c| !drm_data.used_crtcs.contains(&(node, *c)))
            .or_else(|| {
                res_handles
                    .filter_crtcs(encoder.possible_crtcs())
                    .into_iter()
                    .find(|c| !drm_data.used_crtcs.contains(&(node, *c)))
            });
        let Some(crtc_handle) = crtc_handle else {
            tracing::warn!(connector = ?conn.interface(), "No free CRTC available for connected output");
            continue;
        };
        drm_data.used_crtcs.insert((node, crtc_handle));

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
        let output_loc = (drm_data.next_output_x, 0);
        output.create_global::<OkayState>(&state.display_handle);
        output.change_current_state(
            Some(wl_mode),
            Some(Transform::Normal),
            None,
            Some(output_loc.into()),
        );
        output.set_preferred(wl_mode);
        state.space.map_output(&output, output_loc);
        drm_data.next_output_x += wl_mode.size.w;

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
        outputs.insert(
            crtc_handle,
            DrmOutputState {
                output,
                compositor,
                damage_tracker,
            },
        );
        tracing::info!(
            output = %output_name,
            ?crtc_handle,
            x = output_loc.0,
            w = wl_mode.size.w,
            h = wl_mode.size.h,
            "Initialized DRM/KMS output"
        );
    }

    event_loop
        .handle()
        .insert_source(drm_notifier, |event, _meta, _state| match event {
            DrmEvent::VBlank(_crtc) => {}
            DrmEvent::Error(err) => {
                tracing::error!(?err, "DRM error event");
            }
        })?;

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
