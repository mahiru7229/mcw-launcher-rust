use eframe::egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Style, Visuals,
};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
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

/// Apply MCW DESIGN.md tokens: Slate neutrals, Emerald accent, sharp 2-4px radii, 1px borders.
pub fn apply_theme(ctx: &Context, mode: ThemeMode) -> ThemePalette {
    let p = ThemePalette::for_mode(mode);
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
    ctx.set_style(style);

    p
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
