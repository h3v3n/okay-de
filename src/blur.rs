use std::cell::RefCell;
use std::collections::HashMap;

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{texture::TextureRenderElement, Id, Kind},
            gles::{
                element::TextureShaderElement, GlesRenderer, GlesTexProgram, GlesTexture, Uniform,
                UniformName, UniformType,
            },
            utils::{draw_render_elements, DamageSnapshot},
            Bind, Color32F, Frame, Offscreen, Renderer,
        },
    },
    output::Output,
    utils::{Buffer as BufferCoord, Logical, Physical, Point, Rectangle, Scale, Size, Transform},
};

use crate::render::OkayRenderElements;

/// Single-pass 25-tap (5x5 Gaussian) frosted-glass fragment shader sampling the captured
/// offscreen background FBO (`GL_TEXTURE_2D`) with SDF rounded-rect clipping, uniform dark
/// tint (`mix(blurred_color, vec3(0.10, 0.12, 0.18), 0.45)`), anti-banding grain, and 1px edge.
const FROSTED_GLASS_FRAG: &str = r#"
//_DEFINES
precision mediump float;
varying vec2 v_coords;
uniform float alpha;
uniform vec2 u_pos;
uniform vec2 u_size;
uniform vec2 u_out_size;
uniform vec2 u_blur_step;
uniform float u_radius;
uniform float u_edge_alpha;
#ifdef EXTERNAL
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

float sdRoundedBox(vec2 p, vec2 b, float r) {
    vec2 q = abs(p) - b + vec2(r);
    return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - r;
}

float hash12(vec2 p) {
    vec3 p3 = fract(vec3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

void main() {
    vec2 local_uv = clamp(v_coords, vec2(0.0), vec2(1.0));
    vec2 px = local_uv * u_size;
    float d = sdRoundedBox(px - u_size * 0.5, u_size * 0.5, u_radius);
    float mask = 1.0 - smoothstep(-0.75, 0.75, d);
    if (mask <= 0.001) {
        discard;
    }

    vec2 screen_uv = clamp((u_pos + local_uv * u_size) / max(u_out_size, vec2(1.0)), vec2(0.001), vec2(0.999));

    // 25-tap (5x5) Gaussian blur over captured background texture
    vec3 blur_sum = vec3(0.0);
    float w_total = 0.0;
    for (int iy = -2; iy <= 2; iy++) {
        float ay = abs(float(iy));
        float wy = (ay < 0.5) ? 6.0 : ((ay < 1.5) ? 4.0 : 1.0);
        for (int ix = -2; ix <= 2; ix++) {
            float ax = abs(float(ix));
            float wx = (ax < 0.5) ? 6.0 : ((ax < 1.5) ? 4.0 : 1.0);
            float w = wx * wy;
            vec2 uv = clamp(screen_uv + vec2(float(ix), float(iy)) * u_blur_step, vec2(0.001), vec2(0.999));
            blur_sum += texture2D(tex, uv).rgb * w;
            w_total += w;
        }
    }
    vec3 blurred_color = blur_sum / max(w_total, 1.0);

    // Uniform frosted dark tint across the entire surface
    vec3 col = mix(blurred_color, vec3(0.10, 0.12, 0.18), 0.45);

    // Subtle procedural grain prevents 8-bit color banding
    float grain = (hash12(px + u_pos) - 0.5) * 0.018;
    col = clamp(col + vec3(grain), 0.0, 1.0);

    // Subtle 1px bright border on the SDF perimeter
    float edge = (1.0 - smoothstep(0.0, 1.05, abs(d + 0.5))) * u_edge_alpha;
    col = mix(col, vec3(1.0), edge);

    gl_FragColor = vec4(col * mask * alpha, mask * alpha);
}
"#;

struct OutputBgCapture {
    phys_size: Size<i32, Physical>,
    logical_size: Size<i32, Logical>,
    capture_size: Size<i32, Physical>,
    bg_tex: Option<GlesTexture>,
}

impl OutputBgCapture {
    fn new(phys_size: Size<i32, Physical>, logical_size: Size<i32, Logical>) -> Self {
        let capture_size: Size<i32, Physical> =
            ((phys_size.w / 2).max(1), (phys_size.h / 2).max(1)).into();
        Self {
            phys_size,
            logical_size,
            capture_size,
            bg_tex: None,
        }
    }
}

thread_local! {
    static FROSTED_SHADER: RefCell<Option<GlesTexProgram>> = const { RefCell::new(None) };
    static BG_CAPTURE_CACHE: RefCell<HashMap<String, OutputBgCapture>> = RefCell::new(HashMap::new());
}

fn ensure_frosted_shader(renderer: &mut GlesRenderer) -> Option<GlesTexProgram> {
    FROSTED_SHADER.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if borrow.is_none() {
            match renderer.compile_custom_texture_shader(
                FROSTED_GLASS_FRAG,
                &[
                    UniformName::new("u_pos", UniformType::_2f),
                    UniformName::new("u_size", UniformType::_2f),
                    UniformName::new("u_out_size", UniformType::_2f),
                    UniformName::new("u_blur_step", UniformType::_2f),
                    UniformName::new("u_radius", UniformType::_1f),
                    UniformName::new("u_edge_alpha", UniformType::_1f),
                ],
            ) {
                Ok(prog) => *borrow = Some(prog),
                Err(err) => {
                    tracing::error!(?err, "Failed to compile FROSTED_GLASS_FRAG shader");
                }
            }
        }
        borrow.clone()
    })
}

/// Captures `bg_elements` (wallpaper, desktop icons, and windows) into an offscreen
/// `GL_TEXTURE_2D` FBO on every frame so Glass/Blur panels can sample & blur it.
pub fn update_output_blur(
    renderer: &mut GlesRenderer,
    output: &Output,
    scale: Scale<f64>,
    bg_elements: &[OkayRenderElements],
    clear_color: Color32F,
) {
    let Some(mode) = output.current_mode() else {
        return;
    };
    let phys_size = mode.size;
    if phys_size.w <= 0 || phys_size.h <= 0 {
        return;
    }
    let logical_size: Size<i32, Logical> = phys_size.to_f64().to_logical(scale).to_i32_round();
    if logical_size.w <= 0 || logical_size.h <= 0 {
        return;
    }

    let out_name = output.name();
    BG_CAPTURE_CACHE.with(|cell| {
        let mut map = cell.borrow_mut();
        let entry = map
            .entry(out_name)
            .or_insert_with(|| OutputBgCapture::new(phys_size, logical_size));

        if entry.phys_size != phys_size || entry.logical_size != logical_size {
            entry.phys_size = phys_size;
            entry.logical_size = logical_size;
            entry.capture_size = ((phys_size.w / 2).max(1), (phys_size.h / 2).max(1)).into();
            entry.bg_tex = None;
        }

        if entry.bg_tex.is_none() {
            let buf_size: Size<i32, BufferCoord> =
                (entry.capture_size.w, entry.capture_size.h).into();
            match <GlesRenderer as Offscreen<GlesTexture>>::create_buffer(
                renderer,
                Fourcc::Abgr8888,
                buf_size,
            ) {
                Ok(tex) => entry.bg_tex = Some(tex),
                Err(err) => {
                    tracing::error!(?err, "Failed to create offscreen blur FBO texture");
                    return;
                }
            }
        }

        let Some(bg_tex) = entry.bg_tex.as_mut() else {
            return;
        };

        let half_scale = Scale {
            x: entry.capture_size.w as f64 / logical_size.w.max(1) as f64,
            y: entry.capture_size.h as f64 / logical_size.h.max(1) as f64,
        };

        let Ok(mut target) = renderer.bind(bg_tex) else {
            tracing::error!("Failed to bind offscreen blur FBO");
            return;
        };
        let Ok(mut frame) = renderer.render(&mut target, entry.capture_size, Transform::Normal)
        else {
            tracing::error!("Failed to begin offscreen blur frame");
            return;
        };
        let dmg = Rectangle::from_size(entry.capture_size);
        let _ = frame.clear(clear_color, &[dmg]);
        let _ = draw_render_elements(&mut frame, half_scale, bg_elements, &[dmg]);
        let _ = frame.finish();
    });
}

/// Returns a real 25-tap Gaussian frosted-glass backdrop element for `rect`, sampling
/// the captured offscreen FBO texture, clipped with an SDF rounded-rect mask, uniform tint,
/// and 1px bright edge.
#[allow(clippy::too_many_arguments)]
pub fn blur_panel_element(
    renderer: &mut GlesRenderer,
    output: &Output,
    scale: Scale<f64>,
    _key: &'static str,
    rect: Rectangle<i32, Logical>,
    radius: f32,
    _tint_rgb: [u8; 3],
    _tint_alpha: f32,
    edge_alpha: f32,
    alpha: f32,
) -> Option<OkayRenderElements> {
    if rect.size.w <= 0 || rect.size.h <= 0 || alpha <= 0.005 {
        return None;
    }

    let program = ensure_frosted_shader(renderer)?;

    let out_name = output.name();
    let (tex, out_logical) = BG_CAPTURE_CACHE.with(|cell| {
        let map = cell.borrow();
        let entry = map.get(&out_name)?;
        Some((entry.bg_tex.clone()?, entry.logical_size))
    })?;

    if out_logical.w <= 0 || out_logical.h <= 0 {
        return None;
    }

    let phys_loc: Point<f64, Physical> = rect.loc.to_f64().to_physical(scale);
    let tex_elem = TextureRenderElement::from_texture_with_damage(
        Id::new(),
        renderer.context_id(),
        phys_loc,
        tex,
        1,
        Transform::Normal,
        Some(alpha),
        None,
        Some(rect.size),
        None,
        DamageSnapshot::empty(),
        Kind::Unspecified,
    );

    // 5x5 Gaussian kernel with ~20px blur radius in logical screen space
    let blur_radius_px = 20.0f32;
    let step_x = (blur_radius_px * 0.5) / out_logical.w.max(1) as f32;
    let step_y = (blur_radius_px * 0.5) / out_logical.h.max(1) as f32;

    Some(OkayRenderElements::TexShader(TextureShaderElement::new(
        tex_elem,
        program,
        vec![
            Uniform::new("u_pos", [rect.loc.x as f32, rect.loc.y as f32]),
            Uniform::new("u_size", [rect.size.w as f32, rect.size.h as f32]),
            Uniform::new(
                "u_out_size",
                [out_logical.w as f32, out_logical.h as f32],
            ),
            Uniform::new("u_blur_step", [step_x, step_y]),
            Uniform::new("u_radius", radius),
            Uniform::new("u_edge_alpha", edge_alpha),
        ],
    )))
}
