use crate::theme::ThemePalette;
use eframe::egui::{self, CornerRadius, Stroke, Vec2};
use std::path::PathBuf;

pub(crate) fn enable_windows_dpi_awareness() {
    #[cfg(target_os = "windows")]
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetModuleHandleA(lp_module_name: *const u8) -> *mut core::ffi::c_void;
            fn GetProcAddress(
                h_module: *mut core::ffi::c_void,
                lp_proc_name: *const u8,
            ) -> *const core::ffi::c_void;
        }
        #[link(name = "user32")]
        extern "system" {
            fn SetProcessDPIAware() -> i32;
        }

        let user32 = GetModuleHandleA(c"user32.dll".as_ptr().cast::<u8>());
        if !user32.is_null() {
            let proc_addr = GetProcAddress(
                user32,
                c"SetProcessDpiAwarenessContext".as_ptr().cast::<u8>(),
            );
            if !proc_addr.is_null() {
                type SetCtxFn = unsafe extern "system" fn(*mut core::ffi::c_void) -> i32;
                let set_ctx: SetCtxFn = core::mem::transmute(proc_addr);
                // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = (DPI_AWARENESS_CONTEXT)-4
                if set_ctx((-4isize) as *mut core::ffi::c_void) != 0 {
                    return;
                }
            }
        }
        let _ = SetProcessDPIAware();
    }
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn window_size_for_resolution(phys_w: i32, phys_h: i32, dpi_scale: f32) -> [f32; 2] {
    let scale = if dpi_scale.is_finite() && dpi_scale > 0.5 {
        dpi_scale
    } else {
        1.0
    };
    let logical_screen_w = if phys_w > 0 {
        phys_w as f32 / scale
    } else {
        1920.0
    };
    let logical_screen_h = if phys_h > 0 {
        phys_h as f32 / scale
    } else {
        1080.0
    };

    let [target_w, target_h] = if phys_w >= 1920 && phys_h >= 1080 {
        if logical_screen_w >= 1920.0 && logical_screen_h >= 1080.0 {
            [1600.0_f32, 900.0_f32]
        } else {
            [1600.0_f32 / scale, 900.0_f32 / scale]
        }
    } else if phys_w >= 1366 && phys_h >= 768 {
        [1280.0_f32 / scale, 720.0_f32 / scale]
    } else {
        let w = if phys_w > 0 {
            ((phys_w as f32 * 0.88) / scale).clamp(840.0, 1280.0)
        } else {
            1280.0
        };
        let h = if phys_h > 0 {
            ((phys_h as f32 * 0.88) / scale).clamp(560.0, 720.0)
        } else {
            720.0
        };
        [w, h]
    };

    if phys_w > 0 && phys_h > 0 {
        let max_logical_w = (logical_screen_w - 16.0).max(840.0);
        let max_logical_h = (logical_screen_h - 40.0).max(560.0);
        [
            target_w.round().min(max_logical_w).max(840.0),
            target_h.round().min(max_logical_h).max(560.0),
        ]
    } else {
        [target_w.round(), target_h.round()]
    }
}

pub(crate) fn default_window_size_for_screen() -> [f32; 2] {
    #[cfg(target_os = "windows")]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetModuleHandleA(lp_module_name: *const u8) -> *mut core::ffi::c_void;
            fn GetProcAddress(
                h_module: *mut core::ffi::c_void,
                lp_proc_name: *const u8,
            ) -> *const core::ffi::c_void;
        }
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
            let user32 = GetModuleHandleA(c"user32.dll".as_ptr().cast::<u8>());
            if !user32.is_null() {
                let proc_addr = GetProcAddress(user32, c"GetDpiForSystem".as_ptr().cast::<u8>());
                if !proc_addr.is_null() {
                    type GetDpiFn = unsafe extern "system" fn() -> u32;
                    let get_dpi: GetDpiFn = core::mem::transmute(proc_addr);
                    let sys_dpi = get_dpi();
                    if sys_dpi >= 96 {
                        dpi_scale = sys_dpi as f32 / 96.0;
                    }
                }
            }
            if phys_w <= 0 || phys_h <= 0 {
                phys_w = GetSystemMetrics(0);
                phys_h = GetSystemMetrics(1);
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

fn parse_filter_pairs(filter: &str) -> Vec<(String, Vec<String>)> {
    let parts: Vec<&str> = filter.split('|').collect();
    let mut out = Vec::new();
    for &[desc_raw, spec_raw] in parts.as_chunks::<2>().0 {
        let label = desc_raw.trim().to_string();
        let exts: Vec<String> = spec_raw
            .split([';', ','])
            .map(|pat| {
                pat.trim()
                    .trim_start_matches('*')
                    .trim_start_matches('.')
                    .to_ascii_lowercase()
            })
            .filter(|ext| !ext.is_empty() && ext != "*")
            .collect();
        if !exts.is_empty() {
            out.push((label, exts));
        }
    }
    out
}

fn apply_rfd_filters(mut dialog: rfd::FileDialog, filter: &str) -> rfd::FileDialog {
    for (label, exts) in parse_filter_pairs(filter) {
        let ext_refs: Vec<&str> = exts.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(&label, &ext_refs);
    }
    dialog
}

#[cfg(all(unix, not(target_os = "macos")))]
fn has_xdg_desktop_portal() -> bool {
    let Ok(out) = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.DBus",
            "--type=method_call",
            "--print-reply",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus.NameHasOwner",
            "string:org.freedesktop.portal.Desktop",
        ])
        .output()
    else {
        return false;
    };
    out.status.success() && String::from_utf8_lossy(&out.stdout).contains("boolean true")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn run_linux_dialog_cmd(bin: &str, args: &[String]) -> Option<PathBuf> {
    let out = std::process::Command::new(bin).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_fallback_pick_folder(title: &str) -> Option<PathBuf> {
    let zenity_args = vec![
        "--file-selection".to_string(),
        "--directory".to_string(),
        "--title".to_string(),
        title.to_string(),
    ];
    if let Some(p) = run_linux_dialog_cmd("zenity", &zenity_args) {
        return Some(p);
    }
    let kdialog_args = vec![
        "--getexistingdirectory".to_string(),
        std::env::var("HOME").unwrap_or_else(|_| ".".to_string()),
        "--title".to_string(),
        title.to_string(),
    ];
    run_linux_dialog_cmd("kdialog", &kdialog_args)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_fallback_pick_file(title: &str, filter: &str) -> Option<PathBuf> {
    let mut zenity_args = vec![
        "--file-selection".to_string(),
        "--title".to_string(),
        title.to_string(),
    ];
    let pairs = parse_filter_pairs(filter);
    for (label, exts) in &pairs {
        let pats = exts
            .iter()
            .map(|e| format!("*.{e}"))
            .collect::<Vec<_>>()
            .join(" ");
        zenity_args.push(format!("--file-filter={label} | {pats}"));
    }
    if !pairs.is_empty() {
        zenity_args.push("--file-filter=All files | *".to_string());
    }
    if let Some(p) = run_linux_dialog_cmd("zenity", &zenity_args) {
        return Some(p);
    }
    let kdialog_filter = if pairs.is_empty() {
        "*".to_string()
    } else {
        pairs
            .iter()
            .flat_map(|(_, exts)| exts.iter().map(|e| format!("*.{e}")))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let kdialog_args = vec![
        "--getopenfilename".to_string(),
        std::env::var("HOME").unwrap_or_else(|_| ".".to_string()),
        kdialog_filter,
        "--title".to_string(),
        title.to_string(),
    ];
    run_linux_dialog_cmd("kdialog", &kdialog_args)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_fallback_save_file(title: &str, default_filename: &str) -> Option<PathBuf> {
    let zenity_args = vec![
        "--file-selection".to_string(),
        "--save".to_string(),
        "--confirm-overwrite".to_string(),
        "--filename".to_string(),
        default_filename.to_string(),
        "--title".to_string(),
        title.to_string(),
    ];
    if let Some(p) = run_linux_dialog_cmd("zenity", &zenity_args) {
        return Some(p);
    }
    let kdialog_args = vec![
        "--getsavefilename".to_string(),
        default_filename.to_string(),
        "--title".to_string(),
        title.to_string(),
    ];
    run_linux_dialog_cmd("kdialog", &kdialog_args)
}

pub(crate) fn pick_file_dialog(title: &str, filter: &str) -> Option<PathBuf> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if !has_xdg_desktop_portal() {
            return linux_fallback_pick_file(title, filter);
        }
    }
    apply_rfd_filters(rfd::FileDialog::new().set_title(title), filter).pick_file()
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

pub(crate) fn pick_folder_dialog(title: &str) -> Option<PathBuf> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if !has_xdg_desktop_portal() {
            return linux_fallback_pick_folder(title);
        }
    }
    rfd::FileDialog::new().set_title(title).pick_folder()
}

pub(crate) fn pick_folder_dialog_async(
    title: String,
    on_done: impl FnOnce(Option<PathBuf>) + Send + 'static,
) {
    std::thread::spawn(move || {
        let picked = pick_folder_dialog(&title);
        on_done(picked);
    });
}

pub(crate) fn save_file_dialog(
    title: &str,
    filter: &str,
    default_filename: &str,
) -> Option<PathBuf> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if !has_xdg_desktop_portal() {
            return linux_fallback_save_file(title, default_filename);
        }
    }
    apply_rfd_filters(
        rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(default_filename),
        filter,
    )
    .save_file()
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

pub(crate) fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn try_spawn_launcher(program: &str, args: &[&str]) -> std::result::Result<(), String> {
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    std::thread::sleep(std::time::Duration::from_millis(90));
    match child.try_wait() {
        Ok(Some(status)) if !status.success() => Err(format!(
            "{program} exited with code {}",
            status.code().unwrap_or(-1)
        )),
        Ok(_) => Ok(()),
        Err(e) => Err(format!("{program}: {e}")),
    }
}

pub(crate) fn open_external_result(target: &str) -> std::result::Result<(), String> {
    if target.starts_with("http://") || target.starts_with("https://") {
        return webbrowser::open(target).map_err(|e| format!("Failed to open URL: {e}"));
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("explorer.exe failed: {e}"))
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("open failed: {e}"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let candidates: Vec<(&str, Vec<&str>)> = vec![
            ("xdg-open", vec![target]),
            ("gio", vec!["open", target]),
            ("kioclient", vec!["exec", target]),
            ("kioclient5", vec!["exec", target]),
            ("kde-open5", vec![target]),
            ("kde-open", vec![target]),
        ];
        let mut errors = Vec::new();
        for (prog, args) in &candidates {
            match try_spawn_launcher(prog, args) {
                Ok(()) => return Ok(()),
                Err(e) => errors.push(e),
            }
        }
        let target_path = std::path::Path::new(target);
        let folder = if target_path.is_file() {
            target_path.parent().unwrap_or(target_path)
        } else {
            target_path
        };
        let folder_str = folder.to_string_lossy().into_owned();
        for fm in ["nautilus", "dolphin", "thunar", "nemo", "pcmanfm", "caja"] {
            match try_spawn_launcher(fm, &[folder_str.as_str()]) {
                Ok(()) => return Ok(()),
                Err(e) => errors.push(e),
            }
        }
        Err(format!(
            "Could not open '{}': {}",
            target,
            errors.join("; ")
        ))
    }
}

pub(crate) fn open_external(target: &str) {
    if let Err(err) = open_external_result(target) {
        tracing::warn!(target = %target, error = %err, "External open failed");
    }
}

pub(crate) fn sanitize_shortcut_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        "Minecraft Instance".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn create_desktop_shortcut(
    exe_path: &std::path::Path,
    work_dir: &std::path::Path,
    instance_id: &str,
    instance_name: &str,
    icon_path: Option<&std::path::Path>,
) -> std::result::Result<std::path::PathBuf, String> {
    let safe_name = sanitize_shortcut_filename(instance_name);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let ps_escape = |s: &str| s.replace('\'', "''");
        let exe_s = ps_escape(&exe_path.display().to_string());
        let dir_s = ps_escape(&work_dir.display().to_string());
        let id_arg = ps_escape(&format!("--launch \"{instance_id}\""));
        let desc_s = ps_escape(&format!("Launch {instance_name} via MCW Launcher"));
        let lnk_name_s = ps_escape(&format!("{safe_name}.lnk"));
        let icon_src = icon_path
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ico")))
            .unwrap_or(exe_path);
        let icon_s = ps_escape(&format!("{},0", icon_src.display()));

        let script = format!(
            "$desktop = [Environment]::GetFolderPath('Desktop'); \
             $lnk = Join-Path $desktop '{lnk_name_s}'; \
             $ws = New-Object -ComObject WScript.Shell; \
             $sc = $ws.CreateShortcut($lnk); \
             $sc.TargetPath = '{exe_s}'; \
             $sc.Arguments = '{id_arg}'; \
             $sc.WorkingDirectory = '{dir_s}'; \
             $sc.Description = '{desc_s}'; \
             $sc.IconLocation = '{icon_s}'; \
             $sc.Save(); \
             Write-Output $lnk"
        );
        let out = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to invoke PowerShell: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(format!("Shortcut creation failed: {}", err.trim()));
        }
        let created = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if created.is_empty() {
            return Err("PowerShell returned empty shortcut path.".to_string());
        }
        Ok(std::path::PathBuf::from(created))
    }
    #[cfg(not(windows))]
    {
        let desktop_dir = std::env::var_os("XDG_DESKTOP_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Desktop"))
            })
            .ok_or_else(|| "Could not determine Desktop folder.".to_string())?;
        std::fs::create_dir_all(&desktop_dir)
            .map_err(|e| format!("Failed to create Desktop directory: {e}"))?;
        let dest = desktop_dir.join(format!("{safe_name}.desktop"));
        let icon_line = icon_path
            .filter(|p| p.is_file())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "minecraft".to_string());
        let body = format!(
            "[Desktop Entry]\nType=Application\nName={}\nComment=Launch {} via MCW Launcher\nExec=\"{}\" --launch \"{}\"\nPath={}\nIcon={}\nTerminal=false\nCategories=Game;\n",
            safe_name,
            instance_name,
            exe_path.display(),
            instance_id,
            work_dir.display(),
            icon_line,
        );
        std::fs::write(&dest, body).map_err(|e| format!("Failed to write .desktop file: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
        }
        Ok(dest)
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_filter_pairs, window_size_for_resolution};

    #[test]
    fn parses_win32_filter_strings_for_rfd() {
        let parsed = parse_filter_pairs("Modpack (*.mrpack;*.zip)|*.mrpack;*.zip|All (*.*)|*.*");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].0, "Modpack (*.mrpack;*.zip)");
        assert_eq!(parsed[0].1, vec!["mrpack", "zip"]);
    }

    #[test]
    fn selects_expected_default_window_sizes_for_1080p_and_768p() {
        assert_eq!(window_size_for_resolution(1920, 1080, 1.0), [1600.0, 900.0]);
        assert_eq!(
            window_size_for_resolution(1920, 1080, 1.25),
            [1280.0, 720.0]
        );
        assert_eq!(window_size_for_resolution(2560, 1440, 1.0), [1600.0, 900.0]);
        assert_eq!(window_size_for_resolution(3840, 2160, 2.0), [1600.0, 900.0]);
        assert_eq!(window_size_for_resolution(1366, 768, 1.0), [1280.0, 720.0]);
        let scaled = window_size_for_resolution(1366, 768, 1.25);
        assert!(scaled[0] <= (1366.0 / 1.25) - 16.0);
        assert!(scaled[1] <= (768.0 / 1.25) - 40.0);
    }
}
