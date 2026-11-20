//! Launcher background picture (user image or procedurally generated pixel landscape).
//!
//! The picture is painted once per frame on the window's background layer; floating panels
//! (translucent `ThemePalette::glass`) are drawn above it. User image decoding & resizing run
//! asynchronously off the GUI thread so switching pictures never stalls rendering.

use eframe::egui::{self, Color32, ColorImage, Context, Rect, TextureHandle, TextureOptions, Vec2};
use std::path::{Path, PathBuf};

const MAX_BACKGROUND_EDGE: u32 = 2560;
const DEFAULT_W: usize = 480;
const DEFAULT_H: usize = 270;

pub struct BackgroundManager {
    texture: Option<TextureHandle>,
    loaded_key: Option<String>,
    loading_key: Option<String>,
    size: Vec2,
}

impl BackgroundManager {
    pub fn new() -> Self {
        Self {
            texture: None,
            loaded_key: None,
            loading_key: None,
            size: Vec2::new(DEFAULT_W as f32, DEFAULT_H as f32),
        }
    }

    /// Ensures bundled/disabled state synchronously and returns `Some((key, path))` when a
    /// user-chosen image still needs asynchronous decoding off the GUI thread.
    pub fn ensure_or_request(
        &mut self,
        ctx: &Context,
        enabled: bool,
        user_path: &str,
    ) -> Option<(String, PathBuf)> {
        if !enabled {
            self.loaded_key = Some("off".to_string());
            self.loading_key = None;
            self.texture = None;
            return None;
        }
        let trimmed = user_path.trim();
        let key = format!("on|{trimmed}");
        if self.loaded_key.as_deref() == Some(key.as_str()) {
            return None;
        }

        if trimmed.is_empty() {
            let image = generate_default_landscape();
            self.size = Vec2::new(image.size[0] as f32, image.size[1] as f32);
            self.texture = Some(ctx.load_texture("mcw_background", image, TextureOptions::NEAREST));
            self.loaded_key = Some(key);
            self.loading_key = None;
            return None;
        }

        // Paint bundled landscape as an immediate fallback until async user image decode completes.
        if self.texture.is_none() {
            let image = generate_default_landscape();
            self.size = Vec2::new(image.size[0] as f32, image.size[1] as f32);
            self.texture = Some(ctx.load_texture("mcw_background", image, TextureOptions::NEAREST));
        }

        if self.loading_key.as_deref() == Some(key.as_str()) {
            return None;
        }
        self.loading_key = Some(key.clone());
        Some((key, PathBuf::from(trimmed)))
    }

    /// Applies the asynchronously decoded user background image on the UI thread.
    /// Returns `Some(err)` if decoding failed (the default landscape remains visible).
    pub fn apply_loaded_user_image(
        &mut self,
        ctx: &Context,
        key: String,
        expected_enabled: bool,
        expected_user_path: &str,
        result: Result<ColorImage, String>,
    ) -> Option<String> {
        if self.loading_key.as_deref() == Some(key.as_str()) {
            self.loading_key = None;
        }
        let current_key = if expected_enabled {
            format!("on|{}", expected_user_path.trim())
        } else {
            "off".to_string()
        };
        if key != current_key {
            return None;
        }
        self.loaded_key = Some(key);
        match result {
            Ok(img) => {
                self.size = Vec2::new(img.size[0] as f32, img.size[1] as f32);
                self.texture =
                    Some(ctx.load_texture("mcw_background", img, TextureOptions::LINEAR));
                None
            }
            Err(err) => {
                let fallback = generate_default_landscape();
                self.size = Vec2::new(fallback.size[0] as f32, fallback.size[1] as f32);
                self.texture =
                    Some(ctx.load_texture("mcw_background", fallback, TextureOptions::NEAREST));
                Some(err)
            }
        }
    }

    pub fn is_active(&self) -> bool {
        self.texture.is_some()
    }

    /// Paints the picture (cover-fit) and a dimming wash of `tint` with alpha `dim` (0..1).
    pub fn paint(&self, ctx: &Context, dim: f32, tint: Color32) {
        let Some(tex) = &self.texture else {
            return;
        };
        let screen = ctx.screen_rect();
        if screen.width() < 1.0 || screen.height() < 1.0 {
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::background());
        painter.image(
            tex.id(),
            screen,
            cover_uv(self.size, screen.size()),
            Color32::WHITE,
        );
        let a = (dim.clamp(0.0, 0.9) * 255.0).round() as u8;
        if a > 0 {
            painter.rect_filled(
                screen,
                0.0,
                Color32::from_rgba_unmultiplied(tint.r(), tint.g(), tint.b(), a),
            );
        }
    }
}

/// UV rectangle that makes an image of `img` size cover a `target` area without distortion.
pub fn cover_uv(img: Vec2, target: Vec2) -> Rect {
    if img.x <= 0.0 || img.y <= 0.0 || target.x <= 0.0 || target.y <= 0.0 {
        return Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    }
    let img_aspect = img.x / img.y;
    let target_aspect = target.x / target.y;
    if target_aspect > img_aspect {
        // target is wider: crop top/bottom
        let visible = img_aspect / target_aspect;
        let off = (1.0 - visible) * 0.5;
        Rect::from_min_max(egui::pos2(0.0, off), egui::pos2(1.0, off + visible))
    } else {
        let visible = target_aspect / img_aspect;
        let off = (1.0 - visible) * 0.5;
        Rect::from_min_max(egui::pos2(off, 0.0), egui::pos2(off + visible, 1.0))
    }
}

/// Decodes and safely downscales a user background image (runs on a worker thread).
pub fn decode_user_image(path: &Path) -> Result<ColorImage, String> {
    let img = image::open(path).map_err(|e| format!("Could not open background picture: {e}"))?;
    let img = if img.width() > MAX_BACKGROUND_EDGE || img.height() > MAX_BACKGROUND_EDGE {
        img.resize(
            MAX_BACKGROUND_EDGE,
            MAX_BACKGROUND_EDGE,
            image::imageops::FilterType::Triangle,
        )
    } else {
        img
    };
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t.clamp(0.0, 1.0)).round() as u8
}

fn lerp_rgb(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    [
        lerp_u8(a[0], b[0], t),
        lerp_u8(a[1], b[1], t),
        lerp_u8(a[2], b[2], t),
    ]
}

/// Deterministic pixel-art dusk: gradient sky, stars, a low sun and layered silhouettes.
pub fn generate_default_landscape() -> ColorImage {
    let (w, h) = (DEFAULT_W, DEFAULT_H);
    let mut px = vec![Color32::BLACK; w * h];
    let horizon = (h as f32 * 0.66) as usize;

    let top = [0x12, 0x0A, 0x2E];
    let mid = [0x5B, 0x2A, 0x7A];
    let low = [0xE8, 0x7A, 0x5C];
    for y in 0..h {
        let t = (y as f32 / horizon as f32).min(1.0);
        let c = if t < 0.6 {
            lerp_rgb(top, mid, t / 0.6)
        } else {
            lerp_rgb(mid, low, (t - 0.6) / 0.4)
        };
        for x in 0..w {
            px[y * w + x] = Color32::from_rgb(c[0], c[1], c[2]);
        }
    }

    // Stars (fixed LCG so the picture is identical every run).
    let mut seed: u32 = 0x1234_5678;
    let mut next = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed
    };
    for _ in 0..140 {
        let x = (next() >> 8) as usize % w;
        let y = (next() >> 8) as usize % (horizon * 6 / 10);
        let b = 150 + (next() >> 24) as u8 % 100;
        px[y * w + x] = Color32::from_rgb(b, b, 255);
    }

    // Low sun disc.
    let (cx, cy, r) = (w as f32 * 0.68, horizon as f32 - 6.0, 15.0_f32);
    for y in 0..horizon {
        for x in 0..w {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            if dx * dx + dy * dy <= r * r {
                px[y * w + x] = Color32::from_rgb(0xFF, 0xD8, 0x8A);
            }
        }
    }

    // Layered silhouettes: far hills, near hills, blocky trees.
    let layers: [([u8; 3], f32, f32, f32); 3] = [
        ([0x4A, 0x1F, 0x5E], 0.0, 14.0, 0.021),
        ([0x2B, 0x12, 0x45], 9.0, 20.0, 0.034),
        ([0x16, 0x0A, 0x2A], 22.0, 16.0, 0.052),
    ];
    for (color, base, amp, freq) in layers {
        for x in 0..w {
            // Blocky 4px columns for the pixel look.
            let bx = (x / 4 * 4) as f32;
            let wave = (bx * freq).sin() * 0.6 + (bx * freq * 2.3 + 1.7).sin() * 0.4;
            let top_y = (horizon as f32 + base - amp * (0.5 + 0.5 * wave)) as usize;
            for y in top_y.min(h - 1)..h {
                px[y * w + x] = Color32::from_rgb(color[0], color[1], color[2]);
            }
        }
    }
    let tree = Color32::from_rgb(0x0C, 0x06, 0x1A);
    let mut x = 6;
    while x + 12 < w {
        let trunk_x = x + 5;
        let gh = horizon + 24;
        let height = 14 + (next() >> 20) as usize % 18;
        for y in gh.saturating_sub(height)..(gh + 2).min(h) {
            for dx in 0..2 {
                px[y * w + trunk_x + dx] = tree;
            }
        }
        for row in 0..3usize {
            let ty = gh.saturating_sub(height) + row * 4;
            let half = 4 + row * 2;
            for yy in ty..(ty + 4).min(h) {
                for xx in trunk_x.saturating_sub(half)..(trunk_x + 2 + half).min(w) {
                    px[yy * w + xx] = tree;
                }
            }
        }
        x += 18 + (next() >> 22) as usize % 40;
    }

    ColorImage {
        size: [w, h],
        pixels: px,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_uv_crops_the_longer_axis_without_distortion() {
        // Wide target, square image: crop top/bottom.
        let r = cover_uv(Vec2::new(100.0, 100.0), Vec2::new(200.0, 100.0));
        assert!((r.min.x - 0.0).abs() < 1e-5 && (r.max.x - 1.0).abs() < 1e-5);
        assert!((r.height() - 0.5).abs() < 1e-5);
        // Tall target, wide image: crop left/right.
        let r = cover_uv(Vec2::new(200.0, 100.0), Vec2::new(100.0, 100.0));
        assert!((r.width() - 0.5).abs() < 1e-5);
        assert!((r.min.y - 0.0).abs() < 1e-5 && (r.max.y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn default_landscape_is_deterministic_and_full_size() {
        let a = generate_default_landscape();
        let b = generate_default_landscape();
        assert_eq!(a.size, [DEFAULT_W, DEFAULT_H]);
        assert_eq!(a.pixels, b.pixels);
        assert!(a.pixels.iter().all(|c| c.a() == 255));
    }
}
