use std::{
    cell::RefCell,
    collections::HashMap,
    time::Instant,
};

use smithay::{
    backend::renderer::{
        element::{
            solid::{SolidColorBuffer, SolidColorRenderElement},
            texture::TextureRenderElement,
            Id, Kind,
        },
        gles::{
            element::{PixelShaderElement, TextureShaderElement},
            GlesPixelProgram, GlesRenderer, GlesTexProgram, Uniform, UniformName, UniformType,
        },
        utils::DamageSnapshot,
        Color32F, Renderer,
    },
    reexports::wayland_server::backend::ObjectId,
    utils::{Logical, Physical, Point, Rectangle, Scale, Size, Transform},
};

use crate::render::OkayRenderElements;

/// Minimize / unminimize animation style selectable in `OKay Settings -> Appearance`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinimizeEffect {
    /// Multi-slice Genie / Magic Lamp funnel deformation into the taskbar button.
    MagicLamp,
    /// Smooth scale + glide toward the taskbar button.
    ScaleGlide,
    /// Instant minimize without animation.
    Off,
}

impl MinimizeEffect {
    pub fn label(self) -> &'static str {
        match self {
            Self::MagicLamp => "Magic Lamp (Genie)",
            Self::ScaleGlide => "Scale & Glide",
            Self::Off => "Off (Instant)",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::MagicLamp => Self::ScaleGlide,
            Self::ScaleGlide => Self::Off,
            Self::Off => Self::MagicLamp,
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "scaleglide" | "scale" => Self::ScaleGlide,
            "off" | "none" => Self::Off,
            _ => Self::MagicLamp,
        }
    }

    pub fn as_config_str(self) -> &'static str {
        match self {
            Self::MagicLamp => "magiclamp",
            Self::ScaleGlide => "scaleglide",
            Self::Off => "off",
        }
    }
}

/// Per-window animation state (open scale/fade, Magic Lamp minimize/restore, smooth tile/snap move).
#[derive(Debug, Clone)]
pub struct WindowAnimation {
    /// `0.0 -> 1.0` when opening a new window.
    pub open_progress: f32,
    /// `0.0` = normal, `1.0` = fully minimized into taskbar button.
    pub minimize_progress: f32,
    /// Direction of minimize transition (`true` = minimizing to 1.0, `false` = restoring to 0.0).
    pub minimizing: bool,
    /// Origin window rectangle in output-local logical coordinates when minimize started.
    pub start_rect: Rectangle<i32, Logical>,
    /// Target taskbar/dock button rectangle in output-local logical coordinates.
    pub target_button_rect: Rectangle<i32, Logical>,
    /// Accent color for the window's Magic Lamp mesh preview.
    pub accent_rgb: [u8; 3],
    /// Smoothly interpolated position offset for tiling / snapping transitions.
    pub move_offset: Point<f64, Logical>,
}

impl Default for WindowAnimation {
    fn default() -> Self {
        Self {
            open_progress: 0.0,
            minimize_progress: 0.0,
            minimizing: false,
            start_rect: Rectangle::from_size((800, 500).into()),
            target_button_rect: Rectangle::new((320, 700).into(), (120, 32).into()),
            accent_rgb: [201, 160, 220],
            move_offset: Point::from((0.0, 0.0)),
        }
    }
}

/// Central 60+ FPS animation controller for windows, workspaces, and popups.
#[derive(Debug)]
pub struct AnimationManager {
    pub enabled: bool,
    pub speed: f32,
    pub minimize_effect: MinimizeEffect,
    pub last_tick: Instant,
    pub windows: HashMap<ObjectId, WindowAnimation>,
    /// Exact global logical rectangle of each window's taskbar/dock button (`ObjectId -> Rect`).
    pub taskbar_button_rects: HashMap<ObjectId, Rectangle<i32, Logical>>,
    /// Horizontal slide offset in logical pixels when switching workspaces (`1..9`).
    pub workspace_slide_x: f64,
    /// Smoothly animated popup progress (`0.0 .. 1.0`) for Start Menu, Calendar, Notifications, Clipboard.
    pub launcher_open_anim: f32,
    pub calendar_open_anim: f32,
    pub notif_open_anim: f32,
    pub clipboard_open_anim: f32,
    /// Active dock icon bounce timestamps (`key -> started_at`).
    pub dock_bounces: HashMap<String, Instant>,
}

impl Default for AnimationManager {
    fn default() -> Self {
        Self {
            enabled: true,
            speed: 1.0,
            minimize_effect: MinimizeEffect::MagicLamp,
            last_tick: Instant::now(),
            windows: HashMap::new(),
            taskbar_button_rects: HashMap::new(),
            workspace_slide_x: 0.0,
            launcher_open_anim: 0.0,
            calendar_open_anim: 0.0,
            notif_open_anim: 0.0,
            clipboard_open_anim: 0.0,
            dock_bounces: HashMap::new(),
        }
    }
}

impl AnimationManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Triggers a macOS-style parabolic bounce on a Dock icon when launching an app.
    pub fn trigger_dock_bounce(&mut self, key: &str) {
        if self.enabled {
            self.dock_bounces.insert(key.to_string(), Instant::now());
        }
    }

    /// Returns the upward Y bounce offset (in logical px, >= 0) for a Dock icon.
    pub fn dock_bounce_offset(&self, key: &str) -> i32 {
        let Some(started) = self.dock_bounces.get(key) else {
            return 0;
        };
        let elapsed = started.elapsed().as_secs_f32() * self.speed.clamp(0.4, 2.5);
        if elapsed >= 0.72 {
            return 0;
        }
        // Two decaying parabolic hops
        let decay = (1.0 - elapsed / 0.72).max(0.0);
        let hop = (elapsed * std::f32::consts::PI * 2.8).sin().abs();
        (hop * 14.0 * decay).round() as i32
    }

    /// Registers a newly mapped window so it plays a smooth open transition.
    pub fn on_window_opened(&mut self, id: ObjectId, accent_rgb: [u8; 3]) {
        let mut anim = WindowAnimation::default();
        anim.open_progress = if self.enabled { 0.0 } else { 1.0 };
        anim.accent_rgb = accent_rgb;
        self.windows.insert(id, anim);
    }

    /// Records a smooth position delta when a window is tiled, snapped, or maximized.
    pub fn on_window_moved(&mut self, id: ObjectId, old_loc: Point<i32, Logical>, new_loc: Point<i32, Logical>) {
        if !self.enabled {
            return;
        }
        let dx = (old_loc.x - new_loc.x) as f64;
        let dy = (old_loc.y - new_loc.y) as f64;
        if dx.abs() > 2.0 || dy.abs() > 2.0 {
            let entry = self.windows.entry(id).or_default();
            entry.move_offset = Point::from((dx, dy));
        }
    }

    /// Starts a workspace horizontal slide transition when switching from `old_ws` to `new_ws`.
    pub fn on_workspace_switched(&mut self, old_ws: usize, new_ws: usize, output_width: i32) {
        if !self.enabled || old_ws == new_ws {
            return;
        }
        let dir = if new_ws > old_ws { 1.0 } else { -1.0 };
        self.workspace_slide_x = dir * (output_width as f64 * 0.28);
    }

    /// Triggers the Magic Lamp (Genie) or Scale/Glide minimize animation for `id`.
    pub fn start_minimize(
        &mut self,
        id: ObjectId,
        win_rect_local: Rectangle<i32, Logical>,
        output_loc: Point<i32, Logical>,
        output_size: smithay::utils::Size<i32, Logical>,
        accent_rgb: [u8; 3],
    ) {
        let target_global = self
            .taskbar_button_rects
            .get(&id)
            .copied()
            .unwrap_or_else(|| {
                Rectangle::new(
                    (output_loc.x + output_size.w / 2 - 60, output_loc.y + output_size.h - 38).into(),
                    (120, 32).into(),
                )
            });
        let target_local = Rectangle::new(
            target_global.loc - output_loc,
            target_global.size,
        );

        let entry = self.windows.entry(id).or_default();
        entry.start_rect = win_rect_local;
        entry.target_button_rect = target_local;
        entry.accent_rgb = accent_rgb;
        entry.minimizing = true;
        if !self.enabled || self.minimize_effect == MinimizeEffect::Off {
            entry.minimize_progress = 1.0;
        } else if entry.minimize_progress >= 1.0 {
            entry.minimize_progress = 0.0;
        }
    }

    /// Triggers the reverse Magic Lamp (Genie) unminimize/restore animation from the taskbar button.
    pub fn start_unminimize(
        &mut self,
        id: ObjectId,
        win_rect_local: Rectangle<i32, Logical>,
        output_loc: Point<i32, Logical>,
        output_size: smithay::utils::Size<i32, Logical>,
        accent_rgb: [u8; 3],
    ) {
        let target_global = self
            .taskbar_button_rects
            .get(&id)
            .copied()
            .unwrap_or_else(|| {
                Rectangle::new(
                    (output_loc.x + output_size.w / 2 - 60, output_loc.y + output_loc.y + output_size.h - 38).into(),
                    (120, 32).into(),
                )
            });
        let target_local = Rectangle::new(
            target_global.loc - output_loc,
            target_global.size,
        );

        let entry = self.windows.entry(id).or_default();
        entry.start_rect = win_rect_local;
        entry.target_button_rect = target_local;
        entry.accent_rgb = accent_rgb;
        entry.minimizing = false;
        if !self.enabled || self.minimize_effect == MinimizeEffect::Off {
            entry.minimize_progress = 0.0;
        } else if entry.minimize_progress <= 0.0 {
            entry.minimize_progress = 1.0;
        }
    }

    /// Advances all active window, workspace, and popup animations for the current frame.
    pub fn tick(
        &mut self,
        launcher_open: bool,
        calendar_open: bool,
        notif_open: bool,
        clipboard_open: bool,
    ) {
        let now = Instant::now();
        let raw_dt = now.duration_since(self.last_tick).as_secs_f32().clamp(0.001, 0.050);
        self.last_tick = now;

        if !self.enabled {
            self.workspace_slide_x = 0.0;
            self.launcher_open_anim = if launcher_open { 1.0 } else { 0.0 };
            self.calendar_open_anim = if calendar_open { 1.0 } else { 0.0 };
            self.notif_open_anim = if notif_open { 1.0 } else { 0.0 };
            self.clipboard_open_anim = if clipboard_open { 1.0 } else { 0.0 };
            for anim in self.windows.values_mut() {
                anim.open_progress = 1.0;
                anim.move_offset = Point::from((0.0, 0.0));
                anim.minimize_progress = if anim.minimizing { 1.0 } else { 0.0 };
            }
            return;
        }

        self.dock_bounces
            .retain(|_, started| started.elapsed().as_secs_f32() < 0.85);

        let dt = raw_dt * self.speed.clamp(0.35, 3.0);

        // Exponential spring decay for workspace slide
        let decay = (-14.0 * dt as f64).exp();
        self.workspace_slide_x *= decay;
        if self.workspace_slide_x.abs() < 0.5 {
            self.workspace_slide_x = 0.0;
        }

        // Popup animations
        step_towards(&mut self.launcher_open_anim, if launcher_open { 1.0 } else { 0.0 }, dt * 6.5);
        step_towards(&mut self.calendar_open_anim, if calendar_open { 1.0 } else { 0.0 }, dt * 6.5);
        step_towards(&mut self.notif_open_anim, if notif_open { 1.0 } else { 0.0 }, dt * 6.5);
        step_towards(&mut self.clipboard_open_anim, if clipboard_open { 1.0 } else { 0.0 }, dt * 6.5);

        for anim in self.windows.values_mut() {
            if anim.open_progress < 1.0 {
                anim.open_progress = (anim.open_progress + dt * 5.0).min(1.0);
            }

            anim.move_offset.x *= decay;
            anim.move_offset.y *= decay;
            if anim.move_offset.x.abs() < 0.5 {
                anim.move_offset.x = 0.0;
            }
            if anim.move_offset.y.abs() < 0.5 {
                anim.move_offset.y = 0.0;
            }

            // Magic Lamp / ScaleGlide minimize & restore speed
            let min_rate = match self.minimize_effect {
                MinimizeEffect::MagicLamp => 2.4,
                MinimizeEffect::ScaleGlide => 3.6,
                MinimizeEffect::Off => 100.0,
            };
            if anim.minimizing {
                if anim.minimize_progress < 1.0 {
                    anim.minimize_progress = (anim.minimize_progress + dt * min_rate).min(1.0);
                }
            } else if anim.minimize_progress > 0.0 {
                anim.minimize_progress = (anim.minimize_progress - dt * min_rate).max(0.0);
            }
        }
    }

    /// Returns `true` if `id` is currently playing a Magic Lamp / ScaleGlide minimize or restore animation.
    pub fn is_animating_minimize(&self, id: &ObjectId) -> bool {
        if let Some(anim) = self.windows.get(id) {
            anim.minimize_progress > 0.001 && anim.minimize_progress < 0.999
        } else {
            false
        }
    }

    /// Renders the **Magic Lamp (Genie)** continuous GLSL fragment UV-warp (or Scale/Glide card)
    /// for a window that is mid-minimize or mid-unminimize — zero stair-stepping!
    pub fn render_minimize_effect(
        &self,
        id: &ObjectId,
        renderer: &mut GlesRenderer,
        scale: Scale<f64>,
    ) -> Vec<OkayRenderElements> {
        let Some(anim) = self.windows.get(id) else {
            return Vec::new();
        };
        if anim.minimize_progress <= 0.001 || anim.minimize_progress >= 0.999 {
            return Vec::new();
        }

        let t = cubic_ease_in_out(anim.minimize_progress);
        let src = anim.start_rect;
        let dst = anim.target_button_rect;
        let accent = anim.accent_rgb;

        if self.minimize_effect == MinimizeEffect::ScaleGlide {
            let cur_x = lerp(src.loc.x as f32, dst.loc.x as f32, t).round() as i32;
            let cur_y = lerp(src.loc.y as f32, dst.loc.y as f32, t).round() as i32;
            let cur_w = lerp(src.size.w as f32, dst.size.w as f32, t).round().max(16.0) as i32;
            let cur_h = lerp(src.size.h as f32, dst.size.h as f32, t).round().max(12.0) as i32;
            let alpha = (1.0 - t * 0.65).clamp(0.15, 0.88);
            let blur_pad = (6.0 + 18.0 * (t * (1.0 - t) * 4.0)).round() as i32;

            let mut out = Vec::with_capacity(3);
            for (pad, a_mul) in [(blur_pad, 0.16), (blur_pad / 2, 0.24)] {
                let halo_buf = SolidColorBuffer::new(
                    (cur_w + pad * 2, cur_h + pad * 2),
                    Color32F::new(0.18, 0.22, 0.34, alpha * a_mul),
                );
                let halo_loc: Point<i32, Physical> =
                    Point::<i32, Logical>::from((cur_x - pad, cur_y - pad)).to_physical_precise_round(scale);
                out.push(OkayRenderElements::Solid(
                    SolidColorRenderElement::from_buffer(&halo_buf, halo_loc, scale, alpha * a_mul, Kind::Unspecified),
                ));
            }
            let buf = SolidColorBuffer::new(
                (cur_w, cur_h),
                Color32F::new(
                    0.12 + (accent[0] as f32 / 255.0) * 0.25,
                    0.14 + (accent[1] as f32 / 255.0) * 0.25,
                    0.20 + (accent[2] as f32 / 255.0) * 0.30,
                    alpha,
                ),
            );
            let phys_loc: Point<i32, Physical> =
                Point::<i32, Logical>::from((cur_x, cur_y)).to_physical_precise_round(scale);
            out.push(OkayRenderElements::Solid(
                SolidColorRenderElement::from_buffer(&buf, phys_loc, scale, alpha, Kind::Unspecified),
            ));
            return out;
        }

        // Continuous full-bounds quad with GLSL cubic-Bezier UV warp + velocity-based minimize blur
        let bbox_x = src.loc.x.min(dst.loc.x) - 16;
        let bbox_y = src.loc.y.min(dst.loc.y) - 8;
        let bbox_r = (src.loc.x + src.size.w).max(dst.loc.x + dst.size.w) + 16;
        let bbox_b = (src.loc.y + src.size.h).max(dst.loc.y + dst.size.h) + 8;
        let bbox_w = (bbox_r - bbox_x).max(16);
        let bbox_h = (bbox_b - bbox_y).max(16);
        let area: Rectangle<i32, Logical> = Rectangle::new((bbox_x, bbox_y).into(), (bbox_w, bbox_h).into());

        // Normalized rect coords inside the bounding box [0..1]
        let src_x0 = (src.loc.x - bbox_x) as f32;
        let src_y0 = (src.loc.y - bbox_y) as f32;
        let src_x1 = (src.loc.x + src.size.w - bbox_x) as f32;
        let src_y1 = (src.loc.y + src.size.h - bbox_y) as f32;
        let dst_x0 = (dst.loc.x - bbox_x) as f32;
        let dst_y0 = (dst.loc.y - bbox_y) as f32;
        let dst_x1 = (dst.loc.x + dst.size.w - bbox_x) as f32;
        let dst_y1 = (dst.loc.y + dst.size.h - bbox_y) as f32;
        let velocity_blur = (t * (1.0 - t) * 4.0).clamp(0.0, 1.0);
        let alpha = (1.0 - t * 0.28).clamp(0.25, 1.0);

        if let Some(tex) = crate::render::get_cached_window_texture(id) {
            const GENIE_TEX_FRAG: &str = r#"
//_DEFINES
precision mediump float;
varying vec2 v_coords;
uniform float alpha;
uniform vec2 u_bbox_size;
uniform vec4 u_src;
uniform vec4 u_dst;
uniform float u_t;
uniform float u_blur;
#ifdef EXTERNAL
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

float genieCurve(float x) {
    float c = clamp(x, 0.0, 1.0);
    return c * c * (3.0 - 2.0 * c);
}

void main() {
    vec2 px = v_coords * u_bbox_size;

    float top_y = mix(u_src.y, u_dst.y, clamp(u_t * 1.15 - 0.15, 0.0, 1.0));
    float bot_y = mix(u_src.w, u_dst.w, clamp(u_t * 1.35, 0.0, 1.0));
    float span_y = max(bot_y - top_y, 2.0);
    float v = (px.y - top_y) / span_y;

    float pinch = clamp(u_t * 1.45 + clamp(v, 0.0, 1.0) * 0.75 - 0.25, 0.0, 1.0);
    float s = genieCurve(pinch);
    float left_x = mix(u_src.x, u_dst.x, s);
    float right_x = mix(u_src.z, u_dst.z, s);
    float span_x = max(right_x - left_x, 2.0);
    float u = (px.x - left_x) / span_x;

    // Smooth anti-aliased boundary mask (zero stair-stepping)
    float mask_x = smoothstep(left_x - 1.2, left_x + 0.6, px.x) *
                   (1.0 - smoothstep(right_x - 0.6, right_x + 1.2, px.x));
    float mask_y = smoothstep(top_y - 1.0, top_y + 0.5, px.y) *
                   (1.0 - smoothstep(bot_y - 0.5, bot_y + 1.0, px.y));
    float mask = mask_x * mask_y;
    if (mask <= 0.001) {
        gl_FragColor = vec4(0.0);
        return;
    }

    // Velocity-based vertical & horizontal minimize blur
    float dv = u_blur * 0.014;
    float du = u_blur * 0.006;
    vec4 col = texture2D(tex, clamp(vec2(u, v), 0.0, 1.0)) * 0.36;
    col += texture2D(tex, clamp(vec2(u - du, v - dv), 0.0, 1.0)) * 0.16;
    col += texture2D(tex, clamp(vec2(u + du, v + dv), 0.0, 1.0)) * 0.16;
    col += texture2D(tex, clamp(vec2(u, v - dv * 1.8), 0.0, 1.0)) * 0.16;
    col += texture2D(tex, clamp(vec2(u, v + dv * 1.8), 0.0, 1.0)) * 0.16;

    #ifdef NO_ALPHA
    col.a = 1.0;
    #endif
    gl_FragColor = col * (mask * alpha);
}
"#;

            thread_local! {
                static GENIE_TEX_SHADER: RefCell<Option<GlesTexProgram>> = const { RefCell::new(None) };
            }

            if let Some(prog) = GENIE_TEX_SHADER.with(|cell| {
                let mut borrow = cell.borrow_mut();
                if borrow.is_none() {
                    if let Ok(p) = renderer.compile_custom_texture_shader(
                        GENIE_TEX_FRAG,
                        &[
                            UniformName::new("u_bbox_size", UniformType::_2f),
                            UniformName::new("u_src", UniformType::_4f),
                            UniformName::new("u_dst", UniformType::_4f),
                            UniformName::new("u_t", UniformType::_1f),
                            UniformName::new("u_blur", UniformType::_1f),
                        ],
                    ) {
                        *borrow = Some(p);
                    }
                }
                borrow.clone()
            }) {
                let phys_loc: Point<i32, Physical> =
                    Point::<i32, Logical>::from((bbox_x, bbox_y)).to_physical_precise_round(scale);
                let tex_elem = TextureRenderElement::from_texture_with_damage(
                    Id::new(),
                    renderer.context_id(),
                    phys_loc.to_f64(),
                    tex,
                    scale.x.round().max(1.0) as i32,
                    Transform::Normal,
                    Some(alpha),
                    None,
                    Some(Size::from((bbox_w, bbox_h))),
                    None,
                    DamageSnapshot::empty(),
                    Kind::Unspecified,
                );
                return vec![OkayRenderElements::TexShader(TextureShaderElement::new(
                    tex_elem,
                    prog,
                    vec![
                        Uniform::new("u_bbox_size", [bbox_w as f32, bbox_h as f32]),
                        Uniform::new("u_src", [src_x0, src_y0, src_x1, src_y1]),
                        Uniform::new("u_dst", [dst_x0, dst_y0, dst_x1, dst_y1]),
                        Uniform::new("u_t", t),
                        Uniform::new("u_blur", velocity_blur),
                    ],
                ))];
            }
        }

        // Fallback single-quad continuous GLSL Genie shader (no discrete scanlines)
        const GENIE_PIX_FRAG: &str = r#"
precision mediump float;
varying vec2 v_coords;
uniform vec2 size;
uniform float alpha;
uniform vec4 u_src;
uniform vec4 u_dst;
uniform float u_t;
uniform float u_blur;
uniform vec3 u_accent;

float genieCurve(float x) {
    float c = clamp(x, 0.0, 1.0);
    return c * c * (3.0 - 2.0 * c);
}

void main() {
    vec2 px = v_coords * size;
    float top_y = mix(u_src.y, u_dst.y, clamp(u_t * 1.15 - 0.15, 0.0, 1.0));
    float bot_y = mix(u_src.w, u_dst.w, clamp(u_t * 1.35, 0.0, 1.0));
    float span_y = max(bot_y - top_y, 2.0);
    float v = (px.y - top_y) / span_y;

    float pinch = clamp(u_t * 1.45 + clamp(v, 0.0, 1.0) * 0.75 - 0.25, 0.0, 1.0);
    float s = genieCurve(pinch);
    float left_x = mix(u_src.x, u_dst.x, s);
    float right_x = mix(u_src.z, u_dst.z, s);

    float mask_x = smoothstep(left_x - 1.5, left_x + 0.8, px.x) *
                   (1.0 - smoothstep(right_x - 0.8, right_x + 1.5, px.x));
    float mask_y = smoothstep(top_y - 1.0, top_y + 0.5, px.y) *
                   (1.0 - smoothstep(bot_y - 0.5, bot_y + 1.0, px.y));
    float mask = mask_x * mask_y;

    vec3 col = mix(vec3(0.14, 0.16, 0.24), u_accent * 0.45 + vec3(0.14), v * 0.6 + u_blur * 0.3);
    float a = mask * alpha * 0.85;
    gl_FragColor = vec4(col * a, a);
}
"#;

        thread_local! {
            static GENIE_PIX_SHADER: RefCell<Option<GlesPixelProgram>> = const { RefCell::new(None) };
        }

        if let Some(prog) = GENIE_PIX_SHADER.with(|cell| {
            let mut borrow = cell.borrow_mut();
            if borrow.is_none() {
                if let Ok(p) = renderer.compile_custom_pixel_shader(
                    GENIE_PIX_FRAG,
                    &[
                        UniformName::new("u_src", UniformType::_4f),
                        UniformName::new("u_dst", UniformType::_4f),
                        UniformName::new("u_t", UniformType::_1f),
                        UniformName::new("u_blur", UniformType::_1f),
                        UniformName::new("u_accent", UniformType::_3f),
                    ],
                ) {
                    *borrow = Some(p);
                }
            }
            borrow.clone()
        }) {
            return vec![OkayRenderElements::Shader(PixelShaderElement::new(
                prog,
                area,
                None,
                alpha,
                vec![
                    Uniform::new("u_src", [src_x0, src_y0, src_x1, src_y1]),
                    Uniform::new("u_dst", [dst_x0, dst_y0, dst_x1, dst_y1]),
                    Uniform::new("u_t", t),
                    Uniform::new("u_blur", velocity_blur),
                    Uniform::new(
                        "u_accent",
                        [
                            accent[0] as f32 / 255.0,
                            accent[1] as f32 / 255.0,
                            accent[2] as f32 / 255.0,
                        ],
                    ),
                ],
                Kind::Unspecified,
            ))];
        }

        Vec::new()
    }
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
pub fn cubic_ease_in_out(t: f32) -> f32 {
    let x = t.clamp(0.0, 1.0);
    if x < 0.5 {
        4.0 * x * x * x
    } else {
        1.0 - (-2.0 * x + 2.0).powi(3) / 2.0
    }
}

fn step_towards(val: &mut f32, target: f32, max_delta: f32) {
    if (*val - target).abs() <= max_delta {
        *val = target;
    } else if *val < target {
        *val += max_delta;
    } else {
        *val -= max_delta;
    }
}
