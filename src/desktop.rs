use std::{
    collections::HashMap,
    fs::File,
    io::BufReader,
    path::PathBuf,
};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                Kind,
            },
            gles::GlesRenderer,
        },
    },
    output::Output,
    utils::{Physical, Point, Scale, Size, Transform},
};

use crate::render::OkayRenderElements;

#[derive(Debug)]
struct RawWallpaper {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Manages desktop background surfaces (wallpaper image or dark gradient) per output.
#[derive(Debug, Default)]
pub struct DesktopState {
    wallpaper: Option<RawWallpaper>,
    attempted_load: bool,
    cached_buffers: HashMap<String, (Size<i32, Physical>, MemoryRenderBuffer)>,
}

impl DesktopState {
    pub fn new() -> Self {
        Self::default()
    }

    fn ensure_wallpaper_loaded(&mut self) {
        if self.attempted_load {
            return;
        }
        self.attempted_load = true;

        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let path = PathBuf::from(home)
            .join(".config")
            .join("okay-de")
            .join("wallpaper.png");

        if !path.is_file() {
            return;
        }

        match load_png_rgba(&path) {
            Ok(wp) => {
                tracing::info!(?path, width = wp.width, height = wp.height, "Loaded desktop wallpaper");
                self.wallpaper = Some(wp);
            }
            Err(err) => {
                tracing::warn!(?path, ?err, "Failed to decode wallpaper.png, using default gradient");
            }
        }
    }

    /// Reloads `~/.config/okay-de/wallpaper.png` and invalidates cached per-output buffers.
    pub fn reload(&mut self) {
        self.attempted_load = false;
        self.wallpaper = None;
        self.cached_buffers.clear();
        tracing::info!("Reloaded desktop wallpaper configuration");
    }

    /// Returns a cached `MemoryRenderBufferRenderElement` covering the given `output`.
    pub fn background_element(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        scale: Scale<f64>,
    ) -> Option<OkayRenderElements<GlesRenderer>> {
        self.ensure_wallpaper_loaded();

        let mode = output.current_mode()?;
        let phys_size: Size<i32, Physical> = mode.size;
        if phys_size.w <= 0 || phys_size.h <= 0 {
            return None;
        }

        let logical_size = phys_size.to_f64().to_logical(scale).to_i32_round();
        let output_name = output.name();

        let needs_rebuild = match self.cached_buffers.get(&output_name) {
            Some((cached_sz, _)) => *cached_sz != phys_size,
            None => true,
        };

        if needs_rebuild {
            let pixels = if let Some(ref wp) = self.wallpaper {
                scale_cover_rgba(wp, phys_size.w as u32, phys_size.h as u32)
            } else {
                generate_dark_gradient_rgba(phys_size.w as u32, phys_size.h as u32)
            };

            let buf = MemoryRenderBuffer::from_slice(
                &pixels,
                Fourcc::Abgr8888,
                (phys_size.w, phys_size.h),
                1,
                Transform::Normal,
                None,
            );
            self.cached_buffers.insert(output_name.clone(), (phys_size, buf));
        }

        let (_, buf) = self.cached_buffers.get(&output_name)?;
        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            Point::<f64, Physical>::from((0.0, 0.0)),
            buf,
            None,
            None,
            Some(logical_size),
            Kind::Unspecified,
        ) {
            Ok(elem) => Some(OkayRenderElements::Memory(elem)),
            Err(err) => {
                tracing::warn!(?err, "Failed to import desktop background buffer");
                None
            }
        }
    }
}

fn load_png_rgba(path: &PathBuf) -> Result<RawWallpaper, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let mut decoder = png::Decoder::new(BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    let raw = &buf[..info.buffer_size()];

    let rgba = match info.color_type {
        png::ColorType::Rgba => raw.to_vec(),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity((info.width * info.height * 4) as usize);
            for chunk in raw.chunks_exact(3) {
                out.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity((info.width * info.height * 4) as usize);
            for chunk in raw.chunks_exact(2) {
                out.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
            }
            out
        }
        png::ColorType::Grayscale => {
            let mut out = Vec::with_capacity((info.width * info.height * 4) as usize);
            for &g in raw {
                out.extend_from_slice(&[g, g, g, 255]);
            }
            out
        }
        png::ColorType::Indexed => return Err("Unsupported indexed PNG".into()),
    };

    Ok(RawWallpaper {
        width: info.width,
        height: info.height,
        rgba,
    })
}

/// Scales a source RGBA image to cover `(dst_w, dst_h)` with aspect-ratio preservation.
fn scale_cover_rgba(wp: &RawWallpaper, dst_w: u32, dst_h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (dst_w * dst_h * 4) as usize];
    if wp.width == 0 || wp.height == 0 {
        return out;
    }

    let scale_x = dst_w as f64 / wp.width as f64;
    let scale_y = dst_h as f64 / wp.height as f64;
    let scale = scale_x.max(scale_y);

    let scaled_w = wp.width as f64 * scale;
    let scaled_h = wp.height as f64 * scale;
    let offset_x = (scaled_w - dst_w as f64) * 0.5;
    let offset_y = (scaled_h - dst_h as f64) * 0.5;

    for y in 0..dst_h {
        let sy = (((y as f64 + offset_y) / scale) as u32).min(wp.height - 1);
        let row_src = (sy * wp.width) as usize;
        let row_dst = (y * dst_w) as usize;
        for x in 0..dst_w {
            let sx = (((x as f64 + offset_x) / scale) as u32).min(wp.width - 1);
            let s_idx = (row_src + sx as usize) * 4;
            let d_idx = (row_dst + x as usize) * 4;
            out[d_idx..d_idx + 4].copy_from_slice(&wp.rgba[s_idx..s_idx + 4]);
        }
    }
    out
}

/// Generates a macOS Sonoma/Sequoia-inspired multi-stop aurora gradient with subtle dithering.
fn generate_dark_gradient_rgba(w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    let wf = w.max(1) as f32;
    let hf = h.max(1) as f32;

    for y in 0..h {
        let ny = y as f32 / hf;
        for x in 0..w {
            let nx = x as f32 / wf;

            // Deep midnight base gradient (top-left obsidian to bottom-right deep indigo)
            let base_t = 0.55 * ny + 0.45 * nx;
            let mut r = 11.0 + 14.0 * base_t;
            let mut g = 14.0 + 12.0 * base_t;
            let mut b = 26.0 + 28.0 * base_t;

            // Primary Sonoma/Sequoia sweeping teal-blue aurora wave
            let wave_y = 0.46 + 0.12 * (nx * 4.2 - 0.8).sin() + 0.05 * (nx * 8.5).cos();
            let d_wave = ((ny - wave_y) * 3.1).abs();
            let teal_band = (-d_wave * d_wave * 3.8).exp() * (0.45 + 0.55 * (1.0 - nx * 0.4));
            r += 14.0 * teal_band;
            g += 68.0 * teal_band;
            b += 108.0 * teal_band;

            // Warm magenta-rose & violet aurora ribbon
            let rose_y = 0.34 - 0.10 * (nx * 3.4 + 0.6).cos();
            let d_rose = ((ny - rose_y) * 3.5).abs();
            let rose_band = (-d_rose * d_rose * 4.2).exp() * (0.35 + 0.65 * nx);
            r += 96.0 * rose_band;
            g += 28.0 * rose_band;
            b += 86.0 * rose_band;

            // Soft horizon amber/coral glow near lower-left
            let dx_a = nx - 0.22;
            let dy_a = ny - 0.78;
            let warm_glow = (-(dx_a * dx_a * 3.2 + dy_a * dy_a * 5.0)).exp();
            r += 48.0 * warm_glow;
            g += 24.0 * warm_glow;
            b += 36.0 * warm_glow;

            // Subtle ordered dither to prevent 8-bit gradient banding
            let dither = (((x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263)) & 3) as f32 - 1.5) * 0.45;

            let idx = ((y * w + x) * 4) as usize;
            out[idx] = (r + dither).clamp(0.0, 255.0) as u8;
            out[idx + 1] = (g + dither).clamp(0.0, 255.0) as u8;
            out[idx + 2] = (b + dither).clamp(0.0, 255.0) as u8;
            out[idx + 3] = 255;
        }
    }
    out
}
