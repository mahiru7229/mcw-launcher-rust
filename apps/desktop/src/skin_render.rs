use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use image::{imageops, Rgba, RgbaImage};

pub const STEVE_SKIN_B64: &str = "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";

pub const ALEX_SKIN_B64: &str = "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";

/// Crop Minecraft 8x8 base face `(8,8)` + 8x8 hat overlay `(40,8)`, composite with SourceOver,
/// and upscale with nearest-neighbor pixel art filtering.
pub fn render_skin_face_rgba(png_bytes: &[u8], out_size: u32) -> Option<([usize; 2], Vec<u8>)> {
    let img = image::load_from_memory(png_bytes).ok()?.to_rgba8();
    if img.width() < 48 || img.height() < 16 {
        return None;
    }

    let mut face = RgbaImage::new(8, 8);
    for y in 0..8 {
        for x in 0..8 {
            let base = *img.get_pixel(8 + x, 8 + y);
            let hat = *img.get_pixel(40 + x, 8 + y);
            let blended = blend_source_over(base, hat);
            face.put_pixel(x, y, blended);
        }
    }

    let size = out_size.max(8);
    let scaled = imageops::resize(&face, size, size, imageops::FilterType::Nearest);
    Some(([size as usize, size as usize], scaled.into_raw()))
}

/// Encode `text` into a crisp square RGBA QR code buffer suitable for nearest-neighbor upload.
pub fn render_qr_code_rgba(
    text: &str,
    module_px: usize,
    quiet_modules: usize,
) -> Option<([usize; 2], Vec<u8>)> {
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let width_mods = code.width();
    let module_px = module_px.max(2);
    let total_mods = width_mods + quiet_modules * 2;
    let img_px = total_mods * module_px;
    let mut rgba = vec![255u8; img_px * img_px * 4];

    for my in 0..width_mods {
        for mx in 0..width_mods {
            if code[(mx, my)] == qrcode::Color::Dark {
                let x0 = (mx + quiet_modules) * module_px;
                let y0 = (my + quiet_modules) * module_px;
                for dy in 0..module_px {
                    for dx in 0..module_px {
                        let idx = ((y0 + dy) * img_px + (x0 + dx)) * 4;
                        rgba[idx] = 15;
                        rgba[idx + 1] = 23;
                        rgba[idx + 2] = 42;
                        rgba[idx + 3] = 255;
                    }
                }
            }
        }
    }

    Some(([img_px, img_px], rgba))
}

pub fn default_skin_face_rgba(variant: &str, out_size: u32) -> Option<([usize; 2], Vec<u8>)> {
    let b64 = if variant.eq_ignore_ascii_case("alex") || variant.eq_ignore_ascii_case("slim") {
        ALEX_SKIN_B64
    } else {
        STEVE_SKIN_B64
    };
    let png_bytes = STANDARD.decode(b64).ok()?;
    render_skin_face_rgba(&png_bytes, out_size)
}

fn blend_source_over(dst: Rgba<u8>, src: Rgba<u8>) -> Rgba<u8> {
    let sa = src[3] as f32 / 255.0;
    let da = dst[3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 0.0 {
        return Rgba([0, 0, 0, 0]);
    }
    let mix = |sc: u8, dc: u8| -> u8 {
        let val = ((sc as f32 * sa) + (dc as f32 * da * (1.0 - sa))) / out_a;
        val.round().clamp(0.0, 255.0) as u8
    };
    Rgba([
        mix(src[0], dst[0]),
        mix(src[1], dst[1]),
        mix(src[2], dst[2]),
        (out_a * 255.0).round().clamp(0.0, 255.0) as u8,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_embedded_steve_and_alex_faces_crisp_nearest_neighbor() {
        let (steve_size, steve_rgba) = default_skin_face_rgba("classic", 32).unwrap();
        assert_eq!(steve_size, [32, 32]);
        assert_eq!(steve_rgba.len(), 32 * 32 * 4);
        assert!(steve_rgba.iter().any(|&b| b > 0));

        let (alex_size, alex_rgba) = default_skin_face_rgba("slim", 24).unwrap();
        assert_eq!(alex_size, [24, 24]);
        assert_eq!(alex_rgba.len(), 24 * 24 * 4);
        assert_ne!(alex_rgba, steve_rgba[..alex_rgba.len()]);
    }

    #[test]
    fn renders_scannable_qr_code_rgba_for_verification_url() {
        let (size, rgba) = render_qr_code_rgba("https://microsoft.com/link", 4, 2).unwrap();
        assert_eq!(size[0], size[1]);
        assert!(size[0] >= 80);
        assert_eq!(rgba.len(), size[0] * size[1] * 4);
        let has_dark = rgba.as_chunks::<4>().0.iter().any(|px| px[0] == 15);
        let has_light = rgba.as_chunks::<4>().0.iter().any(|px| px[0] == 255);
        assert!(has_dark && has_light);
    }
}
