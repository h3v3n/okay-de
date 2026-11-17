use std::{
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            gles::GlesRenderer,
            utils::draw_render_elements,
            Bind, Color32F, ExportMem, Frame, Offscreen, Renderer, TextureMapping,
        },
    },
    output::Output,
    reexports::{
        wayland_protocols_wlr::screencopy::v1::server::{
            zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
            zwlr_screencopy_manager_v1::{self, ZwlrScreencopyManagerV1},
        },
        wayland_server::{
            protocol::{wl_buffer::WlBuffer, wl_shm},
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
        },
    },
    utils::{Buffer as BufferCoords, Logical, Physical, Rectangle, Scale, Size, Transform},
    wayland::shm::with_buffer_contents_mut,
};

use crate::{render::OkayRenderElements, state::OkayState};

#[derive(Debug, Clone)]
pub struct ScreencopyFrameData {
    pub output: Output,
    pub region: Rectangle<i32, Physical>,
    pub overlay_cursor: bool,
    pub state: std::sync::Arc<Mutex<ScreencopyFrameInner>>,
}

#[derive(Debug, Default)]
pub struct ScreencopyFrameInner {
    pub copied: bool,
}

#[derive(Debug, Clone)]
pub struct PendingScreencopy {
    pub frame: ZwlrScreencopyFrameV1,
    pub buffer: WlBuffer,
    pub output: Output,
    pub region: Rectangle<i32, Physical>,
    pub overlay_cursor: bool,
}

pub fn init_screencopy_global(dh: &DisplayHandle) {
    dh.create_global::<OkayState, ZwlrScreencopyManagerV1, ()>(3, ());
}

impl GlobalDispatch<ZwlrScreencopyManagerV1, ()> for OkayState {
    fn bind(
        _state: &mut Self,
        _handle: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrScreencopyManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for OkayState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ZwlrScreencopyManagerV1,
        request: zwlr_screencopy_manager_v1::Request,
        _data: &(),
        _dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_screencopy_manager_v1::Request::CaptureOutput {
                frame,
                overlay_cursor,
                output,
            } => {
                let Some(out) = Output::from_resource(&output).or_else(|| state.space.outputs().next().cloned()) else {
                    let f = data_init.init(
                        frame,
                        ScreencopyFrameData {
                            output: Output::new("none".into(), smithay::output::PhysicalProperties {
                                size: (0, 0).into(),
                                subpixel: smithay::output::Subpixel::Unknown,
                                make: "OKay DE".into(),
                                model: "None".into(),
                            }),
                            region: Rectangle::from_size((0, 0).into()),
                            overlay_cursor: overlay_cursor != 0,
                            state: Default::default(),
                        },
                    );
                    f.failed();
                    return;
                };

                let size = out
                    .current_mode()
                    .map(|m| m.size)
                    .unwrap_or_else(|| Size::from((1280, 720)));
                let region = Rectangle::from_size(size);
                init_frame(frame, data_init, out, region, overlay_cursor != 0);
            }
            zwlr_screencopy_manager_v1::Request::CaptureOutputRegion {
                frame,
                overlay_cursor,
                output,
                x,
                y,
                width,
                height,
            } => {
                let Some(out) = Output::from_resource(&output).or_else(|| state.space.outputs().next().cloned()) else {
                    let f = data_init.init(
                        frame,
                        ScreencopyFrameData {
                            output: Output::new("none".into(), smithay::output::PhysicalProperties {
                                size: (0, 0).into(),
                                subpixel: smithay::output::Subpixel::Unknown,
                                make: "OKay DE".into(),
                                model: "None".into(),
                            }),
                            region: Rectangle::from_size((0, 0).into()),
                            overlay_cursor: overlay_cursor != 0,
                            state: Default::default(),
                        },
                    );
                    f.failed();
                    return;
                };

                let scale = Scale::from(out.current_scale().fractional_scale());
                let out_size = out
                    .current_mode()
                    .map(|m| m.size)
                    .unwrap_or_else(|| Size::from((1280, 720)));
                let phys_rect: Rectangle<i32, Physical> =
                    Rectangle::<i32, Logical>::new((x, y).into(), (width, height).into())
                        .to_physical_precise_round(scale);

                let rx = phys_rect.loc.x.clamp(0, out_size.w.max(1) - 1);
                let ry = phys_rect.loc.y.clamp(0, out_size.h.max(1) - 1);
                let rw = phys_rect.size.w.clamp(1, (out_size.w - rx).max(1));
                let rh = phys_rect.size.h.clamp(1, (out_size.h - ry).max(1));
                let region = Rectangle::new((rx, ry).into(), (rw, rh).into());

                init_frame(frame, data_init, out, region, overlay_cursor != 0);
            }
            zwlr_screencopy_manager_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

fn init_frame(
    frame: New<ZwlrScreencopyFrameV1>,
    data_init: &mut DataInit<'_, OkayState>,
    output: Output,
    region: Rectangle<i32, Physical>,
    overlay_cursor: bool,
) {
    let f = data_init.init(
        frame,
        ScreencopyFrameData {
            output,
            region,
            overlay_cursor,
            state: Default::default(),
        },
    );
    let w = region.size.w.max(1) as u32;
    let h = region.size.h.max(1) as u32;
    let stride = w * 4;
    f.buffer(wl_shm::Format::Argb8888, w, h, stride);
    if f.version() >= 3 {
        f.buffer_done();
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, ScreencopyFrameData> for OkayState {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &ZwlrScreencopyFrameV1,
        request: zwlr_screencopy_frame_v1::Request,
        data: &ScreencopyFrameData,
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_screencopy_frame_v1::Request::Copy { buffer }
            | zwlr_screencopy_frame_v1::Request::CopyWithDamage { buffer } => {
                if let Ok(mut guard) = data.state.lock() {
                    if guard.copied {
                        resource.post_error(
                            zwlr_screencopy_frame_v1::Error::AlreadyUsed,
                            "screencopy frame already used",
                        );
                        return;
                    }
                    guard.copied = true;
                }
                state.pending_screencopies.push(PendingScreencopy {
                    frame: resource.clone(),
                    buffer,
                    output: data.output.clone(),
                    region: data.region,
                    overlay_cursor: data.overlay_cursor,
                });
            }
            zwlr_screencopy_frame_v1::Request::Destroy => {
                state.pending_screencopies.retain(|p| p.frame != *resource);
            }
            _ => {}
        }
    }
}

/// Fulfills any queued `zwlr_screencopy_frame_v1` requests for `output` by rendering
/// the current compositor elements offscreen and copying the pixels into the client's `wl_shm` buffer.
pub fn process_pending_screencopies(
    state: &mut OkayState,
    renderer: &mut GlesRenderer,
    output: &Output,
    scale: Scale<f64>,
) {
    let mut remaining = Vec::new();
    let mut ready = Vec::new();
    for req in state.pending_screencopies.drain(..) {
        if req.output == *output {
            ready.push(req);
        } else {
            remaining.push(req);
        }
    }
    state.pending_screencopies = remaining;

    if ready.is_empty() {
        return;
    }

    let Some(mode) = output.current_mode() else {
        for req in ready {
            req.frame.failed();
        }
        return;
    };
    let out_size = mode.size;
    if out_size.w <= 0 || out_size.h <= 0 {
        for req in ready {
            req.frame.failed();
        }
        return;
    }

    let elements: Vec<OkayRenderElements> =
        crate::render::collect_output_elements(state, renderer, output, scale);
    let clear_color = Color32F::from(state.theme.background_color);

    let buf_size: Size<i32, BufferCoords> = (out_size.w, out_size.h).into();
    let Ok(mut tex) = <GlesRenderer as Offscreen<
        smithay::backend::renderer::gles::GlesTexture,
    >>::create_buffer(renderer, Fourcc::Argb8888, buf_size) else {
        for req in ready {
            req.frame.failed();
        }
        return;
    };

    {
        let Ok(mut target) = renderer.bind(&mut tex) else {
            for req in ready {
                req.frame.failed();
            }
            return;
        };
        let Ok(mut frame) = renderer.render(&mut target, out_size, Transform::Normal) else {
            for req in ready {
                req.frame.failed();
            }
            return;
        };
        let dmg = Rectangle::from_size(out_size);
        let _ = frame.clear(clear_color, &[dmg]);
        let _ = draw_render_elements(&mut frame, scale, &elements, &[dmg]);
        if frame.finish().is_err() {
            for req in ready {
                req.frame.failed();
            }
            return;
        }
    }

    let Ok(target) = renderer.bind(&mut tex) else {
        for req in ready {
            req.frame.failed();
        }
        return;
    };

    for req in ready {
        if copy_region_to_shm(renderer, &target, &req) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default();
            let secs = now.as_secs();
            let tv_sec_hi = (secs >> 32) as u32;
            let tv_sec_lo = (secs & 0xffff_ffff) as u32;
            let tv_nsec = now.subsec_nanos();
            if req.frame.version() >= 2 {
                req.frame.damage(
                    req.region.loc.x as u32,
                    req.region.loc.y as u32,
                    req.region.size.w as u32,
                    req.region.size.h as u32,
                );
            }
            req.frame.ready(tv_sec_hi, tv_sec_lo, tv_nsec);
        } else {
            req.frame.failed();
        }
    }
}

fn copy_region_to_shm(
    renderer: &mut GlesRenderer,
    target: &smithay::backend::renderer::gles::GlesTarget<'_>,
    req: &PendingScreencopy,
) -> bool {
    let region_buf: Rectangle<i32, BufferCoords> = Rectangle::new(
        (req.region.loc.x, req.region.loc.y).into(),
        (req.region.size.w, req.region.size.h).into(),
    );
    let Ok(mapping) = renderer.copy_framebuffer(target, region_buf, Fourcc::Argb8888) else {
        return false;
    };
    let is_flipped = mapping.flipped();
    let Ok(src_bytes) = renderer.map_texture(&mapping) else {
        return false;
    };

    let w = req.region.size.w as usize;
    let h = req.region.size.h as usize;
    let src_stride = w * 4;
    if src_bytes.len() < src_stride * h {
        return false;
    }

    let write_res = with_buffer_contents_mut(&req.buffer, |ptr, pool_len, data| {
        if data.width < req.region.size.w || data.height < req.region.size.h {
            return false;
        }
        let dst_stride = data.stride as usize;
        let offset = data.offset as usize;
        if offset + dst_stride * h > pool_len {
            return false;
        }
        for y in 0..h {
            let src_y = if is_flipped { h - 1 - y } else { y };
            let src_row = &src_bytes[src_y * src_stride..(src_y + 1) * src_stride];
            unsafe {
                let dst_ptr = ptr.add(offset + y * dst_stride);
                std::ptr::copy_nonoverlapping(src_row.as_ptr(), dst_ptr, src_stride);
            }
        }
        true
    });

    match write_res {
        Ok(ok) => {
            let _ = ok;
            req.frame.flags(zwlr_screencopy_frame_v1::Flags::empty());
            ok
        }
        Err(_) => false,
    }
}
