use crate::theme::ThemePalette;
use eframe::egui::{self, CornerRadius, Stroke, Vec2};
use std::path::PathBuf;

pub(crate) fn window_size_for_resolution(phys_w: i32, phys_h: i32, dpi_scale: f32) -> [f32; 2] {
    let scale = if dpi_scale.is_finite() && dpi_scale > 0.5 {
        dpi_scale
    } else {
        1.0
    };
    let [target_w, target_h] = if phys_w >= 1920 && phys_h >= 1080 {
        [1600.0_f32, 900.0_f32]
    } else if phys_w >= 1366 && phys_h >= 768 {
        [1280.0_f32, 720.0_f32]
    } else {
        let w = if phys_w > 0 {
            (phys_w as f32 * 0.88).clamp(840.0, 1280.0)
        } else {
            1280.0
        };
        let h = if phys_h > 0 {
            (phys_h as f32 * 0.88).clamp(560.0, 720.0)
        } else {
            720.0
        };
        [w, h]
    };

    if phys_w > 0 && phys_h > 0 {
        let max_logical_w = ((phys_w as f32 / scale) - 16.0).max(840.0);
        let max_logical_h = ((phys_h as f32 / scale) - 40.0).max(560.0);
        [
            target_w.min(max_logical_w).max(840.0),
            target_h.min(max_logical_h).max(560.0),
        ]
    } else {
        [target_w, target_h]
    }
}

pub(crate) fn default_window_size_for_screen() -> [f32; 2] {
    #[cfg(target_os = "windows")]
    {
        #[link(name = "user32")]
        extern "system" {
            fn GetDC(hwnd: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
            fn ReleaseDC(hwnd: *mut core::ffi::c_void, hdc: *mut core::ffi::c_void) -> i32;
            fn GetSystemMetrics(n_index: i32) -> i32;
        }
        #[link(name = "gdi32")]
        extern "system" {
            fn GetDeviceCaps(hdc: *mut core::ffi::c_void, index: i32) -> i32;
        }
        const DESKTOPVERTRES: i32 = 117;
        const DESKTOPHORZRES: i32 = 118;
        const LOGPIXELSX: i32 = 88;
        unsafe {
            let hdc = GetDC(core::ptr::null_mut());
            let mut phys_w = 0;
            let mut phys_h = 0;
            let mut dpi_scale = 1.0_f32;
            if !hdc.is_null() {
                phys_w = GetDeviceCaps(hdc, DESKTOPHORZRES);
                phys_h = GetDeviceCaps(hdc, DESKTOPVERTRES);
                let dpi_x = GetDeviceCaps(hdc, LOGPIXELSX);
                if dpi_x > 0 {
                    dpi_scale = dpi_x as f32 / 96.0;
                }
                ReleaseDC(core::ptr::null_mut(), hdc);
            }
            if phys_w <= 0 || phys_h <= 0 {
                phys_w = GetSystemMetrics(0);
                phys_h = GetSystemMetrics(1);
                dpi_scale = 1.0;
            }
            if phys_w > 0 && phys_h > 0 {
                return window_size_for_resolution(phys_w, phys_h, dpi_scale);
            }
        }
    }
    [1600.0, 900.0]
}

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

pub(crate) fn display_loader_name(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" => String::new(),
        "vanilla" => "Vanilla".to_string(),
        "fabric" => "Fabric".to_string(),
        "quilt" => "Quilt".to_string(),
        "forge" => "Forge".to_string(),
        "neoforge" => "NeoForge".to_string(),
        "all" => "All Loaders".to_string(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        }
    }
}

pub(crate) fn display_version_type(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "release" => "Release".to_string(),
        "snapshot" => "Snapshot".to_string(),
        "old_beta" => "Old Beta".to_string(),
        "old_alpha" => "Old Alpha".to_string(),
        "all" => "All".to_string(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        }
    }
}

/// Restores (un-minimizes) and focuses an already-open OS viewport so users never lose
/// secondary windows behind the main window.
pub(crate) fn focus_viewport(
    ctx: &egui::Context,
    vp_id: egui::ViewportId,
    title_hint: Option<&str>,
) {
    ctx.send_viewport_cmd_to(vp_id, egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd_to(vp_id, egui::ViewportCommand::Focus);
    #[cfg(target_os = "windows")]
    if let Some(title) = title_hint {
        let mut wide: Vec<u16> = title.encode_utf16().collect();
        wide.push(0);
        #[link(name = "user32")]
        extern "system" {
            fn FindWindowW(lp_class_name: *const u16, lp_window_name: *const u16) -> isize;
            fn IsIconic(h_wnd: isize) -> i32;
            fn ShowWindow(h_wnd: isize, n_cmd_show: i32) -> i32;
            fn SetForegroundWindow(h_wnd: isize) -> i32;
        }
        unsafe {
            let hwnd = FindWindowW(std::ptr::null(), wide.as_ptr());
            if hwnd != 0 {
                if IsIconic(hwnd) != 0 {
                    ShowWindow(hwnd, 9);
                }
                SetForegroundWindow(hwnd);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = title_hint;
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
             $owner = New-Object System.Windows.Forms.Form; \
             $owner.TopMost = $true; \
             $d = New-Object System.Windows.Forms.OpenFileDialog; \
             $d.Title = '{safe_title}'; \
             $d.Filter = '{safe_filter}'; \
             $res = $d.ShowDialog($owner); \
             $owner.Dispose(); \
             if ($res -eq 'OK') {{ Write-Output $d.FileName }}"
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

pub(crate) fn pick_file_dialog_async(
    title: String,
    filter: String,
    on_done: impl FnOnce(Option<PathBuf>) + Send + 'static,
) {
    std::thread::spawn(move || {
        let picked = pick_file_dialog(&title, &filter);
        on_done(picked);
    });
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
             $owner = New-Object System.Windows.Forms.Form; \
             $owner.TopMost = $true; \
             $d = New-Object System.Windows.Forms.SaveFileDialog; \
             $d.Title = '{safe_title}'; \
             $d.Filter = '{safe_filter}'; \
             $d.FileName = '{safe_file}'; \
             $d.OverwritePrompt = $true; \
             $res = $d.ShowDialog($owner); \
             $owner.Dispose(); \
             if ($res -eq 'OK') {{ Write-Output $d.FileName }}"
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

#[cfg(test)]
mod tests {
    use super::window_size_for_resolution;

    #[test]
    fn selects_expected_default_window_sizes_for_1080p_and_768p() {
        assert_eq!(window_size_for_resolution(1920, 1080, 1.0), [1600.0, 900.0]);
        assert_eq!(window_size_for_resolution(2560, 1440, 1.0), [1600.0, 900.0]);
        assert_eq!(window_size_for_resolution(1366, 768, 1.0), [1280.0, 720.0]);
        let scaled = window_size_for_resolution(1920, 1080, 1.25);
        assert!(scaled[0] <= (1920.0 / 1.25) - 16.0);
        assert!(scaled[1] <= (1080.0 / 1.25) - 40.0);
    }
}
