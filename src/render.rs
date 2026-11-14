use std::cell::RefCell;
use std::sync::{Mutex, OnceLock};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            damage::{OutputDamageTracker, RenderOutputResult},
            element::{
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                solid::{SolidColorBuffer, SolidColorRenderElement},
                surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
                AsRenderElements, Kind,
            },
            gles::{
                element::PixelShaderElement, GlesError, GlesPixelProgram, GlesRenderer, GlesTarget,
                Uniform, UniformName, UniformType,
            },
            Color32F,
        },
    },
    desktop::{layer_map_for_output, utils::send_frames_surface_tree},
    input::pointer::{CursorImageAttributes, CursorImageStatus},
    output::Output,
    reexports::wayland_server::Resource,
    render_elements,
    utils::{IsAlive, Logical, Physical, Point, Rectangle, Scale, Transform},
    wayland::{compositor::with_states, shell::wlr_layer::Layer},
};

use crate::state::OkayState;

render_elements! {
    pub OkayRenderElements<=GlesRenderer>;
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
    Solid=SolidColorRenderElement,
    Memory=MemoryRenderBufferRenderElement<GlesRenderer>,
    Shader=PixelShaderElement,
}

/// Collects all render elements for a given `Output` in front-to-back order
/// (Cursor -> Overlay/Top Layer -> Native Panel -> Visible Windows & Borders -> Bottom/Background Layer -> Desktop Wallpaper).
///
/// Designed to be easily extended with custom GLSL post-processing shaders,
/// window animations, and native compositor UI widgets.
pub fn collect_output_elements(
    state: &mut OkayState,
    renderer: &mut GlesRenderer,
    output: &Output,
    scale: Scale<f64>,
) -> Vec<OkayRenderElements> {
    let mut elements: Vec<OkayRenderElements> = Vec::new();
    let Some(output_geo) = state.space.output_geometry(output) else {
        return elements;
    };

    // Advance 60+ FPS animations once per output frame
    state.animations.tick(
        state.panel.launcher_open,
        state.overlays.calendar_open,
        state.overlays.notif_drawer_open,
        state.overlays.clipboard_open,
    );

    // 0. Pointer cursor (topmost element on whichever output contains the pointer)
    if output_geo.to_f64().contains(state.pointer_location) {
        let local_cursor = state.pointer_location - output_geo.loc.to_f64();
        elements.extend(collect_cursor_elements(
            &state.cursor_status,
            local_cursor,
            renderer,
            scale,
        ));
    }

    // 0b. Topmost desktop overlays (Lockscreen, Alt+Tab, Toasts, Notification Drawer, Calendar, Clipboard, Context Menu)
    elements.extend(crate::overlays::render_top_overlays(
        state, renderer, output, scale,
    ));

    let layer_map = layer_map_for_output(output);

    // 1. Overlay and Top layer-shell surfaces (notifications, bars, docks, launchers)
    for layer_type in [Layer::Overlay, Layer::Top] {
        for layer in layer_map.layers_on(layer_type).rev() {
            if let Some(geo) = layer_map.layer_geometry(layer) {
                let loc = geo.loc.to_physical_precise_round(scale);
                let surface_elems: Vec<OkayRenderElements> =
                    render_elements_from_surface_tree(
                        renderer,
                        layer.wl_surface(),
                        loc,
                        scale,
                        1.0,
                        Kind::Unspecified,
                    );
                elements.extend(surface_elems);
            }
        }
    }

    // 2. Native OKay DE taskbar, launcher popup & OKay Settings window
    elements.extend(crate::panel::render_panel_elements(
        state, renderer, output, scale,
    ));
    elements.extend(crate::settings::render_settings_elements(
        state, renderer, output, scale,
    ));

    // 2b. Active Magic Lamp (Genie) minimize / restore animations
    for window in state.space.elements().rev() {
        if !state.workspaces.is_on_active_workspace(window) {
            continue;
        }
        if let Some(toplevel) = window.toplevel() {
            let id = toplevel.wl_surface().id();
            if state.animations.is_animating_minimize(&id) {
                elements.extend(state.animations.render_minimize_effect(&id, scale));
            }
        }
    }

    // 3. Visible XDG windows on the active workspace (Always-on-Top first, then normal top-to-bottom)
    let focused_id = state
        .seat
        .get_keyboard()
        .and_then(|k| k.current_focus())
        .map(|s| s.id());
    let accent = state.panel.accent_rgb();
    let ws_slide_x = state.animations.workspace_slide_x.round() as i32;

    let mut ordered_windows: Vec<_> = state.space.elements().rev().cloned().collect();
    ordered_windows.sort_by_key(|w| if state.workspaces.is_pinned(w) { 0 } else { 1 });

    for window in &ordered_windows {
        let win_id = window.toplevel().map(|t| t.wl_surface().id());
        if let Some(ref id) = win_id {
            if state.animations.is_animating_minimize(id) {
                // Window is currently mid-Magic-Lamp funnel animation
                continue;
            }
        }
        if !state.workspaces.is_visible(window) {
            continue;
        }
        let Some(win_loc) = state.space.element_location(window) else {
            continue;
        };

        let (move_dx, move_dy, alpha) = win_id
            .as_ref()
            .and_then(|id| state.animations.windows.get(id))
            .map(|a| {
                (
                    a.move_offset.x.round() as i32,
                    a.move_offset.y.round() as i32,
                    a.open_progress.clamp(0.15, 1.0),
                )
            })
            .unwrap_or((0, 0, 1.0));

        // Translate global space coordinates into output-local coordinates + smooth animation offsets
        let local_loc = win_loc - output_geo.loc + Point::from((ws_slide_x + move_dx, move_dy));
        let phys_loc: Point<i32, Physical> = local_loc.to_physical_precise_round(scale);

        // Window surfaces (toplevel + subsurfaces + popups)
        let win_elements: Vec<OkayRenderElements> =
            window.render_elements(renderer, phys_loc, scale, alpha);
        elements.extend(win_elements);

        // Server-side titlebar & subtle 1px macOS border around non-fullscreen windows
        if !state.workspaces.is_fullscreen(window) {
            let is_focused = win_id.as_ref() == focused_id.as_ref();
            let is_pinned = state.workspaces.is_pinned(window);
            let pointer_local = state.pointer_location - output_geo.loc.to_f64();

            if let Some(tb_elem) = crate::panel::render_titlebar_element(
                window,
                local_loc,
                pointer_local,
                is_focused,
                is_pinned,
                accent,
                renderer,
                scale,
            ) {
                elements.push(tb_elem);
            }

            let win_geo = window.geometry();
            let tb_h = crate::state::TITLEBAR_HEIGHT;
            let pad = 14;
            let shader_area = Rectangle::new(
                local_loc + win_geo.loc - Point::from((pad, pad + tb_h)),
                (
                    win_geo.size.w.max(120) + pad * 2,
                    win_geo.size.h + tb_h + pad * 2,
                )
                    .into(),
            );
            let border_alpha = if is_focused { 0.16f32 } else { 0.08f32 };
            if let Some(sdf_elem) =
                sdf_rounded_window_element(renderer, shader_area, 12.0, pad as f32, border_alpha, alpha)
            {
                elements.push(sdf_elem);
            } else {
                let border_color = if is_focused {
                    [1.0f32, 1.0, 1.0, 0.18]
                } else {
                    state.theme.inactive_border_color
                };
                let bw = state.theme.border_width.min(1);
                if bw > 0 {
                    let border_geo = Rectangle::new(
                        local_loc + win_geo.loc - Point::from((bw, bw + tb_h)),
                        (
                            win_geo.size.w.max(120) + bw * 2,
                            win_geo.size.h + tb_h + bw * 2,
                        )
                            .into(),
                    );
                    let border_elems = build_border_elements(
                        border_geo,
                        bw,
                        Color32F::from(border_color),
                        scale,
                    );
                    elements.extend(border_elems);
                }
            }
        }
    }

    // 4. Bottom and Background layer-shell surfaces (external wallpapers, desktop widgets)
    for layer_type in [Layer::Bottom, Layer::Background] {
        for layer in layer_map.layers_on(layer_type).rev() {
            if let Some(geo) = layer_map.layer_geometry(layer) {
                let loc = geo.loc.to_physical_precise_round(scale);
                let surface_elems: Vec<OkayRenderElements> =
                    render_elements_from_surface_tree(
                        renderer,
                        layer.wl_surface(),
                        loc,
                        scale,
                        1.0,
                        Kind::Unspecified,
                    );
                elements.extend(surface_elems);
            }
        }
    }

    // 4b. Desktop Icons Grid (right above wallpaper, underneath all windows)
    elements.extend(crate::overlays::render_desktop_icons(
        state, renderer, output, scale,
    ));

    // 5. Native OKay DE desktop wallpaper / gradient (bottommost)
    if let Some(bg) = state.desktop.background_element(renderer, output, scale) {
        elements.push(bg);
    }

    elements
}

/// Renders either the client-provided cursor surface or a crisp software pointer arrow.
fn collect_cursor_elements(
    cursor_status: &CursorImageStatus,
    local_cursor: Point<f64, Logical>,
    renderer: &mut GlesRenderer,
    scale: Scale<f64>,
) -> Vec<OkayRenderElements> {
    match cursor_status {
        CursorImageStatus::Hidden => Vec::new(),
        CursorImageStatus::Surface(surface) if surface.alive() => {
            let hotspot = with_states(surface, |states| {
                states
                    .data_map
                    .get::<Mutex<CursorImageAttributes>>()
                    .map(|attrs| attrs.lock().unwrap().hotspot)
                    .unwrap_or_default()
            });
            let pos = (local_cursor - hotspot.to_f64()).to_physical_precise_round(scale);
            render_elements_from_surface_tree(renderer, surface, pos, scale, 1.0, Kind::Cursor)
        }
        _ => {
            let buf = default_cursor_buffer();
            let phys_loc: Point<f64, Physical> = local_cursor.to_physical_precise_round::<f64, i32>(scale).to_f64();
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                phys_loc,
                buf,
                None,
                None,
                Some((16, 22).into()),
                Kind::Cursor,
            ) {
                Ok(elem) => vec![OkayRenderElements::Memory(elem)],
                Err(err) => {
                    tracing::warn!(?err, "Failed to create software cursor render element");
                    Vec::new()
                }
            }
        }
    }
}

/// Cached 16x22 high-contrast RGBA pointer arrow (white fill with dark outline).
fn default_cursor_buffer() -> &'static MemoryRenderBuffer {
    static CURSOR_BUF: OnceLock<MemoryRenderBuffer> = OnceLock::new();
    CURSOR_BUF.get_or_init(|| {
        const W: usize = 16;
        const H: usize = 22;
        #[rustfmt::skip]
        const SHAPE: [&[u8; W]; H] = [
            b"#...............",
            b"##..............",
            b"#.#.............",
            b"#..#............",
            b"#...#...........",
            b"#....#..........",
            b"#.....#.........",
            b"#......#........",
            b"#.......#.......",
            b"#........#......",
            b"#.........#.....",
            b"#..........#....",
            b"#......######...",
            b"#...#..#........",
            b"#..#.#..#.......",
            b"#.#..#..#.......",
            b"##....#..#......",
            b"#.....#..#......",
            b".......#..#.....",
            b".......#..#.....",
            b"........##......",
            b"................",
        ];

        let mut pixels = vec![0u8; W * H * 4];
        for (y, row) in SHAPE.iter().enumerate() {
            let mut inside = false;
            let mut last_hash = None;
            for x in (0..W).rev() {
                if row[x] == b'#' {
                    last_hash = Some(x);
                    break;
                }
            }
            for x in 0..W {
                let idx = (y * W + x) * 4;
                if row[x] == b'#' {
                    // Crisp dark outline
                    pixels[idx] = 16;
                    pixels[idx + 1] = 18;
                    pixels[idx + 2] = 28;
                    pixels[idx + 3] = 255;
                    if Some(x) != last_hash {
                        inside = true;
                    } else {
                        inside = false;
                    }
                } else if inside {
                    // Bright white pointer fill
                    pixels[idx] = 250;
                    pixels[idx + 1] = 252;
                    pixels[idx + 2] = 255;
                    pixels[idx + 3] = 255;
                }
            }
        }

        MemoryRenderBuffer::from_slice(
            &pixels,
            Fourcc::Abgr8888,
            (W as i32, H as i32),
            1,
            Transform::Normal,
            None,
        )
    })
}

/// Compiles (once) and instantiates an OpenGL ES SDF rounded-corner shadow + 1px hairline border shader.
fn sdf_rounded_window_element(
    renderer: &mut GlesRenderer,
    area: Rectangle<i32, Logical>,
    radius: f32,
    pad: f32,
    border_alpha: f32,
    alpha: f32,
) -> Option<OkayRenderElements> {
    const SDF_FRAG: &str = r#"
precision mediump float;
varying vec2 v_coords;
uniform vec2 size;
uniform float alpha;
uniform float u_radius;
uniform float u_pad;
uniform float u_border_alpha;

float sdRoundedBox(vec2 p, vec2 b, float r) {
    vec2 q = abs(p) - b + vec2(r);
    return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - r;
}

void main() {
    vec2 px = v_coords * size;
    vec2 half_size = size * 0.5;
    vec2 box_half = max(half_size - vec2(u_pad), vec2(1.0));
    vec2 p = px - half_size;
    float d = sdRoundedBox(p, box_half, u_radius);

    float shadow = 0.0;
    if (d > 0.0) {
        shadow = exp(-d * 0.28) * 0.38;
    }

    float border = (1.0 - smoothstep(0.0, 1.1, abs(d + 0.5))) * u_border_alpha;
    float a = clamp(shadow + border, 0.0, 1.0) * alpha;
    gl_FragColor = vec4(vec3(border * alpha), a);
}
"#;

    thread_local! {
        static SDF_SHADER: RefCell<Option<GlesPixelProgram>> = const { RefCell::new(None) };
    }

    let program = SDF_SHADER.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if borrow.is_none() {
            if let Ok(prog) = renderer.compile_custom_pixel_shader(
                SDF_FRAG,
                &[
                    UniformName::new("u_radius", UniformType::_1f),
                    UniformName::new("u_pad", UniformType::_1f),
                    UniformName::new("u_border_alpha", UniformType::_1f),
                ],
            ) {
                *borrow = Some(prog);
            }
        }
        borrow.clone()
    })?;

    Some(OkayRenderElements::Shader(PixelShaderElement::new(
        program,
        area,
        None,
        alpha,
        vec![
            Uniform::new("u_radius", radius),
            Uniform::new("u_pad", pad),
            Uniform::new("u_border_alpha", border_alpha),
        ],
        Kind::Unspecified,
    )))
}

/// Builds 4 crisp solid-color border strips (top, bottom, left, right) around `rect`.
fn build_border_elements(
    rect: Rectangle<i32, Logical>,
    thickness: i32,
    color: Color32F,
    scale: Scale<f64>,
) -> Vec<OkayRenderElements> {
    if thickness <= 0 || rect.size.w <= 0 || rect.size.h <= 0 {
        return Vec::new();
    }

    let strips = [
        // Top
        Rectangle::new(rect.loc, (rect.size.w, thickness).into()),
        // Bottom
        Rectangle::new(
            (rect.loc.x, rect.loc.y + rect.size.h - thickness).into(),
            (rect.size.w, thickness).into(),
        ),
        // Left
        Rectangle::new(
            (rect.loc.x, rect.loc.y + thickness).into(),
            (thickness, (rect.size.h - 2 * thickness).max(0)).into(),
        ),
        // Right
        Rectangle::new(
            (rect.loc.x + rect.size.w - thickness, rect.loc.y + thickness).into(),
            (thickness, (rect.size.h - 2 * thickness).max(0)).into(),
        ),
    ];

    strips
        .into_iter()
        .filter(|s| s.size.w > 0 && s.size.h > 0)
        .map(|strip| {
            let buf = SolidColorBuffer::new(strip.size, color);
            let phys_loc = strip.loc.to_physical_precise_round(scale);
            OkayRenderElements::Solid(SolidColorRenderElement::from_buffer(
                &buf,
                phys_loc,
                scale,
                1.0,
                Kind::Unspecified,
            ))
        })
        .collect()
}

/// Renders a single frame for `output` using `OutputDamageTracker`.
pub fn render_output<'a, 'd>(
    state: &mut OkayState,
    renderer: &mut GlesRenderer,
    framebuffer: &mut GlesTarget<'a>,
    damage_tracker: &'d mut OutputDamageTracker,
    output: &Output,
    age: usize,
) -> Result<RenderOutputResult<'d>, smithay::backend::renderer::damage::Error<GlesError>> {
    let scale = Scale::from(output.current_scale().fractional_scale());
    let elements = collect_output_elements(state, renderer, output, scale);
    let clear_color = Color32F::from(state.theme.background_color);

    damage_tracker.render_output(renderer, framebuffer, age, &elements, clear_color)
}

/// Sends Wayland frame callbacks to all mapped windows and layer-shell surfaces on `output`.
pub fn send_frame_callbacks(state: &OkayState, output: &Output) {
    let time = state.start_time.elapsed();

    for window in state.space.elements() {
        if state.workspaces.is_visible(window) {
            window.send_frame(output, time, Some(std::time::Duration::ZERO), |_, _| {
                Some(output.clone())
            });
        }
    }

    let layer_map = layer_map_for_output(output);
    for layer in layer_map.layers() {
        send_frames_surface_tree(
            layer.wl_surface(),
            output,
            time,
            Some(std::time::Duration::ZERO),
            |_, _| Some(output.clone()),
        );
    }
}
