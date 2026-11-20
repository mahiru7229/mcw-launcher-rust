use eframe::egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Style, Visuals,
};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MotionMode {
    #[default]
    Full,
    Reduced,
    Off,
}

impl MotionMode {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "reduced" => Self::Reduced,
            "off" => Self::Off,
            _ => Self::Full,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Reduced => "reduced",
            Self::Off => "off",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Full => "Full (Default)",
            Self::Reduced => "Reduced",
            Self::Off => "Off (Instant)",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ThemePalette {
    pub canvas: Color32,
    pub surface: Color32,
    pub elevated: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub warning: Color32,
    pub danger: Color32,
    pub focus: Color32,
}

impl ThemePalette {
    pub fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self {
                canvas: Color32::from_rgb(0x11, 0x13, 0x17),
                surface: Color32::from_rgb(0x18, 0x1B, 0x21),
                elevated: Color32::from_rgb(0x20, 0x24, 0x2C),
                border: Color32::from_rgb(0x2B, 0x30, 0x3B),
                text: Color32::from_rgb(0xE6, 0xE9, 0xEF),
                text_muted: Color32::from_rgb(0x8E, 0x96, 0xA6),
                accent: Color32::from_rgb(0x10, 0xB9, 0x81),
                accent_text: Color32::from_rgb(0x09, 0x15, 0x10),
                warning: Color32::from_rgb(0xD9, 0x77, 0x06),
                danger: Color32::from_rgb(0xDC, 0x26, 0x26),
                focus: Color32::from_rgb(0x34, 0xD3, 0x99),
            },
            ThemeMode::Light => Self {
                canvas: Color32::from_rgb(0xF7, 0xF8, 0xFA),
                surface: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                elevated: Color32::from_rgb(0xEE, 0xF0, 0xF4),
                border: Color32::from_rgb(0xD6, 0xDB, 0xE3),
                text: Color32::from_rgb(0x18, 0x1B, 0x21),
                text_muted: Color32::from_rgb(0x5A, 0x63, 0x74),
                accent: Color32::from_rgb(0x05, 0x96, 0x69),
                accent_text: Color32::WHITE,
                warning: Color32::from_rgb(0xD9, 0x77, 0x06),
                danger: Color32::from_rgb(0xDC, 0x26, 0x26),
                focus: Color32::from_rgb(0x05, 0x96, 0x69),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThemeDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub base_mode: ThemeMode,
    pub palette: ThemePalette,
    pub source_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ThemeCatalog {
    pub themes: Vec<ThemeDefinition>,
}

#[derive(Debug, Deserialize, Default)]
struct ExternalThemeJson {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    base_mode: String,
    #[serde(default)]
    palette: ExternalThemePaletteJson,
}

#[derive(Debug, Deserialize, Default)]
struct ExternalThemePaletteJson {
    #[serde(default)]
    primary: Option<String>,
    #[serde(default)]
    primary_text: Option<String>,
    #[serde(default)]
    focus: Option<String>,
    #[serde(default)]
    warning: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    text_primary: Option<String>,
    #[serde(default)]
    text_muted: Option<String>,
    #[serde(default)]
    canvas: Option<String>,
    #[serde(default)]
    surface: Option<String>,
    #[serde(default)]
    elevated: Option<String>,
    #[serde(default)]
    border: Option<String>,
}

const EMBEDDED_MCW_DEFAULT_THEME_JSON: &str = r##"{
  "id": "mcw-default",
  "name": "MCW Default (Emerald Block)",
  "description": "Default MCW palette ported from MCW Launcher Python themes/mcw-default.",
  "palette": {
    "primary": "#63984a",
    "primary_text": "#ffffff",
    "focus": "#8ed35b",
    "warning": "#d6a93c",
    "error": "#c47a7a",
    "text_primary": "#f4f4f4",
    "text_muted": "#b8b8b8"
  }
}"##;

impl ThemeCatalog {
    pub fn load(theme_root: &Path) -> Self {
        let mut themes = vec![
            ThemeDefinition {
                id: "dark".to_string(),
                name: "Dark (Default)".to_string(),
                description: "Built-in MCW Slate & Emerald dark palette.".to_string(),
                base_mode: ThemeMode::Dark,
                palette: ThemePalette::for_mode(ThemeMode::Dark),
                source_path: None,
            },
            ThemeDefinition {
                id: "light".to_string(),
                name: "Light".to_string(),
                description: "Built-in clean bright MCW palette.".to_string(),
                base_mode: ThemeMode::Light,
                palette: ThemePalette::for_mode(ThemeMode::Light),
                source_path: None,
            },
        ];

        if let Some(mcw_def) =
            Self::parse_json_str(EMBEDDED_MCW_DEFAULT_THEME_JSON, "mcw-default", None)
        {
            themes.push(mcw_def);
        }

        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(entries) = fs::read_dir(theme_root) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file()
                    && p.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
                {
                    candidates.push(p);
                } else if p.is_dir() {
                    let manifest = p.join("theme.json");
                    if manifest.is_file() {
                        candidates.push(manifest);
                    }
                }
            }
        }
        candidates.sort();

        for path in candidates {
            if let Some(def) = Self::parse_external(&path) {
                if let Some(existing) = themes
                    .iter_mut()
                    .find(|t| t.id.eq_ignore_ascii_case(&def.id))
                {
                    *existing = def;
                } else {
                    themes.push(def);
                }
            }
        }

        Self { themes }
    }

    fn parse_external(path: &Path) -> Option<ThemeDefinition> {
        let raw = fs::read_to_string(path).ok()?;
        let fallback_id = path
            .parent()
            .filter(|_| path.file_name().and_then(|n| n.to_str()) == Some("theme.json"))
            .and_then(|p| p.file_name())
            .or_else(|| path.file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("custom");
        Self::parse_json_str(&raw, fallback_id, Some(path.to_path_buf()))
    }

    pub fn parse_json_str(
        raw: &str,
        fallback_id: &str,
        source_path: Option<PathBuf>,
    ) -> Option<ThemeDefinition> {
        let parsed: ExternalThemeJson = serde_json::from_str(raw).ok()?;
        let id = if parsed.id.trim().is_empty() {
            fallback_id.to_string()
        } else {
            parsed.id.trim().to_string()
        };
        let name = if parsed.name.trim().is_empty() {
            id.clone()
        } else {
            parsed.name.trim().to_string()
        };
        let base_mode = if parsed.base_mode.trim().eq_ignore_ascii_case("light") {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };

        let mut pal = ThemePalette::for_mode(base_mode);
        let ep = &parsed.palette;
        if let Some(c) = ep.canvas.as_deref().and_then(parse_hex_color) {
            pal.canvas = c;
        }
        if let Some(c) = ep.surface.as_deref().and_then(parse_hex_color) {
            pal.surface = c;
        }
        if let Some(c) = ep.elevated.as_deref().and_then(parse_hex_color) {
            pal.elevated = c;
        }
        if let Some(c) = ep.border.as_deref().and_then(parse_hex_color) {
            pal.border = c;
        }
        if let Some(c) = ep.text_primary.as_deref().and_then(parse_hex_color) {
            pal.text = c;
        }
        if let Some(c) = ep.text_muted.as_deref().and_then(parse_hex_color) {
            pal.text_muted = c;
        }
        if let Some(ref primary) = ep.primary {
            let _ = apply_custom_accent(&mut pal, primary);
        }
        if let Some(c) = ep.primary_text.as_deref().and_then(parse_hex_color) {
            pal.accent_text = c;
        }
        if let Some(c) = ep.focus.as_deref().and_then(parse_hex_color) {
            pal.focus = c;
        }
        if let Some(c) = ep.warning.as_deref().and_then(parse_hex_color) {
            pal.warning = c;
        }
        if let Some(c) = ep.error.as_deref().and_then(parse_hex_color) {
            pal.danger = c;
        }

        Some(ThemeDefinition {
            id,
            name,
            description: parsed.description.trim().to_string(),
            base_mode,
            palette: pal,
            source_path,
        })
    }

    pub fn find(&self, id: &str) -> &ThemeDefinition {
        let needle = id.trim();
        self.themes
            .iter()
            .find(|t| t.id.eq_ignore_ascii_case(needle))
            .unwrap_or(&self.themes[0])
    }
}

/// Normalize `#RRGGBB` or `RRGGBB` hex colors matching Python `normalize_hex_color`.
pub fn parse_hex_color(raw: &str) -> Option<Color32> {
    let hex = raw.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color32::from_rgb(r, g, b))
}

/// Format Color32 as `#rrggbb`.
pub fn format_hex_color(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

/// Derive contrast-safe `accent_text` and focus tint from a user `#RRGGBB` accent hex color
/// matching Python `src/core/theme/theme_palette.py::derive_custom_accent`.
pub fn apply_custom_accent(palette: &mut ThemePalette, hex: &str) -> bool {
    let Some(c) = parse_hex_color(hex) else {
        return false;
    };
    let r = c.r() as f32;
    let g = c.g() as f32;
    let b = c.b() as f32;
    let luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255.0;

    palette.accent = c;
    palette.accent_text = if luminance > 0.45 {
        Color32::from_rgb(0x11, 0x11, 0x11)
    } else {
        Color32::WHITE
    };
    palette.focus = Color32::from_rgb(
        (c.r() as f32 + (255.0 - c.r() as f32) * 0.25)
            .round()
            .clamp(0.0, 255.0) as u8,
        (c.g() as f32 + (255.0 - c.g() as f32) * 0.25)
            .round()
            .clamp(0.0, 255.0) as u8,
        (c.b() as f32 + (255.0 - c.b() as f32) * 0.25)
            .round()
            .clamp(0.0, 255.0) as u8,
    );
    true
}

/// Apply MCW theme palette and motion configuration to `egui::Context`.
pub fn apply_theme_palette(
    ctx: &Context,
    mode: ThemeMode,
    p: ThemePalette,
    motion: MotionMode,
) -> ThemePalette {
    let mut visuals = match mode {
        ThemeMode::Dark => Visuals::dark(),
        ThemeMode::Light => Visuals::light(),
    };

    let radius = CornerRadius::same(3);
    visuals.window_corner_radius = CornerRadius::same(4);
    visuals.menu_corner_radius = CornerRadius::same(3);
    visuals.window_fill = p.surface;
    visuals.panel_fill = p.canvas;
    visuals.faint_bg_color = p.elevated;
    visuals.extreme_bg_color = p.canvas;
    visuals.override_text_color = Some(p.text);
    visuals.selection.bg_fill = p.accent.gamma_multiply(0.22);
    visuals.selection.stroke = Stroke::new(1.0_f32, p.focus);

    visuals.widgets.noninteractive.bg_fill = p.surface;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    visuals.widgets.noninteractive.corner_radius = radius;

    visuals.widgets.inactive.bg_fill = p.elevated;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);
    visuals.widgets.inactive.corner_radius = radius;

    visuals.widgets.hovered.bg_fill = p.elevated;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.accent);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, p.text);
    visuals.widgets.hovered.corner_radius = radius;

    visuals.widgets.active.bg_fill = p.accent.gamma_multiply(0.25);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, p.accent);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.text);
    visuals.widgets.active.corner_radius = radius;

    let mut style = Style {
        visuals,
        ..Style::default()
    };
    style.spacing.item_spacing = eframe::egui::vec2(8.0, 6.0);
    style.spacing.button_padding = eframe::egui::vec2(12.0, 6.0);
    style.spacing.interact_size = eframe::egui::vec2(40.0, 26.0);
    style.spacing.window_margin = eframe::egui::Margin::same(12);
    style.animation_time = match motion {
        MotionMode::Full => 0.12,
        MotionMode::Reduced => 0.04,
        MotionMode::Off => 0.0,
    };
    ctx.set_style(style);

    p
}

/// Apply MCW DESIGN.md built-in palette tokens.
#[allow(dead_code)]
pub fn apply_theme(ctx: &Context, mode: ThemeMode) -> ThemePalette {
    apply_theme_palette(ctx, mode, ThemePalette::for_mode(mode), MotionMode::Full)
}

/// Load system UI font (Segoe UI on Windows, Noto Sans on Linux) so Vietnamese diacritics
/// render sharply, plus a system monospace font for Live Logs and file paths.
pub fn configure_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();

    #[cfg(windows)]
    let ui_candidates = [
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
    ];
    #[cfg(not(windows))]
    let ui_candidates = [
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];

    for path in ui_candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("mcw_ui".to_owned(), Arc::new(FontData::from_owned(bytes)));
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "mcw_ui".to_owned());
            break;
        }
    }

    #[cfg(windows)]
    if let Ok(sym_bytes) = std::fs::read(r"C:\Windows\Fonts\seguisym.ttf") {
        fonts.font_data.insert(
            "mcw_symbols".to_owned(),
            Arc::new(FontData::from_owned(sym_bytes)),
        );
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(1, "mcw_symbols".to_owned());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push("mcw_symbols".to_owned());
    }

    #[cfg(windows)]
    let mono_candidates = [
        r"C:\Windows\Fonts\CascadiaMono.ttf",
        r"C:\Windows\Fonts\consola.ttf",
    ];
    #[cfg(not(windows))]
    let mono_candidates = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
    ];

    for path in mono_candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("mcw_mono".to_owned(), Arc::new(FontData::from_owned(bytes)));
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .insert(0, "mcw_mono".to_owned());
            break;
        }
    }

    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn derives_custom_accent_contrast_text_and_parses_external_theme_json() {
        let mut pal = ThemePalette::for_mode(ThemeMode::Dark);
        assert!(apply_custom_accent(&mut pal, "#8ed35b"));
        assert_eq!(pal.accent, Color32::from_rgb(0x8e, 0xd3, 0x5b));
        assert_eq!(pal.accent_text, Color32::from_rgb(0x11, 0x11, 0x11));

        assert!(apply_custom_accent(&mut pal, "1e3a8a"));
        assert_eq!(pal.accent_text, Color32::WHITE);
        assert!(!apply_custom_accent(&mut pal, "invalid"));

        let dir = tempdir().unwrap();
        let pack_dir = dir.path().join("emerald-block");
        fs::create_dir_all(&pack_dir).unwrap();
        fs::write(
            pack_dir.join("theme.json"),
            r##"{
                "id": "emerald-block",
                "name": "Emerald Block",
                "palette": {
                    "primary": "#63984a",
                    "primary_text": "#ffffff",
                    "warning": "#d6a93c"
                }
            }"##,
        )
        .unwrap();

        let catalog = ThemeCatalog::load(dir.path());
        assert_eq!(catalog.themes.len(), 4);
        assert_eq!(
            catalog.find("mcw-default").name,
            "MCW Default (Emerald Block)"
        );
        let custom = catalog.find("emerald-block");
        assert_eq!(custom.name, "Emerald Block");
        assert_eq!(custom.palette.accent, Color32::from_rgb(0x63, 0x98, 0x4a));
    }
}
