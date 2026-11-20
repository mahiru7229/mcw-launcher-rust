use crate::theme::ThemePalette;
use eframe::egui::{self, CornerRadius, Stroke, Vec2};
use std::path::PathBuf;

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
            CornerRadius::same(3),
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
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
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
