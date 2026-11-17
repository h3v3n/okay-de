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

const KAWASE_DOWN_FRAG: &str = r#"
//_DEFINES
precision mediump float;
varying vec2 v_coords;
uniform float alpha;
uniform vec2 u_halfpixel;
uniform float u_offset;
#ifdef EXTERNAL
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

void main() {
    vec2 uv = v_coords;
    vec2 hp = u_halfpixel * u_offset;
    vec4 sum = texture2D(tex, uv) * 4.0;
    sum += texture2D(tex, clamp(uv - hp, vec2(0.001), vec2(0.999)));
    sum += texture2D(tex, clamp(uv + hp, vec2(0.001), vec2(0.999)));
    sum += texture2D(tex, clamp(uv + vec2(hp.x, -hp.y), vec2(0.001), vec2(0.999)));
    sum += texture2D(tex, clamp(uv - vec2(hp.x, -hp.y), vec2(0.001), vec2(0.999)));
    gl_FragColor = (sum / 8.0) * alpha;
}
"#;

const KAWASE_UP_FRAG: &str = r#"
//_DEFINES
precision mediump float;
varying vec2 v_coords;
uniform float alpha;
uniform vec2 u_halfpixel;
uniform float u_offset;
#ifdef EXTERNAL
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

void main() {
    vec2 uv = v_coords;
    vec2 hp = u_halfpixel * u_offset;
    vec4 sum = texture2D(tex, clamp(uv + vec2(-hp.x * 2.0, 0.0), vec2(0.001), vec2(0.999)));
    sum += texture2D(tex, clamp(uv + vec2(-hp.x, hp.y), vec2(0.001), vec2(0.999))) * 2.0;
    sum += texture2D(tex, clamp(uv + vec2(0.0, hp.y * 2.0), vec2(0.001), vec2(0.999)));
    sum += texture2D(tex, clamp(uv + vec2(hp.x, hp.y), vec2(0.001), vec2(0.999))) * 2.0;
    sum += texture2D(tex, clamp(uv + vec2(hp.x * 2.0, 0.0), vec2(0.001), vec2(0.999)));
    sum += texture2D(tex, clamp(uv + vec2(hp.x, -hp.y), vec2(0.001), vec2(0.999))) * 2.0;
    sum += texture2D(tex, clamp(uv + vec2(0.0, -hp.y * 2.0), vec2(0.001), vec2(0.999)));
    sum += texture2D(tex, clamp(uv + vec2(-hp.x, -hp.y), vec2(0.001), vec2(0.999))) * 2.0;
    gl_FragColor = (sum / 12.0) * alpha;
}
"#;

const BLUR_COMPOSITE_FRAG: &str = r#"
//_DEFINES
precision mediump float;
varying vec2 v_coords;
uniform float alpha;
uniform vec2 u_pos;
uniform vec2 u_size;
uniform vec2 u_out_size;
uniform float u_radius;
uniform vec3 u_tint;
uniform float u_tint_alpha;
uniform float u_grain;
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

    vec2 sample_uv = clamp((u_pos + local_uv * u_size) / max(u_out_size, vec2(1.0)), vec2(0.001), vec2(0.999));
    vec4 blurred = texture2D(tex, sample_uv);
    #ifdef NO_ALPHA
    blurred.a = 1.0;
    #endif

    // Dark/tonal tint (~0.48) over the blurred backdrop for legibility
    vec3 col = mix(blurred.rgb, u_tint, u_tint_alpha);

    // Subtle procedural grain prevents 8-bit color banding
    float grain = (hash12(px + u_pos) - 0.5) * u_grain;
    col = clamp(col + vec3(grain), 0.0, 1.0);

    // Crisp 1px bright specular edge on the glass perimeter
    float edge = (1.0 - smoothstep(0.0, 1.05, abs(d + 0.5))) * u_edge_alpha;
    col = mix(col, vec3(1.0), edge);

    gl_FragColor = vec4(col * mask * alpha, mask * alpha);
}
"#;

struct BlurShaders {
    down: GlesTexProgram,
    up: GlesTexProgram,
    composite: GlesTexProgram,
}

struct BlurPyramid {
    half_size: Size<i32, Physical>,
    quarter_size: Size<i32, Physical>,
    eighth_size: Size<i32, Physical>,
    sixteenth_size: Size<i32, Physical>,
    tex_half_a: GlesTexture,
    tex_half_b: GlesTexture,
    tex_quarter_a: GlesTexture,
    tex_quarter_b: GlesTexture,
    tex_eighth_a: GlesTexture,
    tex_eighth_b: GlesTexture,
    tex_sixteenth: GlesTexture,
}

struct OutputBlurCache {
    phys_size: Size<i32, Physical>,
    logical_size: Size<i32, Logical>,
    pyramid: Option<BlurPyramid>,
    blurred_tex: Option<GlesTexture>,
}

impl OutputBlurCache {
    fn new(phys_size: Size<i32, Physical>, logical_size: Size<i32, Logical>) -> Self {
        Self {
            phys_size,
            logical_size,
            pyramid: None,
            blurred_tex: None,
        }
    }
}

thread_local! {
    static BLUR_SHADERS: RefCell<Option<BlurShaders>> = const { RefCell::new(None) };
    static BLUR_CACHE: RefCell<HashMap<String, OutputBlurCache>> = RefCell::new(HashMap::new());
}

fn ensure_shaders(renderer: &mut GlesRenderer) -> Option<(GlesTexProgram, GlesTexProgram, GlesTexProgram)> {
    BLUR_SHADERS.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if borrow.is_none() {
            let down = renderer
                .compile_custom_texture_shader(
                    KAWASE_DOWN_FRAG,
                    &[
                        UniformName::new("u_halfpixel", UniformType::_2f),
                        UniformName::new("u_offset", UniformType::_1f),
                    ],
                )
                .ok()?;
            let up = renderer
                .compile_custom_texture_shader(
                    KAWASE_UP_FRAG,
                    &[
                        UniformName::new("u_halfpixel", UniformType::_2f),
                        UniformName::new("u_offset", UniformType::_1f),
                    ],
                )
                .ok()?;
            let composite = renderer
                .compile_custom_texture_shader(
                    BLUR_COMPOSITE_FRAG,
                    &[
                        UniformName::new("u_pos", UniformType::_2f),
                        UniformName::new("u_size", UniformType::_2f),
                        UniformName::new("u_out_size", UniformType::_2f),
                        UniformName::new("u_radius", UniformType::_1f),
                        UniformName::new("u_tint", UniformType::_3f),
                        UniformName::new("u_tint_alpha", UniformType::_1f),
                        UniformName::new("u_grain", UniformType::_1f),
                        UniformName::new("u_edge_alpha", UniformType::_1f),
                    ],
                )
                .ok()?;
            *borrow = Some(BlurShaders { down, up, composite });
        }
        borrow
            .as_ref()
            .map(|s| (s.down.clone(), s.up.clone(), s.composite.clone()))
    })
}

fn create_offscreen(renderer: &mut GlesRenderer, size: Size<i32, Physical>) -> Option<GlesTexture> {
    let buf_size: Size<i32, BufferCoord> = (size.w.max(1), size.h.max(1)).into();
    <GlesRenderer as Offscreen<GlesTexture>>::create_buffer(renderer, Fourcc::Abgr8888, buf_size).ok()
}

fn create_pyramid(renderer: &mut GlesRenderer, phys_size: Size<i32, Physical>) -> Option<BlurPyramid> {
    let half_size: Size<i32, Physical> = ((phys_size.w / 2).max(1), (phys_size.h / 2).max(1)).into();
    let quarter_size: Size<i32, Physical> = ((phys_size.w / 4).max(1), (phys_size.h / 4).max(1)).into();
    let eighth_size: Size<i32, Physical> = ((phys_size.w / 8).max(1), (phys_size.h / 8).max(1)).into();
    let sixteenth_size: Size<i32, Physical> = ((phys_size.w / 16).max(1), (phys_size.h / 16).max(1)).into();

    Some(BlurPyramid {
        half_size,
        quarter_size,
        eighth_size,
        sixteenth_size,
        tex_half_a: create_offscreen(renderer, half_size)?,
        tex_half_b: create_offscreen(renderer, half_size)?,
        tex_quarter_a: create_offscreen(renderer, quarter_size)?,
        tex_quarter_b: create_offscreen(renderer, quarter_size)?,
        tex_eighth_a: create_offscreen(renderer, eighth_size)?,
        tex_eighth_b: create_offscreen(renderer, eighth_size)?,
        tex_sixteenth: create_offscreen(renderer, sixteenth_size)?,
    })
}

fn blit_kawase(
    renderer: &mut GlesRenderer,
    src_tex: &GlesTexture,
    src_size: Size<i32, Physical>,
    dst_tex: &mut GlesTexture,
    dst_size: Size<i32, Physical>,
    program: GlesTexProgram,
    offset: f32,
) -> Option<()> {
    let tex_elem = TextureRenderElement::from_texture_with_damage(
        Id::new(),
        renderer.context_id(),
        Point::<f64, Physical>::from((0.0, 0.0)),
        src_tex.clone(),
        1,
        Transform::Normal,
        Some(1.0),
        None,
        Some(Size::<i32, Logical>::from((dst_size.w, dst_size.h))),
        None,
        DamageSnapshot::empty(),
        Kind::Unspecified,
    );
    let shader_elem = OkayRenderElements::TexShader(TextureShaderElement::new(
        tex_elem,
        program,
        vec![
            Uniform::new(
                "u_halfpixel",
                [
                    0.5 / src_size.w.max(1) as f32,
                    0.5 / src_size.h.max(1) as f32,
                ],
            ),
            Uniform::new("u_offset", offset),
        ],
    ));

    let mut target = renderer.bind(dst_tex).ok()?;
    let mut frame = renderer
        .render(&mut target, dst_size, Transform::Normal)
        .ok()?;
    let dmg = Rectangle::from_size(dst_size);
    let _ = frame.clear(Color32F::TRANSPARENT, &[dmg]);
    let _ = draw_render_elements(&mut frame, Scale::from(1.0), &[shader_elem], &[dmg]);
    let _ = frame.finish().ok()?;
    Some(())
}

fn run_dual_kawase_passes(
    renderer: &mut GlesRenderer,
    pyramid: &mut BlurPyramid,
    down_prog: &GlesTexProgram,
    up_prog: &GlesTexProgram,
    capture_elements: &[OkayRenderElements],
    logical_size: Size<i32, Logical>,
    clear_color: Color32F,
) -> Option<()> {
    // 1. Capture current frame content below panels/windows into 1/2-res FBO
    let half_scale = Scale {
        x: pyramid.half_size.w as f64 / logical_size.w.max(1) as f64,
        y: pyramid.half_size.h as f64 / logical_size.h.max(1) as f64,
    };
    {
        let mut target = renderer.bind(&mut pyramid.tex_half_a).ok()?;
        let mut frame = renderer
            .render(&mut target, pyramid.half_size, Transform::Normal)
            .ok()?;
        let dmg = Rectangle::from_size(pyramid.half_size);
        let _ = frame.clear(clear_color, &[dmg]);
        let _ = draw_render_elements(&mut frame, half_scale, capture_elements, &[dmg]);
        let _ = frame.finish().ok()?;
    }

    // 2. Multi-pass Downsample (1/2 -> 1/4 -> 1/8 -> 1/16)
    blit_kawase(
        renderer,
        &pyramid.tex_half_a,
        pyramid.half_size,
        &mut pyramid.tex_quarter_a,
        pyramid.quarter_size,
        down_prog.clone(),
        2.0,
    )?;
    blit_kawase(
        renderer,
        &pyramid.tex_quarter_a,
        pyramid.quarter_size,
        &mut pyramid.tex_eighth_a,
        pyramid.eighth_size,
        down_prog.clone(),
        2.5,
    )?;
    blit_kawase(
        renderer,
        &pyramid.tex_eighth_a,
        pyramid.eighth_size,
        &mut pyramid.tex_sixteenth,
        pyramid.sixteenth_size,
        down_prog.clone(),
        3.0,
    )?;

    // 3. Multi-pass Upsample (1/16 -> 1/8 -> 1/4 -> 1/2)
    blit_kawase(
        renderer,
        &pyramid.tex_sixteenth,
        pyramid.sixteenth_size,
        &mut pyramid.tex_eighth_b,
        pyramid.eighth_size,
        up_prog.clone(),
        3.0,
    )?;
    blit_kawase(
        renderer,
        &pyramid.tex_eighth_b,
        pyramid.eighth_size,
        &mut pyramid.tex_quarter_b,
        pyramid.quarter_size,
        up_prog.clone(),
        2.5,
    )?;
    blit_kawase(
        renderer,
        &pyramid.tex_quarter_b,
        pyramid.quarter_size,
        &mut pyramid.tex_half_b,
        pyramid.half_size,
        up_prog.clone(),
        2.0,
    )?;

    Some(())
}

/// Captures `bg_elements` into an offscreen FBO and runs the 6-pass Dual-Kawase blur
/// unconditionally on every frame.
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

    let Some((down_prog, up_prog, _)) = ensure_shaders(renderer) else {
        return;
    };

    let out_name = output.name();
    BLUR_CACHE.with(|cache_cell| {
        let mut cache_map = cache_cell.borrow_mut();
        let entry = cache_map
            .entry(out_name)
            .or_insert_with(|| OutputBlurCache::new(phys_size, logical_size));

        if entry.phys_size != phys_size || entry.logical_size != logical_size {
            entry.phys_size = phys_size;
            entry.logical_size = logical_size;
            entry.pyramid = None;
            entry.blurred_tex = None;
        }

        if entry.pyramid.is_none() {
            entry.pyramid = create_pyramid(renderer, phys_size);
        }
        let Some(pyramid) = entry.pyramid.as_mut() else {
            return;
        };

        if run_dual_kawase_passes(
            renderer,
            pyramid,
            &down_prog,
            &up_prog,
            bg_elements,
            logical_size,
            clear_color,
        )
        .is_some()
        {
            entry.blurred_tex = Some(pyramid.tex_half_b.clone());
        }
    });
}

/// Returns a Dual-Kawase frosted-glass backdrop element for `rect` clipped with an SDF rounded-rect mask,
/// tonal tint (~0.48), anti-banding noise grain, and a subtle 1px bright edge.
#[allow(clippy::too_many_arguments)]
pub fn blur_panel_element(
    renderer: &mut GlesRenderer,
    output: &Output,
    scale: Scale<f64>,
    _key: &'static str,
    rect: Rectangle<i32, Logical>,
    radius: f32,
    tint_rgb: [u8; 3],
    tint_alpha: f32,
    edge_alpha: f32,
    alpha: f32,
) -> Option<OkayRenderElements> {
    if rect.size.w <= 0 || rect.size.h <= 0 || alpha <= 0.005 {
        return None;
    }

    let Some((_, _, composite_prog)) = ensure_shaders(renderer) else {
        return None;
    };

    let out_name = output.name();
    let (tex, out_logical) = BLUR_CACHE.with(|cache_cell| {
        let cache_map = cache_cell.borrow();
        let entry = cache_map.get(&out_name)?;
        Some((entry.blurred_tex.clone()?, entry.logical_size))
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

    let tint = [
        tint_rgb[0] as f32 / 255.0,
        tint_rgb[1] as f32 / 255.0,
        tint_rgb[2] as f32 / 255.0,
    ];

    Some(OkayRenderElements::TexShader(TextureShaderElement::new(
        tex_elem,
        composite_prog,
        vec![
            Uniform::new("u_pos", [rect.loc.x as f32, rect.loc.y as f32]),
            Uniform::new("u_size", [rect.size.w as f32, rect.size.h as f32]),
            Uniform::new(
                "u_out_size",
                [out_logical.w as f32, out_logical.h as f32],
            ),
            Uniform::new("u_radius", radius),
            Uniform::new("u_tint", tint),
            Uniform::new("u_tint_alpha", tint_alpha),
            Uniform::new("u_grain", 0.020),
            Uniform::new("u_edge_alpha", edge_alpha),
        ],
    )))
}
