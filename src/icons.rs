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
    Calculator,
    Calendar,
    Clock,
    Camera,
    AudioPlayer,
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
        } else if s.contains("calculator") || s.contains("kcalc") || s.contains("qalculate") || s.contains("galculator") {
            Self::Calculator
        } else if s.contains("calendar") || s.contains("kalendar") {
            Self::Calendar
        } else if s.contains("clock") || s.contains("timer") {
            Self::Clock
        } else if s.contains("camera") || s.contains("snapshot") || s.contains("cheese") || s.contains("kamoso") {
            Self::Camera
        } else if s.contains("audio") || s.contains("music") || s.contains("decibels") || s.contains("rhythmbox") || s.contains("audacious") || s.contains("spotify") || s.contains("vlc") || s.contains("mpv") {
            Self::AudioPlayer
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

#[derive(Debug, Clone)]
enum IconCacheEntry {
    Loading,
    Ready(DecodedIcon),
    Missing,
}

static ICON_CACHE: OnceLock<Mutex<HashMap<String, IconCacheEntry>>> = OnceLock::new();

/// Non-blocking icon lookup: returns cached RGBA icon if ready, or schedules background
/// decoding (PNG / SVG via `rsvg-convert`) on the worker thread and returns `None` meanwhile.
pub fn load_system_icon(icon_name: &str, target_size: i32) -> Option<DecodedIcon> {
    let trimmed = icon_name.trim();
    if trimmed.is_empty() {
        return None;
    }
    let key = format!("{}@{}", trimmed, target_size);
    let cache = ICON_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut guard) = cache.lock() {
        match guard.get(&key) {
            Some(IconCacheEntry::Ready(icon)) => return Some(icon.clone()),
            Some(IconCacheEntry::Loading) | Some(IconCacheEntry::Missing) => return None,
            None => {
                guard.insert(key, IconCacheEntry::Loading);
            }
        }
    }

    crate::worker::dispatch(crate::worker::WorkerRequest::LoadIcon {
        name: trimmed.to_string(),
        target_size,
    });
    None
}

/// Synchronous icon loader executed on the background worker thread.
pub fn load_system_icon_sync(icon_name: &str, target_size: i32) {
    let trimmed = icon_name.trim();
    if trimmed.is_empty() {
        return;
    }
    let key = format!("{}@{}", trimmed, target_size);
    let loaded = find_icon_path(trimmed).and_then(|p| decode_icon_file(&p, target_size));
    let cache = ICON_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut guard) = cache.lock() {
        match loaded {
            Some(icon) => {
                guard.insert(key, IconCacheEntry::Ready(icon));
            }
            None => {
                guard.insert(key, IconCacheEntry::Missing);
            }
        }
    }
}

static ICON_THEME_CHAIN: OnceLock<Vec<String>> = OnceLock::new();

fn read_ini_icon_theme(path: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(key) {
            if let Some(val) = rest.trim().strip_prefix('=') {
                let theme = val.trim().trim_matches('"').trim_matches('\'');
                if !theme.is_empty() {
                    return Some(theme.to_string());
                }
            }
        }
    }
    None
}

fn read_theme_inherits(theme_name: &str, roots: &[PathBuf]) -> Vec<String> {
    for root in roots {
        let idx_path = root.join(theme_name).join("index.theme");
        if let Ok(text) = fs::read_to_string(idx_path) {
            for line in text.lines() {
                if let Some(rest) = line.trim().strip_prefix("Inherits=") {
                    return rest
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
            }
        }
    }
    Vec::new()
}

fn icon_theme_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        let h = PathBuf::from(home);
        roots.push(h.join(".local/share/icons"));
        roots.push(h.join(".icons"));
        roots.push(h.join(".local/share/flatpak/exports/share/icons"));
    }
    roots.push(PathBuf::from("/var/lib/flatpak/exports/share/icons"));
    roots.push(PathBuf::from("/usr/local/share/icons"));
    roots.push(PathBuf::from("/usr/share/icons"));
    roots
}

fn active_icon_theme_chain() -> &'static Vec<String> {
    ICON_THEME_CHAIN.get_or_init(|| {
        let roots = icon_theme_roots();
        let mut seeds: Vec<String> = Vec::new();
        for env_key in ["OKAY_ICON_THEME", "GTK_ICON_THEME"] {
            if let Ok(val) = std::env::var(env_key) {
                if !val.trim().is_empty() {
                    seeds.push(val.trim().to_string());
                }
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            let h = PathBuf::from(home);
            for (cfg, key) in [
                (h.join(".config/gtk-4.0/settings.ini"), "gtk-icon-theme-name"),
                (h.join(".config/gtk-3.0/settings.ini"), "gtk-icon-theme-name"),
                (h.join(".config/kdeglobals"), "Theme"),
            ] {
                if let Some(t) = read_ini_icon_theme(&cfg, key) {
                    seeds.push(t);
                }
            }
        }
        for builtin in [
            "Papirus",
            "Papirus-Dark",
            "Adwaita",
            "breeze",
            "breeze-dark",
            "Tela",
            "Colloid",
            "WhiteSur",
            "hicolor",
        ] {
            seeds.push(builtin.to_string());
        }

        let mut chain: Vec<String> = Vec::new();
        let mut queue = seeds;
        while let Some(theme) = queue.first().cloned() {
            queue.remove(0);
            if chain.iter().any(|t| t == &theme) {
                continue;
            }
            let parents = read_theme_inherits(&theme, &roots);
            chain.push(theme);
            for p in parents {
                if !chain.iter().any(|t| t == &p) {
                    queue.push(p);
                }
            }
        }

        // Also include any other installed themes under /usr/share/icons or ~/.local/share/icons
        for root in &roots {
            if let Ok(entries) = fs::read_dir(root) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() && p.join("index.theme").is_file() {
                        if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                            if !chain.iter().any(|t| t == name) {
                                chain.push(name.to_string());
                            }
                        }
                    }
                }
            }
        }
        chain
    })
}

fn strip_known_icon_ext(icon_name: &str) -> &str {
    for ext in [".png", ".svg", ".xpm", ".ico", ".PNG", ".SVG"] {
        if let Some(stripped) = icon_name.strip_suffix(ext) {
            return stripped;
        }
    }
    icon_name
}

fn find_icon_path(icon_name: &str) -> Option<PathBuf> {
    let cleaned = icon_name.trim().trim_matches('"');
    let cleaned = cleaned.strip_prefix("file://").unwrap_or(cleaned);
    let direct = PathBuf::from(cleaned);
    if direct.is_absolute() && direct.is_file() {
        return Some(direct);
    }

    // Preserve reverse-DNS dots like `org.gnome.Calculator`; only strip real image extensions.
    let base_name = strip_known_icon_ext(cleaned);
    let lower_name = base_name.to_lowercase();
    let roots = icon_theme_roots();
    let themes = active_icon_theme_chain();

    const SUBDIRS: &[&str] = &[
        "scalable/apps",
        "256x256/apps",
        "128x128/apps",
        "96x96/apps",
        "64x64/apps",
        "48x48/apps",
        "32x32/apps",
        "512x512/apps",
        "apps/scalable",
        "apps/128",
        "apps/64",
        "apps/48",
        "apps/32",
        "scalable/categories",
        "48x48/categories",
        "64x64/categories",
        "scalable/places",
        "48x48/places",
        "64x64/places",
        "scalable/devices",
        "48x48/devices",
        "64x64/devices",
        "scalable/mimetypes",
        "48x48/mimetypes",
        "64x64/mimetypes",
        "scalable/status",
        "48x48/status",
        "symbolic/apps",
        "symbolic/categories",
        "symbolic/devices",
        "symbolic/places",
    ];

    for theme in themes {
        for root in &roots {
            let theme_dir = root.join(theme);
            if !theme_dir.is_dir() {
                continue;
            }
            for sub in SUBDIRS {
                let dir = theme_dir.join(sub);
                if !dir.is_dir() {
                    continue;
                }
                for candidate_stem in [base_name, lower_name.as_str()] {
                    for ext in ["png", "svg"] {
                        let candidate = dir.join(format!("{}.{}", candidate_stem, ext));
                        if candidate.is_file() {
                            return Some(candidate);
                        }
                    }
                    let sym = dir.join(format!("{}-symbolic.svg", candidate_stem));
                    if sym.is_file() {
                        return Some(sym);
                    }
                }
            }
        }
    }

    let pixmaps = Path::new("/usr/share/pixmaps");
    if pixmaps.is_dir() {
        for candidate_stem in [base_name, lower_name.as_str()] {
            for ext in ["png", "svg"] {
                let candidate = pixmaps.join(format!("{}.{}", candidate_stem, ext));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn rasterize_svg_resvg(svg_bytes: &[u8], target_size: i32, is_symbolic: bool) -> Option<DecodedIcon> {
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(svg_bytes, &opt).ok()?;
    let size = target_size.max(16) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)?;
    let tree_size = tree.size();
    let sx = size as f32 / tree_size.width().max(1.0);
    let sy = size as f32 / tree_size.height().max(1.0);
    let scale = sx.min(sy);
    let dx = (size as f32 - tree_size.width() * scale) * 0.5;
    let dy = (size as f32 - tree_size.height() * scale) * 0.5;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale).post_translate(dx, dy);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let raw = pixmap.data();
    let mut rgba = Vec::with_capacity(raw.len());
    for px in raw.chunks_exact(4) {
        let a = px[3];
        if a == 0 {
            rgba.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            let (mut r, mut g, mut b) = if a == 255 {
                (px[0], px[1], px[2])
            } else {
                (
                    ((px[0] as u16 * 255) / a as u16).min(255) as u8,
                    ((px[1] as u16 * 255) / a as u16).min(255) as u8,
                    ((px[2] as u16 * 255) / a as u16).min(255) as u8,
                )
            };
            if is_symbolic && r.max(g).max(b) < 95 {
                r = 238;
                g = 242;
                b = 252;
            }
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    Some(DecodedIcon {
        width: size as i32,
        height: size as i32,
        rgba,
    })
}

fn decode_icon_file(path: &Path, target_size: i32) -> Option<DecodedIcon> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let is_symbolic = path.to_string_lossy().contains("-symbolic");

    let raw_png_bytes = if ext == "svg" {
        if let Ok(svg_bytes) = fs::read(path) {
            if let Some(icon) = rasterize_svg_resvg(&svg_bytes, target_size, is_symbolic) {
                return Some(icon);
            }
        }
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
    if info.width == 0 || info.height == 0 || info.width > 4096 || info.height > 4096 {
        return None;
    }
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
        VectorIconKind::Calculator => ([248, 152, 56], [214, 96, 28]),
        VectorIconKind::Calendar => ([242, 88, 96], [192, 42, 56]),
        VectorIconKind::Clock => ([54, 62, 82], [28, 32, 46]),
        VectorIconKind::Camera => ([116, 132, 156], [62, 74, 96]),
        VectorIconKind::AudioPlayer => ([236, 78, 138], [164, 42, 116]),
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
        VectorIconKind::Calculator => {
            let cw = (24.0 * sc).round() as i32;
            let ch = (28.0 * sc).round() as i32;
            let bx = (cx - cw as f32 * 0.5).round() as i32;
            let by = (cy - ch as f32 * 0.5).round() as i32;
            canvas.fill_rounded_rect_aa(bx, by, cw, ch, (4.0 * sc) as i32, [28, 32, 44, 235]);
            canvas.fill_rounded_rect_aa(bx + 3, by + 3, cw - 6, (7.0 * sc).round().max(3.0) as i32, 2, [146, 226, 180, 235]);
            for r in 0..2 {
                for c in 0..3 {
                    let gx = bx + 4 + c * (6.5 * sc).round().max(3.0) as i32;
                    let gy = by + (13.0 * sc).round() as i32 + r * (6.0 * sc).round().max(3.0) as i32;
                    let col = if c == 2 { [248, 158, 62, 255] } else { [224, 230, 244, 235] };
                    canvas.fill_circle_aa(gx as f32 + 2.0 * sc, gy as f32 + 2.0 * sc, 2.0 * sc, col);
                }
            }
        }
        VectorIconKind::Calendar => {
            let cw = (26.0 * sc).round() as i32;
            let ch = (26.0 * sc).round() as i32;
            let bx = (cx - cw as f32 * 0.5).round() as i32;
            let by = (cy - ch as f32 * 0.5).round() as i32;
            canvas.fill_rounded_rect_aa(bx, by, cw, ch, (4.5 * sc) as i32, [248, 250, 255, 248]);
            canvas.fill_rounded_rect_aa(bx, by, cw, (8.0 * sc).round().max(4.0) as i32, (4.0 * sc) as i32, [218, 52, 66, 255]);
            for r in 0..2 {
                for c in 0..3 {
                    canvas.fill_circle_aa(
                        bx as f32 + (6.5 + c as f32 * 6.5) * sc,
                        by as f32 + (14.5 + r as f32 * 6.0) * sc,
                        1.7 * sc,
                        [64, 72, 92, 220],
                    );
                }
            }
        }
        VectorIconKind::Clock => {
            canvas.fill_circle_aa(cx, cy, 13.5 * sc, [246, 248, 255, 248]);
            canvas.fill_circle_aa(cx, cy, 11.0 * sc, [34, 40, 56, 255]);
            draw_thick_line(canvas, cx, cy, cx, cy - 7.5 * sc, 1.6 * sc, [248, 250, 255, 255]);
            draw_thick_line(canvas, cx, cy, cx + 5.5 * sc, cy + 2.5 * sc, 1.5 * sc, [255, 116, 92, 255]);
            canvas.fill_circle_aa(cx, cy, 1.8 * sc, [255, 255, 255, 255]);
        }
        VectorIconKind::Camera => {
            let cw = (26.0 * sc).round() as i32;
            let ch = (19.0 * sc).round() as i32;
            canvas.fill_rounded_rect_aa((cx - cw as f32 * 0.5).round() as i32, (cy - ch as f32 * 0.45).round() as i32, cw, ch, (4.5 * sc) as i32, [244, 248, 255, 245]);
            canvas.fill_circle_aa(cx, cy + 1.0 * sc, 6.2 * sc, [42, 52, 72, 255]);
            canvas.fill_circle_aa(cx, cy + 1.0 * sc, 3.2 * sc, [96, 178, 255, 255]);
        }
        VectorIconKind::AudioPlayer => {
            canvas.fill_circle_aa(cx - 4.5 * sc, cy + 6.5 * sc, 4.0 * sc, [248, 250, 255, 250]);
            canvas.fill_circle_aa(cx + 6.5 * sc, cy + 4.0 * sc, 4.0 * sc, [248, 250, 255, 250]);
            draw_thick_line(canvas, cx - 1.5 * sc, cy + 6.0 * sc, cx - 1.5 * sc, cy - 8.5 * sc, 1.6 * sc, [248, 250, 255, 250]);
            draw_thick_line(canvas, cx + 9.5 * sc, cy + 3.5 * sc, cx + 9.5 * sc, cy - 10.5 * sc, 1.6 * sc, [248, 250, 255, 250]);
            draw_thick_line(canvas, cx - 1.5 * sc, cy - 8.5 * sc, cx + 9.5 * sc, cy - 10.5 * sc, 2.0 * sc, [248, 250, 255, 250]);
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

fn draw_arc_aa(canvas: &mut Canvas, cx: f32, cy: f32, radius: f32, start_deg: f32, end_deg: f32, thickness: f32, color: [u8; 4]) {
    let steps = ((end_deg - start_deg).abs() * 0.45).ceil().max(6.0) as i32;
    for i in 0..=steps {
        let deg = start_deg + (end_deg - start_deg) * (i as f32 / steps as f32);
        let rad = deg.to_radians();
        canvas.fill_circle_aa(cx + rad.cos() * radius, cy + rad.sin() * radius, thickness, color);
    }
}

/// Crisp vector Wi-Fi signal fan centered horizontally at `cx`, base dot at `cy`.
pub fn draw_wifi_glyph(canvas: &mut Canvas, cx: f32, cy: f32, scale: f32, color: [u8; 4]) {
    canvas.fill_circle_aa(cx, cy, 1.5 * scale, color);
    draw_arc_aa(canvas, cx, cy, 4.2 * scale, -135.0, -45.0, 0.95 * scale, color);
    draw_arc_aa(canvas, cx, cy, 7.6 * scale, -135.0, -45.0, 0.95 * scale, color);
    draw_arc_aa(canvas, cx, cy, 11.0 * scale, -135.0, -45.0, 0.95 * scale, color);
}

/// Crisp vector Bluetooth rune centered at `(cx, cy)`.
pub fn draw_bluetooth_glyph(canvas: &mut Canvas, cx: f32, cy: f32, scale: f32, color: [u8; 4]) {
    let r = 0.9 * scale;
    draw_thick_line(canvas, cx, cy - 6.5 * scale, cx, cy + 6.5 * scale, r, color);
    draw_thick_line(canvas, cx, cy - 6.5 * scale, cx + 4.2 * scale, cy - 3.0 * scale, r, color);
    draw_thick_line(canvas, cx + 4.2 * scale, cy - 3.0 * scale, cx - 3.8 * scale, cy + 3.2 * scale, r, color);
    draw_thick_line(canvas, cx, cy + 6.5 * scale, cx + 4.2 * scale, cy + 3.0 * scale, r, color);
    draw_thick_line(canvas, cx + 4.2 * scale, cy + 3.0 * scale, cx - 3.8 * scale, cy - 3.2 * scale, r, color);
}

/// Crisp vector Master-Stack tiling layout glyph centered at `(cx, cy)`.
pub fn draw_tiling_glyph(canvas: &mut Canvas, cx: f32, cy: f32, scale: f32, color: [u8; 4]) {
    let left_x = (cx - 6.0 * scale).round() as i32;
    let top_y = (cy - 5.5 * scale).round() as i32;
    let mw = (5.0 * scale).round().max(3.0) as i32;
    let mh = (11.0 * scale).round().max(6.0) as i32;
    let rw = (5.5 * scale).round().max(3.0) as i32;
    let rh = (4.5 * scale).round().max(2.0) as i32;
    let rx = (cx + 1.0 * scale).round() as i32;
    canvas.fill_rounded_rect_aa(left_x, top_y, mw, mh, 1, color);
    canvas.fill_rounded_rect_aa(rx, top_y, rw, rh, 1, color);
    canvas.fill_rounded_rect_aa(rx, top_y + mh - rh, rw, rh, 1, color);
}

/// Crisp vector Notification Bell centered at `(cx, cy)`.
pub fn draw_bell_glyph(canvas: &mut Canvas, cx: f32, cy: f32, scale: f32, color: [u8; 4]) {
    canvas.fill_circle_aa(cx, cy - 1.5 * scale, 4.0 * scale, color);
    let bw = (9.5 * scale).round().max(5.0) as i32;
    let bh = (4.5 * scale).round().max(3.0) as i32;
    canvas.fill_rounded_rect_aa((cx - bw as f32 * 0.5).round() as i32, (cy - 1.0 * scale).round() as i32, bw, bh, 2, color);
    let base_w = (12.0 * scale).round().max(7.0) as i32;
    canvas.fill_rounded_rect_aa((cx - base_w as f32 * 0.5).round() as i32, (cy + 2.5 * scale).round() as i32, base_w, (2.2 * scale).round().max(2.0) as i32, 1, color);
    canvas.fill_circle_aa(cx, cy + 5.2 * scale, 1.5 * scale, color);
}

/// Crisp vector Speaker icon with sound waves (or mute cross) centered vertically at `cy`.
pub fn draw_speaker_glyph(canvas: &mut Canvas, left_x: f32, cy: f32, muted: bool, scale: f32, color: [u8; 4]) {
    let bx = left_x.round() as i32;
    let by = (cy - 2.5 * scale).round() as i32;
    canvas.fill_rounded_rect_aa(bx, by, (3.8 * scale).round().max(2.0) as i32, (5.0 * scale).round().max(3.0) as i32, 1, color);
    draw_thick_line(canvas, left_x + 3.0 * scale, cy - 2.0 * scale, left_x + 6.5 * scale, cy - 4.8 * scale, 1.0 * scale, color);
    draw_thick_line(canvas, left_x + 6.5 * scale, cy - 4.8 * scale, left_x + 6.5 * scale, cy + 4.8 * scale, 1.0 * scale, color);
    draw_thick_line(canvas, left_x + 3.0 * scale, cy + 2.0 * scale, left_x + 6.5 * scale, cy + 4.8 * scale, 1.0 * scale, color);
    if muted {
        let mut xcol = color;
        xcol[0] = 255; xcol[1] = 108; xcol[2] = 104;
        draw_thick_line(canvas, left_x + 9.0 * scale, cy - 2.8 * scale, left_x + 13.5 * scale, cy + 2.8 * scale, 0.9 * scale, xcol);
        draw_thick_line(canvas, left_x + 13.5 * scale, cy - 2.8 * scale, left_x + 9.0 * scale, cy + 2.8 * scale, 0.9 * scale, xcol);
    } else {
        draw_arc_aa(canvas, left_x + 6.0 * scale, cy, 4.0 * scale, -48.0, 48.0, 0.85 * scale, color);
        draw_arc_aa(canvas, left_x + 6.0 * scale, cy, 6.8 * scale, -52.0, 52.0, 0.85 * scale, color);
    }
}

/// Crisp vector Battery icon with fill level at `(x, cy)`.
pub fn draw_battery_glyph(canvas: &mut Canvas, x: i32, cy: i32, percent: i32, color: [u8; 4]) {
    let bw = 15;
    let bh = 9;
    let by = cy - bh / 2;
    canvas.fill_rounded_rect_aa(x, by, bw, bh, 2, [color[0], color[1], color[2], 70]);
    canvas.fill_rounded_rect_aa(x + bw, by + 2, 2, 5, 1, [color[0], color[1], color[2], 180]);
    let fill_w = ((bw - 4) * percent.clamp(0, 100) / 100).max(2);
    let fill_col = if percent <= 20 { [255, 102, 96, 255] } else { [92, 224, 146, 255] };
    canvas.fill_rounded_rect_aa(x + 2, by + 2, fill_w, bh - 4, 1, fill_col);
}
