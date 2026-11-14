use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
};

use crate::panel::Canvas;

#[derive(Debug, Clone)]
pub struct DecodedIcon {
    pub width: i32,
    pub height: i32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorIconKind {
    Home,
    Terminal,
    Files,
    Browser,
    Settings,
    SystemMonitor,
    Screenshot,
    TextEditor,
    Folder,
    GenericApp,
}

impl VectorIconKind {
    pub fn from_hint(name: &str, exec: &str, is_dir: bool) -> Self {
        if is_dir {
            return Self::Folder;
        }
        let s = format!("{} {}", name.to_lowercase(), exec.to_lowercase());
        if s.contains("terminal") || s.contains("foot") || s.contains("kitty") || s.contains("alacritty") || s.contains("wezterm") || s.contains("konsole") || s.contains("xterm") {
            Self::Terminal
        } else if s.contains("firefox") || s.contains("chrom") || s.contains("brave") || s.contains("browser") || s.contains("floorp") || s.contains("zen") {
            Self::Browser
        } else if s.contains("thunar") || s.contains("dolphin") || s.contains("nautilus") || s.contains("nemo") || s.contains("pcmanfm") || s.contains("files") {
            Self::Files
        } else if s.contains("settings") || s.contains("control") || s.contains("preferences") {
            Self::Settings
        } else if s.contains("monitor") || s.contains("btop") || s.contains("htop") || s.contains("system") {
            Self::SystemMonitor
        } else if s.contains("screenshot") || s.contains("grim") {
            Self::Screenshot
        } else if s.contains("edit") || s.contains("code") || s.contains("vim") || s.contains("gedit") || s.contains("kate") || s.contains("mousepad") {
            Self::TextEditor
        } else if s == "home" {
            Self::Home
        } else {
            Self::GenericApp
        }
    }
}

static ICON_CACHE: OnceLock<Mutex<HashMap<String, Option<DecodedIcon>>>> = OnceLock::new();

/// Resolves an icon name or path from standard XDG icon themes (`/usr/share/icons/{Papirus,Adwaita,hicolor}`,
/// `/usr/share/pixmaps`, `~/.local/share/icons`), decodes `.png` or rasterizes `.svg` via `rsvg-convert`,
/// scales to `target_size x target_size`, and caches the RGBA buffer.
pub fn load_system_icon(icon_name: &str, target_size: i32) -> Option<DecodedIcon> {
    let trimmed = icon_name.trim();
    if trimmed.is_empty() {
        return None;
    }
    let key = format!("{}@{}", trimmed, target_size);
    let cache = ICON_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some(cached) = guard.get(&key) {
            return cached.clone();
        }
    }

    let loaded = find_icon_path(trimmed).and_then(|p| decode_icon_file(&p, target_size));
    if let Ok(mut guard) = cache.lock() {
        guard.insert(key, loaded.clone());
    }
    loaded
}

fn find_icon_path(icon_name: &str) -> Option<PathBuf> {
    let direct = PathBuf::from(icon_name);
    if direct.is_absolute() && direct.is_file() {
        return Some(direct);
    }

    let mut search_dirs: Vec<PathBuf> = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        let local = PathBuf::from(home).join(".local/share/icons");
        search_dirs.push(local.join("hicolor/48x48/apps"));
        search_dirs.push(local.join("hicolor/scalable/apps"));
    }

    for theme in ["Papirus", "Adwaita", "hicolor"] {
        for sub in [
            "48x48/apps",
            "64x64/apps",
            "128x128/apps",
            "256x256/apps",
            "scalable/apps",
            "48x48/places",
            "scalable/places",
        ] {
            search_dirs.push(PathBuf::from("/usr/share/icons").join(theme).join(sub));
        }
    }
    search_dirs.push(PathBuf::from("/usr/share/pixmaps"));

    let base_name = Path::new(icon_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(icon_name);

    for dir in search_dirs {
        if !dir.is_dir() {
            continue;
        }
        for ext in ["png", "svg"] {
            let candidate = dir.join(format!("{}.{}", base_name, ext));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn decode_icon_file(path: &Path, target_size: i32) -> Option<DecodedIcon> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let raw_png_bytes = if ext == "svg" {
        let sz_str = target_size.max(32).to_string();
        let out = Command::new("rsvg-convert")
            .args(["-w", &sz_str, "-h", &sz_str])
            .arg(path)
            .output()
            .ok()?;
        if !out.status.success() || out.stdout.is_empty() {
            return None;
        }
        out.stdout
    } else if ext == "png" {
        fs::read(path).ok()?
    } else {
        return None;
    };

    let mut decoder = png::Decoder::new(raw_png_bytes.as_slice());
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
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
        png::ColorType::Indexed => return None,
    };

    if info.width == target_size as u32 && info.height == target_size as u32 {
        return Some(DecodedIcon {
            width: target_size,
            height: target_size,
            rgba,
        });
    }

    Some(scale_icon_bilinear(
        &rgba,
        info.width as i32,
        info.height as i32,
        target_size,
        target_size,
    ))
}

fn scale_icon_bilinear(src: &[u8], sw: i32, sh: i32, dw: i32, dh: i32) -> DecodedIcon {
    let mut out = vec![0u8; (dw * dh * 4) as usize];
    if sw <= 0 || sh <= 0 || dw <= 0 || dh <= 0 {
        return DecodedIcon {
            width: dw,
            height: dh,
            rgba: out,
        };
    }
    for y in 0..dh {
        let fy = (y as f32 + 0.5) * (sh as f32 / dh as f32) - 0.5;
        let y0 = (fy.floor() as i32).clamp(0, sh - 1);
        let y1 = (y0 + 1).clamp(0, sh - 1);
        let wy = (fy - y0 as f32).clamp(0.0, 1.0);
        for x in 0..dw {
            let fx = (x as f32 + 0.5) * (sw as f32 / dw as f32) - 0.5;
            let x0 = (fx.floor() as i32).clamp(0, sw - 1);
            let x1 = (x0 + 1).clamp(0, sw - 1);
            let wx = (fx - x0 as f32).clamp(0.0, 1.0);

            let i00 = ((y0 * sw + x0) * 4) as usize;
            let i10 = ((y0 * sw + x1) * 4) as usize;
            let i01 = ((y1 * sw + x0) * 4) as usize;
            let i11 = ((y1 * sw + x1) * 4) as usize;
            let di = ((y * dw + x) * 4) as usize;

            for c in 0..4 {
                let top = src[i00 + c] as f32 * (1.0 - wx) + src[i10 + c] as f32 * wx;
                let bot = src[i01 + c] as f32 * (1.0 - wx) + src[i11 + c] as f32 * wx;
                out[di + c] = (top * (1.0 - wy) + bot * wy).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    DecodedIcon {
        width: dw,
        height: dh,
        rgba: out,
    }
}

/// Blits a decoded RGBA icon onto `canvas` at `(dx, dy)` scaled to `(dw, dh)` with alpha blending.
pub fn blit_icon_to_canvas(canvas: &mut Canvas, dx: i32, dy: i32, dw: i32, dh: i32, icon: &DecodedIcon) {
    if icon.width <= 0 || icon.height <= 0 || dw <= 0 || dh <= 0 {
        return;
    }
    for py in 0..dh {
        let sy = ((py as f32 + 0.5) * (icon.height as f32 / dh as f32)) as i32;
        let sy = sy.clamp(0, icon.height - 1);
        for px in 0..dw {
            let sx = ((px as f32 + 0.5) * (icon.width as f32 / dw as f32)) as i32;
            let sx = sx.clamp(0, icon.width - 1);
            let idx = ((sy * icon.width + sx) * 4) as usize;
            let a = icon.rgba[idx + 3];
            if a > 2 {
                canvas.blend_pixel(
                    dx + px,
                    dy + py,
                    [icon.rgba[idx], icon.rgba[idx + 1], icon.rgba[idx + 2], a],
                );
            }
        }
    }
}

/// Draws either a resolved real system icon (from `/usr/share/icons/...`) or a rich
/// multi-layered vector squircle icon (zero plain two-letter text boxes!).
pub fn draw_app_icon(
    canvas: &mut Canvas,
    x: i32,
    y: i32,
    size: i32,
    icon_name: Option<&str>,
    fallback_kind: VectorIconKind,
) {
    if let Some(name) = icon_name {
        if let Some(decoded) = load_system_icon(name, size.max(32)) {
            blit_icon_to_canvas(canvas, x, y, size, size, &decoded);
            return;
        }
    }
    draw_vector_squircle_icon(canvas, x, y, size, fallback_kind);
}

/// Renders a colorful, multi-layered macOS-style vector squircle icon with custom symbols.
pub fn draw_vector_squircle_icon(
    canvas: &mut Canvas,
    x: i32,
    y: i32,
    size: i32,
    kind: VectorIconKind,
) {
    let s = size.max(18);
    let (top_rgb, bot_rgb) = match kind {
        VectorIconKind::Home => ([72, 178, 255], [32, 114, 224]),
        VectorIconKind::Terminal => ([46, 52, 68], [22, 26, 36]),
        VectorIconKind::Files | VectorIconKind::Folder => ([78, 184, 255], [34, 122, 228]),
        VectorIconKind::Browser => ([66, 176, 252], [44, 96, 232]),
        VectorIconKind::Settings => ([146, 152, 170], [82, 88, 106]),
        VectorIconKind::SystemMonitor => ([136, 104, 242], [78, 52, 186]),
        VectorIconKind::Screenshot => ([238, 112, 158], [176, 58, 116]),
        VectorIconKind::TextEditor => ([246, 186, 84], [216, 128, 38]),
        VectorIconKind::GenericApp => ([96, 148, 246], [54, 92, 202]),
    };

    canvas.fill_squircle_gradient(x, y, s, top_rgb, bot_rgb);

    let cx = x as f32 + s as f32 * 0.5;
    let cy = y as f32 + s as f32 * 0.5;
    let sc = s as f32 / 48.0;

    match kind {
        VectorIconKind::Terminal => {
            // Inner dark screen bezel
            let pad = (6.0 * sc).round() as i32;
            canvas.fill_rounded_rect_aa(x + pad, y + pad, s - pad * 2, s - pad * 2, (5.0 * sc) as i32, [14, 18, 24, 235]);
            // Prompt chevron `>`
            draw_thick_line(canvas, cx - 10.0 * sc, cy - 5.0 * sc, cx - 3.0 * sc, cy, 2.0 * sc, [110, 244, 178, 255]);
            draw_thick_line(canvas, cx - 3.0 * sc, cy, cx - 10.0 * sc, cy + 5.0 * sc, 2.0 * sc, [110, 244, 178, 255]);
            // Prompt cursor `_`
            canvas.fill_rounded_rect_aa(
                (cx + 1.0 * sc).round() as i32,
                (cy + 3.0 * sc).round() as i32,
                (9.0 * sc).round().max(3.0) as i32,
                (2.5 * sc).round().max(2.0) as i32,
                1,
                [240, 246, 255, 240],
            );
        }
        VectorIconKind::Files | VectorIconKind::Folder => {
            // Folder tab + crisp layered body
            let fw = (28.0 * sc).round() as i32;
            let fh = (18.0 * sc).round() as i32;
            let fx = (cx - fw as f32 * 0.5).round() as i32;
            let fy = (cy - fh as f32 * 0.42).round() as i32;
            canvas.fill_rounded_rect_aa(fx, fy - (4.0 * sc).round() as i32, (13.0 * sc).round() as i32, (8.0 * sc).round() as i32, (3.0 * sc) as i32, [215, 238, 255, 245]);
            canvas.fill_rounded_rect_aa(fx, fy, fw, fh, (4.0 * sc) as i32, [242, 248, 255, 245]);
            canvas.fill_rect_blend(fx + 2, fy + 2, fw - 4, (2.0 * sc).round().max(1.0) as i32, [140, 200, 255, 140]);
        }
        VectorIconKind::Browser => {
            // Outer white compass ring + red/white needle
            canvas.fill_circle_aa(cx, cy, 13.5 * sc, [248, 252, 255, 240]);
            canvas.fill_circle_aa(cx, cy, 10.5 * sc, [42, 118, 232, 255]);
            draw_thick_line(canvas, cx - 5.5 * sc, cy + 5.5 * sc, cx + 6.0 * sc, cy - 6.0 * sc, 2.4 * sc, [255, 96, 88, 255]);
            draw_thick_line(canvas, cx - 5.5 * sc, cy + 5.5 * sc, cx, cy, 2.2 * sc, [248, 250, 255, 255]);
            canvas.fill_circle_aa(cx, cy, 2.2 * sc, [255, 255, 255, 255]);
        }
        VectorIconKind::Settings => {
            // 8-tooth gear + inner dark hub
            for k in 0..8 {
                let ang = k as f32 * std::f32::consts::FRAC_PI_4;
                let tx = cx + ang.cos() * 11.0 * sc;
                let ty = cy + ang.sin() * 11.0 * sc;
                canvas.fill_circle_aa(tx, ty, 3.0 * sc, [242, 245, 252, 245]);
            }
            canvas.fill_circle_aa(cx, cy, 10.0 * sc, [232, 236, 246, 255]);
            canvas.fill_circle_aa(cx, cy, 4.6 * sc, [68, 74, 92, 255]);
        }
        VectorIconKind::SystemMonitor => {
            // Dark display + glowing ECG pulse waveform
            let pad = (6.0 * sc).round() as i32;
            canvas.fill_rounded_rect_aa(x + pad, y + pad, s - pad * 2, s - pad * 2, (5.0 * sc) as i32, [28, 22, 54, 220]);
            let pts = [
                (cx - 12.0 * sc, cy + 2.0 * sc),
                (cx - 6.0 * sc, cy + 2.0 * sc),
                (cx - 2.5 * sc, cy - 8.5 * sc),
                (cx + 2.5 * sc, cy + 8.5 * sc),
                (cx + 6.0 * sc, cy - 2.0 * sc),
                (cx + 12.0 * sc, cy - 2.0 * sc),
            ];
            for w in pts.windows(2) {
                draw_thick_line(canvas, w[0].0, w[0].1, w[1].0, w[1].1, 2.0 * sc, [120, 248, 196, 255]);
            }
        }
        VectorIconKind::Screenshot => {
            // Camera body + lens ring + shutter dot
            let cw = (26.0 * sc).round() as i32;
            let ch = (18.0 * sc).round() as i32;
            canvas.fill_rounded_rect_aa((cx - cw as f32 * 0.5).round() as i32, (cy - ch as f32 * 0.45).round() as i32, cw, ch, (4.5 * sc) as i32, [250, 248, 255, 245]);
            canvas.fill_circle_aa(cx, cy + 1.0 * sc, 5.5 * sc, [176, 58, 116, 255]);
            canvas.fill_circle_aa(cx, cy + 1.0 * sc, 2.6 * sc, [242, 232, 250, 255]);
        }
        VectorIconKind::TextEditor => {
            // Document page + ruled lines
            let pw = (22.0 * sc).round() as i32;
            let ph = (26.0 * sc).round() as i32;
            let px = (cx - pw as f32 * 0.5).round() as i32;
            let py = (cy - ph as f32 * 0.5).round() as i32;
            canvas.fill_rounded_rect_aa(px, py, pw, ph, (4.0 * sc) as i32, [252, 250, 246, 248]);
            for row in 0..3 {
                let ly = py + (7.0 * sc).round() as i32 + row * (5.5 * sc).round().max(3.0) as i32;
                canvas.fill_rounded_rect_aa(px + 4, ly, pw - 8 - if row == 2 { 5 } else { 0 }, 2, 1, [196, 130, 48, 220]);
            }
        }
        VectorIconKind::Home => {
            // House roof + body
            draw_thick_line(canvas, cx, cy - 10.0 * sc, cx - 11.0 * sc, cy, 2.6 * sc, [250, 252, 255, 255]);
            draw_thick_line(canvas, cx, cy - 10.0 * sc, cx + 11.0 * sc, cy, 2.6 * sc, [250, 252, 255, 255]);
            let bw = (16.0 * sc).round() as i32;
            let bh = (12.0 * sc).round() as i32;
            canvas.fill_rounded_rect_aa((cx - bw as f32 * 0.5).round() as i32, cy.round() as i32, bw, bh, (2.5 * sc) as i32, [248, 252, 255, 245]);
            canvas.fill_rounded_rect_aa((cx - 2.5 * sc).round() as i32, (cy + 4.0 * sc).round() as i32, (5.0 * sc).round().max(2.0) as i32, (8.0 * sc).round().max(3.0) as i32, 1, [36, 118, 224, 255]);
        }
        VectorIconKind::GenericApp => {
            // Layered mini app window card
            let ww = (26.0 * sc).round() as i32;
            let wh = (22.0 * sc).round() as i32;
            let wx = (cx - ww as f32 * 0.5).round() as i32;
            let wy = (cy - wh as f32 * 0.5).round() as i32;
            canvas.fill_rounded_rect_aa(wx, wy, ww, wh, (4.5 * sc) as i32, [246, 248, 255, 242]);
            canvas.fill_rect_blend(wx + 2, wy + (6.0 * sc).round().max(3.0) as i32, ww - 4, 1, [90, 120, 200, 140]);
            canvas.fill_circle_aa(wx as f32 + 5.0 * sc, wy as f32 + 3.5 * sc, 1.4 * sc, [255, 95, 87, 255]);
            canvas.fill_circle_aa(wx as f32 + 9.0 * sc, wy as f32 + 3.5 * sc, 1.4 * sc, [254, 188, 46, 255]);
            canvas.fill_circle_aa(wx as f32 + 13.0 * sc, wy as f32 + 3.5 * sc, 1.4 * sc, [40, 200, 64, 255]);
        }
    }
}

fn draw_thick_line(canvas: &mut Canvas, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32, color: [u8; 4]) {
    let steps = ((x1 - x0).hypot(y1 - y0) * 1.5).ceil().max(2.0) as i32;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        canvas.fill_circle_aa(x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, radius.max(0.9), color);
    }
}
