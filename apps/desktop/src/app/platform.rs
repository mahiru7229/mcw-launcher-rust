use crate::theme::ThemePalette;
use eframe::egui::{self, CornerRadius, Stroke, Vec2};
use std::path::PathBuf;

pub(crate) fn primary_screen_size(pixels_per_point: f32) -> Vec2 {
    #[cfg(target_os = "windows")]
    {
        #[link(name = "user32")]
        extern "system" {
            fn GetSystemMetrics(n_index: i32) -> i32;
        }
        let scale = if pixels_per_point > 0.5 {
            pixels_per_point
        } else {
            1.0
        };
        // SM_CXSCREEN = 0, SM_CYSCREEN = 1
        let w = unsafe { GetSystemMetrics(0) };
        let h = unsafe { GetSystemMetrics(1) };
        if w > 0 && h > 0 {
            return Vec2::new((w as f32) / scale, (h as f32) / scale);
        }
    }
    let _ = pixels_per_point;
    Vec2::new(1920.0, 1080.0)
}

/// Builds an OS viewport (`egui::ViewportBuilder`) positioned at the center of the active screen.
pub(crate) fn centered_viewport_builder(
    ctx: &egui::Context,
    title: impl Into<String>,
    size: [f32; 2],
    min_size: [f32; 2],
) -> egui::ViewportBuilder {
    let screen = ctx
        .input(|i| i.viewport().monitor_size)
        .filter(|s| s.x > 100.0 && s.y > 100.0)
        .unwrap_or_else(|| primary_screen_size(ctx.pixels_per_point()));
    let x = ((screen.x - size[0]) * 0.5).max(24.0);
    let y = ((screen.y - size[1]) * 0.5).max(24.0);
    egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size(size)
        .with_min_inner_size(min_size)
        .with_position(egui::pos2(x, y))
}

/// Renders a responsive pill badge for Group or Tag that automatically sizes to the exact width of its text.
pub(crate) fn render_badge_pill(
    ui: &mut egui::Ui,
    text: &str,
    is_group: bool,
    small: bool,
    p: ThemePalette,
) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    let font_size = if small { 10.0 } else { 11.0 };
    let pad_x = if small { 5.5 } else { 7.5 };
    let pad_y = if small { 1.5 } else { 2.5 };
    let text_col = if is_group { p.accent } else { p.text };
    let bg_col = if is_group {
        p.accent.gamma_multiply(0.15)
    } else {
        p.elevated
    };
    let border_col = if is_group { p.accent } else { p.border };

    let galley = ui.painter().layout_no_wrap(
        trimmed.to_string(),
        egui::FontId::monospace(font_size),
        text_col,
    );
    let desired = Vec2::new(galley.size().x + pad_x * 2.0, galley.size().y + pad_y * 2.0);
    let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect(
            rect,
            CornerRadius::same(8),
            bg_col,
            Stroke::new(1.0_f32, border_col),
            egui::StrokeKind::Inside,
        );
        let text_pos = egui::pos2(rect.min.x + pad_x, rect.center().y - galley.size().y * 0.5);
        painter.galley(text_pos, galley, text_col);
    }
}

/// Computes a responsive width for an `egui::ComboBox` that automatically expands/shrinks
/// to fit `selected_text` (plus the dropdown arrow), clamped between `min_w` and `max_w`.
pub(crate) fn auto_combo_width(ui: &egui::Ui, selected_text: &str, min_w: f32, max_w: f32) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        selected_text.to_string(),
        egui::FontId::proportional(13.0),
        egui::Color32::WHITE,
    );
    (galley.size().x + 34.0).clamp(min_w, max_w.max(min_w))
}

pub(crate) fn pick_file_dialog(title: &str, filter: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let safe_title = title.replace('\'', "''");
        let safe_filter = filter.replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $d = New-Object System.Windows.Forms.OpenFileDialog; \
             $d.Title = '{safe_title}'; \
             $d.Filter = '{safe_filter}'; \
             if ($d.ShowDialog() -eq 'OK') {{ Write-Output $d.FileName }}"
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-STA", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(PathBuf::from(s))
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (title, filter);
        None
    }
}

pub(crate) fn save_file_dialog(
    title: &str,
    filter: &str,
    default_filename: &str,
) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let safe_title = title.replace('\'', "''");
        let safe_filter = filter.replace('\'', "''");
        let safe_file = default_filename.replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $d = New-Object System.Windows.Forms.SaveFileDialog; \
             $d.Title = '{safe_title}'; \
             $d.Filter = '{safe_filter}'; \
             $d.FileName = '{safe_file}'; \
             $d.OverwritePrompt = $true; \
             if ($d.ShowDialog() -eq 'OK') {{ Write-Output $d.FileName }}"
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-STA", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(PathBuf::from(s))
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (title, filter);
        Some(PathBuf::from(default_filename))
    }
}

pub(crate) fn format_speed(bps: f64) -> String {
    if bps >= 1024.0 * 1024.0 {
        format!("{:.2} MB/s", bps / (1024.0 * 1024.0))
    } else if bps >= 1024.0 {
        format!("{:.1} KB/s", bps / 1024.0)
    } else {
        format!("{:.0} B/s", bps)
    }
}

/// Paints a crisp 8-tooth vector gear icon button (cross-platform, never relies on emoji/symbol font glyphs).
pub(crate) fn gear_icon_button(ui: &mut egui::Ui, p: ThemePalette, active: bool) -> egui::Response {
    let size = Vec2::new(28.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let bg = if active {
            p.accent
        } else if resp.hovered() {
            p.elevated
        } else {
            p.surface
        };
        let border = if active || resp.hovered() {
            p.accent
        } else {
            p.border
        };
        let icon_col = if active { p.accent_text } else { p.text };

        let painter = ui.painter();
        painter.rect(
            rect,
            CornerRadius::same(8),
            bg,
            Stroke::new(1.0_f32, border),
            egui::StrokeKind::Inside,
        );

        let center = rect.center();
        let outer_r = 7.2_f32;
        let inner_r = 4.6_f32;
        let hole_r = 2.2_f32;
        let teeth = 8;

        for i in 0..teeth {
            let angle = (i as f32) * std::f32::consts::TAU / (teeth as f32);
            let dir = Vec2::new(angle.cos(), angle.sin());
            let p1 = center + dir * (inner_r - 0.5);
            let p2 = center + dir * outer_r;
            painter.line_segment([p1, p2], Stroke::new(2.2_f32, icon_col));
        }
        painter.circle_stroke(center, inner_r, Stroke::new(1.8_f32, icon_col));
        painter.circle_filled(center, hole_r, bg);
    }
    resp
}

/// Paints a crisp 5-pointed vector star button for instance favorites (never relies on font glyphs).
pub(crate) fn star_icon_button(
    ui: &mut egui::Ui,
    p: ThemePalette,
    favorite: bool,
) -> egui::Response {
    let size = Vec2::new(20.0, 20.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if resp.hovered() {
            painter.rect_filled(rect, CornerRadius::same(6), p.surface);
        }
        let col = if favorite {
            p.warning
        } else if resp.hovered() {
            p.text
        } else {
            p.text_muted
        };
        let center = rect.center();
        let outer_r = 6.8_f32;
        let inner_r = 2.9_f32;
        let mut pts = Vec::with_capacity(11);
        for i in 0..10 {
            let r = if i % 2 == 0 { outer_r } else { inner_r };
            let angle = -std::f32::consts::FRAC_PI_2 + (i as f32) * std::f32::consts::PI / 5.0_f32;
            pts.push(center + Vec2::new(angle.cos(), angle.sin()) * r);
        }
        if let Some(&first) = pts.first() {
            pts.push(first);
        }
        let stroke_w = if favorite { 1.8_f32 } else { 1.3_f32 };
        for pair in pts.windows(2) {
            painter.line_segment([pair[0], pair[1]], Stroke::new(stroke_w, col));
        }
        if favorite {
            painter.circle_filled(center, 2.3_f32, col);
        }
    }
    resp
}

pub(crate) fn default_user_downloads_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Downloads"))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Downloads"))
    }
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

pub(crate) fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

pub(crate) fn open_external(target: &str) {
    if target.starts_with("http://") || target.starts_with("https://") {
        let _ = webbrowser::open(target);
        return;
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer.exe")
            .arg(target)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(target).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(target).spawn();
    }
}
