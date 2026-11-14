use std::{
    collections::HashMap,
    time::Instant,
};

use smithay::{
    backend::renderer::{
        element::{
            solid::{SolidColorBuffer, SolidColorRenderElement},
            Kind,
        },
        gles::GlesRenderer,
        Color32F,
    },
    reexports::wayland_server::backend::ObjectId,
    utils::{Logical, Physical, Point, Rectangle, Scale},
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

    /// Renders the **Magic Lamp (Genie)** multi-slice funnel mesh (or Scale/Glide card) for a window
    /// that is mid-minimize or mid-unminimize!
    pub fn render_minimize_effect(
        &self,
        id: &ObjectId,
        scale: Scale<f64>,
    ) -> Vec<OkayRenderElements<GlesRenderer>> {
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
            // Soft-focus blur halo pass during minimize/unminimize
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

        // Magic Lamp (Genie) multi-slice S-curve funnel deformation + lightweight soft-focus blur pass!
        // Divides the window into 28 horizontal slices. Lower slices get pulled horizontally
        // into the dock icon neck first, while upper slices cascade down the Genie funnel.
        const SLICES: usize = 28;
        let mut elems = Vec::with_capacity(SLICES * 2);

        // Soft-focus blur intensity peaks mid-flight (`t * (1 - t) * 4`)
        let blur_intensity = (t * (1.0 - t) * 4.0).clamp(0.0, 1.0);
        let blur_spread_x = (4.0 + 16.0 * blur_intensity).round() as i32;

        let top_y = lerp(src.loc.y as f32, dst.loc.y as f32, (t * 1.15 - 0.15).clamp(0.0, 1.0));
        let bot_y = lerp((src.loc.y + src.size.h) as f32, (dst.loc.y + dst.size.h) as f32, (t * 1.35).clamp(0.0, 1.0));
        let total_h = (bot_y - top_y).max(14.0);
        let slice_h = (total_h / SLICES as f32).ceil().max(2.0) as i32;

        for i in 0..SLICES {
            // `v` goes from 0.0 (top of window) to 1.0 (bottom of window nearest the dock)
            let v = i as f32 / (SLICES - 1) as f32;
            let sy = (top_y + v * total_h).round() as i32;

            // Genie funnel factor: bottom slices pinch toward the dock icon immediately (`v` high),
            // while top slices retain window width early and then funnel smoothly in the second half of `t`.
            let pinch_phase = ((t * 1.45) + v * 0.75 - 0.25).clamp(0.0, 1.0);
            let s_curve = smoothstep(pinch_phase);

            let src_left = src.loc.x as f32;
            let src_right = (src.loc.x + src.size.w) as f32;
            let dst_left = dst.loc.x as f32;
            let dst_right = (dst.loc.x + dst.size.w) as f32;

            let left_x = lerp(src_left, dst_left, s_curve).round() as i32;
            let right_x = lerp(src_right, dst_right, s_curve).round() as i32;
            let sw = (right_x - left_x).max(8);

            let is_title_slice = i <= 1;
            let r = if is_title_slice {
                0.24 + (accent[0] as f32 / 255.0) * 0.35
            } else {
                lerp(0.11, 0.18 + (accent[0] as f32 / 255.0) * 0.24, v * 0.7 + blur_intensity * 0.3)
            };
            let g = if is_title_slice {
                0.26 + (accent[1] as f32 / 255.0) * 0.35
            } else {
                lerp(0.12, 0.20 + (accent[1] as f32 / 255.0) * 0.24, v * 0.7 + blur_intensity * 0.3)
            };
            let b = if is_title_slice {
                0.34 + (accent[2] as f32 / 255.0) * 0.40
            } else {
                lerp(0.18, 0.28 + (accent[2] as f32 / 255.0) * 0.30, v * 0.7 + blur_intensity * 0.3)
            };
            let alpha = (0.86 - t * 0.34).clamp(0.22, 0.88);

            // Fast GPU soft-focus blur halo pass behind each deforming slice
            let blur_alpha = alpha * (0.14 + 0.16 * blur_intensity);
            let blur_buf = SolidColorBuffer::new(
                (sw + blur_spread_x * 2, slice_h + 4),
                Color32F::new(r * 1.15, g * 1.15, b * 1.20, blur_alpha),
            );
            let blur_phys: Point<i32, Physical> =
                Point::<i32, Logical>::from((left_x - blur_spread_x, sy - 2))
                    .to_physical_precise_round(scale);
            elems.push(OkayRenderElements::Solid(
                SolidColorRenderElement::from_buffer(
                    &blur_buf,
                    blur_phys,
                    scale,
                    blur_alpha,
                    Kind::Unspecified,
                ),
            ));

            // Core frosted slice
            let slice_buf = SolidColorBuffer::new((sw, slice_h), Color32F::new(r, g, b, alpha));
            let phys_loc: Point<i32, Physical> =
                Point::<i32, Logical>::from((left_x, sy)).to_physical_precise_round(scale);
            elems.push(OkayRenderElements::Solid(
                SolidColorRenderElement::from_buffer(
                    &slice_buf,
                    phys_loc,
                    scale,
                    alpha,
                    Kind::Unspecified,
                ),
            ));
        }

        elems
    }
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
fn smoothstep(x: f32) -> f32 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
