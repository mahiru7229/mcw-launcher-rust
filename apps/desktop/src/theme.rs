use eframe::egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, Style,
    TextStyle, Visuals,
};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
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

impl ThemePalette {
    /// Returns a copy whose `surface` / `elevated` fills are translucent so the
    /// launcher background picture shows through floating panels. `canvas`,
    /// borders and text stay opaque (OS windows still paint an opaque canvas).
    pub fn glass(mut self, opacity: f32) -> Self {
        let op = opacity.clamp(0.25, 1.0);
        let surface_a = (op * 255.0).round() as u8;
        let elevated_a = ((op + (1.0 - op) * 0.30) * 255.0).round() as u8;
        let with_alpha =
            |c: Color32, a: u8| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
        self.surface = with_alpha(self.surface, surface_a);
        self.elevated = with_alpha(self.elevated, elevated_a);
        self
    }

    fn preset(
        canvas: u32,
        surface: u32,
        elevated: u32,
        border: u32,
        text: u32,
        muted: u32,
        accent: u32,
    ) -> Self {
        let rgb = |v: u32| Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
        let mut pal = Self {
            canvas: rgb(canvas),
            surface: rgb(surface),
            elevated: rgb(elevated),
            border: rgb(border),
            text: rgb(text),
            text_muted: rgb(muted),
            accent: rgb(accent),
            accent_text: Color32::WHITE,
            warning: Color32::from_rgb(0xE0, 0xA1, 0x3A),
            danger: Color32::from_rgb(0xEF, 0x44, 0x44),
            focus: rgb(accent),
        };
        apply_custom_accent(&mut pal, &format!("#{accent:06x}"));
        pal
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
pub struct ThemeIssue {
    pub file_name: String,
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ThemeCatalog {
    pub themes: Vec<ThemeDefinition>,
    pub issues: Vec<ThemeIssue>,
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
        let preset_def = |id: &str, name: &str, desc: &str, pal: ThemePalette| ThemeDefinition {
            id: id.to_string(),
            name: name.to_string(),
            description: desc.to_string(),
            base_mode: ThemeMode::Dark,
            palette: pal,
            source_path: None,
        };
        let mut themes = vec![
            preset_def(
                "violet",
                "Violet (Default)",
                "Pixel dusk with a violet accent.",
                ThemePalette::preset(
                    0x0E0A1F, 0x181230, 0x241B45, 0x3A2F66, 0xF2EEFF, 0xA79CCB, 0x8B5CF6,
                ),
            ),
            preset_def(
                "sunrise",
                "Sunrise",
                "A warm dawn with a rose accent.",
                ThemePalette::preset(
                    0x1A0C16, 0x2A1422, 0x3B1D2F, 0x5C2D48, 0xFFEEF5, 0xCFA3B8, 0xEC4899,
                ),
            ),
            preset_def(
                "neon",
                "Neon",
                "Near-black with a cyan neon accent.",
                ThemePalette::preset(
                    0x05070A, 0x0D1218, 0x151C25, 0x23303D, 0xE8FBFF, 0x8FB2BD, 0x22D3EE,
                ),
            ),
            preset_def(
                "ocean",
                "Ocean",
                "Deep teal water with a mint accent.",
                ThemePalette::preset(
                    0x06141A, 0x0C222B, 0x133440, 0x1F4C5C, 0xE6FFFA, 0x8FBFB6, 0x14B8A6,
                ),
            ),
            ThemeDefinition {
                id: "dark".to_string(),
                name: "Emerald (Classic Dark)".to_string(),
                description: "Classic MCW Slate & Emerald dark palette.".to_string(),
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

        let mut issues = Vec::new();
        for path in candidates {
            match Self::parse_external(&path) {
                Ok(def) => {
                    if let Some(existing) = themes
                        .iter_mut()
                        .find(|t| t.id.eq_ignore_ascii_case(&def.id))
                    {
                        *existing = def;
                    } else {
                        themes.push(def);
                    }
                }
                Err(message) => {
                    let file_name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("theme.json")
                        .to_string();
                    issues.push(ThemeIssue {
                        file_name,
                        path,
                        message,
                    });
                }
            }
        }

        Self { themes, issues }
    }

    fn parse_external(path: &Path) -> std::result::Result<ThemeDefinition, String> {
        let raw = fs::read_to_string(path).map_err(|e| format!("Failed to read file: {e}"))?;
        let fallback_id = path
            .parent()
            .filter(|_| path.file_name().and_then(|n| n.to_str()) == Some("theme.json"))
            .and_then(|p| p.file_name())
            .or_else(|| path.file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("custom");
        Self::parse_json_str_checked(&raw, fallback_id, Some(path.to_path_buf()))
    }

    pub fn parse_json_str(
        raw: &str,
        fallback_id: &str,
        source_path: Option<PathBuf>,
    ) -> Option<ThemeDefinition> {
        Self::parse_json_str_checked(raw, fallback_id, source_path).ok()
    }

    fn parse_json_str_checked(
        raw: &str,
        fallback_id: &str,
        source_path: Option<PathBuf>,
    ) -> std::result::Result<ThemeDefinition, String> {
        let parsed: ExternalThemeJson =
            serde_json::from_str(raw).map_err(|e| format!("Invalid theme JSON: {e}"))?;
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
        let check_hex = |field: &str,
                         raw_opt: &Option<String>|
         -> std::result::Result<Option<Color32>, String> {
            let Some(ref s) = raw_opt else {
                return Ok(None);
            };
            if s.trim().is_empty() {
                return Ok(None);
            }
            parse_hex_color(s)
                .map(Some)
                .ok_or_else(|| format!("Invalid hex color '{s}' for '{field}' (expected #RRGGBB)"))
        };

        if let Some(c) = check_hex("canvas", &ep.canvas)? {
            pal.canvas = c;
        }
        if let Some(c) = check_hex("surface", &ep.surface)? {
            pal.surface = c;
        }
        if let Some(c) = check_hex("elevated", &ep.elevated)? {
            pal.elevated = c;
        }
        if let Some(c) = check_hex("border", &ep.border)? {
            pal.border = c;
        }
        if let Some(c) = check_hex("text_primary", &ep.text_primary)? {
            pal.text = c;
        }
        if let Some(c) = check_hex("text_muted", &ep.text_muted)? {
            pal.text_muted = c;
        }
        if let Some(ref primary) = ep.primary {
            let _ = check_hex("primary", &Some(primary.clone()))?;
            let _ = apply_custom_accent(&mut pal, primary);
        }
        if let Some(c) = check_hex("primary_text", &ep.primary_text)? {
            pal.accent_text = c;
        }
        if let Some(c) = check_hex("focus", &ep.focus)? {
            pal.focus = c;
        }
        if let Some(c) = check_hex("warning", &ep.warning)? {
            pal.warning = c;
        }
        if let Some(c) = check_hex("error", &ep.error)? {
            pal.danger = c;
        }

        Ok(ThemeDefinition {
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

/// Apply MCW theme palette, motion configuration, and scaled text styles to `egui::Context`.
pub fn apply_theme_palette(
    ctx: &Context,
    mode: ThemeMode,
    p: ThemePalette,
    motion: MotionMode,
    font_size: f32,
) -> ThemePalette {
    let mut visuals = match mode {
        ThemeMode::Dark => Visuals::dark(),
        ThemeMode::Light => Visuals::light(),
    };

    let radius = CornerRadius::same(8);
    visuals.window_corner_radius = CornerRadius::same(14);
    visuals.menu_corner_radius = CornerRadius::same(8);
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
    let base = if font_size.is_finite() {
        font_size.clamp(10.0, 22.0)
    } else {
        13.0
    };
    style.text_styles.insert(
        TextStyle::Small,
        FontId::proportional((base * 0.78).round()),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(base));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(base));
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::monospace((base * 0.92).round()),
    );
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::proportional((base * 1.42).round()),
    );
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

#[derive(Debug, Clone)]
pub struct SystemFontEntry {
    pub name: String,
    pub path: PathBuf,
}

fn read_u16_be(bytes: &[u8], off: usize) -> Option<u16> {
    let slice = bytes.get(off..off + 2)?;
    Some(u16::from_be_bytes([slice[0], slice[1]]))
}

fn read_u32_be(bytes: &[u8], off: usize) -> Option<u32> {
    let slice = bytes.get(off..off + 4)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn decode_sfnt_name_bytes(bytes: &[u8], platform_id: u16) -> Option<String> {
    let text = match platform_id {
        3 | 0 => {
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        }
        1 => bytes
            .iter()
            .filter(|&&b| (0x20..=0x7E).contains(&b))
            .map(|&b| b as char)
            .collect(),
        _ => return None,
    };
    let trimmed = text.trim().trim_matches('\0').trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Reads the Full Font Name (ID 4) or Family Name (ID 1) from an SFNT `.ttf` / `.otf` `name` table.
fn read_sfnt_display_name(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut header = [0u8; 12];
    file.read_exact(&mut header).ok()?;
    let sfnt_ver = read_u32_be(&header, 0)?;
    if sfnt_ver != 0x0001_0000 && sfnt_ver != 0x4F54_544F {
        return None;
    }
    let num_tables = read_u16_be(&header, 4)? as usize;
    if num_tables == 0 || num_tables > 128 {
        return None;
    }
    let mut dir_buf = vec![0u8; num_tables * 16];
    file.read_exact(&mut dir_buf).ok()?;

    let mut name_offset = 0u64;
    let mut name_len = 0usize;
    for i in 0..num_tables {
        let base = i * 16;
        if &dir_buf[base..base + 4] == b"name" {
            name_offset = read_u32_be(&dir_buf, base + 8)? as u64;
            name_len = (read_u32_be(&dir_buf, base + 12)? as usize).min(64 * 1024);
            break;
        }
    }
    if name_offset == 0 || name_len < 6 {
        return None;
    }

    file.seek(SeekFrom::Start(name_offset)).ok()?;
    let mut table = vec![0u8; name_len];
    file.read_exact(&mut table).ok()?;

    let count = read_u16_be(&table, 2)? as usize;
    let string_offset = read_u16_be(&table, 4)? as usize;
    if string_offset >= table.len() {
        return None;
    }

    let mut best_score = -1i32;
    let mut best_name: Option<String> = None;
    for i in 0..count {
        let rec = 6 + i * 12;
        if rec + 12 > table.len() {
            break;
        }
        let platform_id = read_u16_be(&table, rec)?;
        let language_id = read_u16_be(&table, rec + 4)?;
        let name_id = read_u16_be(&table, rec + 6)?;
        let str_len = read_u16_be(&table, rec + 8)? as usize;
        let str_off = read_u16_be(&table, rec + 10)? as usize;

        if !matches!(name_id, 4 | 1 | 16) || str_len == 0 {
            continue;
        }
        let start = string_offset.saturating_add(str_off);
        let end = start.saturating_add(str_len);
        if end > table.len() {
            continue;
        }
        let Some(decoded) = decode_sfnt_name_bytes(&table[start..end], platform_id) else {
            continue;
        };

        let mut score = match name_id {
            4 => 30,
            16 => 20,
            1 => 10,
            _ => 0,
        };
        if platform_id == 3 {
            score += 20;
            if language_id == 0x0409 {
                score += 10;
            }
        } else if platform_id == 0 {
            score += 15;
        } else if platform_id == 1 {
            score += 5;
        }

        if score > best_score {
            best_score = score;
            best_name = Some(decoded);
        }
    }
    best_name
}

fn collect_fonts_in_dir(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) {
    if depth > 3 || !dir.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_fonts_in_dir(&p, depth + 1, out);
        } else if p.is_file() {
            let is_sfnt = p
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("otf"))
                .unwrap_or(false);
            if is_sfnt {
                out.push(p);
            }
        }
    }
}

/// Scans standard OS font folders for `.ttf`/`.otf` fonts sorted alphabetically by display name.
pub fn scan_system_fonts() -> Vec<SystemFontEntry> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        if let Some(win_dir) = std::env::var_os("WINDIR") {
            dirs.push(PathBuf::from(win_dir).join("Fonts"));
        } else {
            dirs.push(PathBuf::from(r"C:\Windows\Fonts"));
        }
        if let Some(local_app) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(
                PathBuf::from(local_app)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
    }
    #[cfg(not(windows))]
    {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join(".local/share/fonts"));
        }
    }

    let mut files = Vec::new();
    for dir in dirs {
        collect_fonts_in_dir(&dir, 0, &mut files);
    }
    files.sort();

    let mut seen_names: HashSet<String> = HashSet::new();
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();
    let mut entries: Vec<SystemFontEntry> = Vec::new();

    for path in files {
        if !seen_paths.insert(path.clone()) {
            continue;
        }
        let fallback = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Font")
            .to_string();
        let name = read_sfnt_display_name(&path).unwrap_or(fallback);
        let key = name.to_ascii_lowercase();
        if seen_names.insert(key) {
            entries.push(SystemFontEntry { name, path });
        }
    }

    entries.sort_by_key(|a| a.name.to_ascii_lowercase());
    entries
}

/// Resolves a stored custom font preference (either an exact display name or file path) to a readable font file.
pub fn resolve_custom_font_path(spec: &str, system_fonts: &[SystemFontEntry]) -> Option<PathBuf> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(hit) = system_fonts
        .iter()
        .find(|f| f.name.eq_ignore_ascii_case(trimmed))
    {
        return Some(hit.path.clone());
    }
    let direct = PathBuf::from(trimmed);
    if direct.is_file() {
        return Some(direct);
    }
    None
}

/// Load system UI font (optional custom font first, then Segoe UI / Noto Sans fallback)
/// plus symbol fallback and monospace font for Live Logs and file paths.
pub fn configure_fonts(ctx: &Context, custom_font_path: Option<&Path>) {
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

    if let Some(custom_path) = custom_font_path {
        if let Ok(bytes) = std::fs::read(custom_path) {
            if !bytes.is_empty() {
                fonts.font_data.insert(
                    "mcw_custom_ui".to_owned(),
                    Arc::new(FontData::from_owned(bytes)),
                );
                fonts
                    .families
                    .entry(FontFamily::Proportional)
                    .or_default()
                    .insert(0, "mcw_custom_ui".to_owned());
            }
        }
    }

    #[cfg(windows)]
    if let Ok(sym_bytes) = std::fs::read(r"C:\Windows\Fonts\seguisym.ttf") {
        fonts.font_data.insert(
            "mcw_symbols".to_owned(),
            Arc::new(FontData::from_owned(sym_bytes)),
        );
        let prop = fonts.families.entry(FontFamily::Proportional).or_default();
        let sym_idx = prop.len().min(2);
        prop.insert(sym_idx, "mcw_symbols".to_owned());
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
        assert_eq!(catalog.themes.len(), 8);
        assert_eq!(
            catalog.find("mcw-default").name,
            "MCW Default (Emerald Block)"
        );
        let custom = catalog.find("emerald-block");
        assert_eq!(custom.name, "Emerald Block");
        assert_eq!(custom.palette.accent, Color32::from_rgb(0x63, 0x98, 0x4a));
        assert!(catalog.issues.is_empty());
    }

    #[test]
    fn records_theme_validation_issues_for_malformed_json() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("broken.json"), "{ not valid json").unwrap();
        fs::write(
            dir.path().join("bad-color.json"),
            r#"{"id":"bad","name":"Bad","palette":{"primary":"not-hex"}}"#,
        )
        .unwrap();

        let catalog = ThemeCatalog::load(dir.path());
        assert_eq!(catalog.issues.len(), 2);
    }
}
