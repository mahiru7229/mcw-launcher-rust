//! Launcher background picture (pure black by default, or user PNG/JPEG/WebP image).
//!
//! When enabled with a user image, decoding & resizing run asynchronously off the GUI thread
//! so switching pictures never stalls rendering. By default (no custom picture), the launcher
//! paints a clean pure-black (`#000000`) backdrop.

use eframe::egui::{self, Color32, ColorImage, Context, Rect, TextureHandle, TextureOptions, Vec2};
use std::path::{Path, PathBuf};

const MAX_BACKGROUND_EDGE: u32 = 2560;

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
            size: Vec2::new(1.0, 1.0),
        }
    }

    /// Clears textures when disabled/default (pure black) and returns `Some((key, path))`
    /// when a user-chosen image needs asynchronous decoding off the GUI thread.
    pub fn ensure_or_request(
        &mut self,
        _ctx: &Context,
        enabled: bool,
        user_path: &str,
    ) -> Option<(String, PathBuf)> {
        let trimmed = user_path.trim();
        if !enabled || trimmed.is_empty() {
            self.loaded_key = Some(if enabled {
                "default_black".to_string()
            } else {
                "off".to_string()
            });
            self.loading_key = None;
            self.texture = None;
            return None;
        }
        let key = format!("on|{trimmed}");
        if self.loaded_key.as_deref() == Some(key.as_str()) {
            return None;
        }

        if self.loading_key.as_deref() == Some(key.as_str()) {
            return None;
        }
        self.loading_key = Some(key.clone());
        Some((key, PathBuf::from(trimmed)))
    }

    /// Applies the asynchronously decoded user background image on the UI thread.
    /// Returns `Some(err)` if decoding failed (falls back to pure black).
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
        let trimmed = expected_user_path.trim();
        let current_key = if expected_enabled && !trimmed.is_empty() {
            format!("on|{trimmed}")
        } else if expected_enabled {
            "default_black".to_string()
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
                self.texture = None;
                Some(err)
            }
        }
    }

    pub fn is_active(&self) -> bool {
        self.texture.is_some()
    }

    /// Paints the user picture (cover-fit) and a dimming wash of `tint` with alpha `dim` (0..1).
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
    fn default_background_without_user_image_is_inactive_for_pure_black() {
        let mut mgr = BackgroundManager::new();
        let ctx = egui::Context::default();
        assert!(mgr.ensure_or_request(&ctx, true, "").is_none());
        assert!(!mgr.is_active());
    }
}
