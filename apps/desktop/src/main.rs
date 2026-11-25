#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod background;
mod i18n;
mod skin_render;
mod state;
mod theme;

use app::{McwApp, StartupLaunchRequest};
use eframe::egui;
use mcw_storage::LauncherPaths;
use tracing_subscriber::EnvFilter;

pub(crate) fn parse_startup_launch_args<I: IntoIterator<Item = String>>(
    args: I,
) -> Option<StartupLaunchRequest> {
    let mut iter = args.into_iter().skip(1);
    let mut instance_id: Option<String> = None;
    let mut quick_play_world: Option<String> = None;
    let mut quick_play_server: Option<String> = None;

    while let Some(arg) = iter.next() {
        if let Some(val) = arg.strip_prefix("--launch=") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                instance_id = Some(trimmed.to_string());
            }
        } else if arg == "--launch" {
            if let Some(val) = iter.next() {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    instance_id = Some(trimmed.to_string());
                }
            }
        } else if let Some(val) = arg.strip_prefix("--world=") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                quick_play_world = Some(trimmed.to_string());
            }
        } else if arg == "--world" {
            if let Some(val) = iter.next() {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    quick_play_world = Some(trimmed.to_string());
                }
            }
        } else if let Some(val) = arg.strip_prefix("--server=") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                quick_play_server = Some(trimmed.to_string());
            }
        } else if arg == "--server" {
            if let Some(val) = iter.next() {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    quick_play_server = Some(trimmed.to_string());
                }
            }
        }
    }

    instance_id.map(|id| StartupLaunchRequest {
        instance_id: id,
        quick_play_world,
        quick_play_server,
    })
}

fn main() -> eframe::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    app::platform::enable_windows_dpi_awareness();

    let startup_launch = parse_startup_launch_args(std::env::args());

    let raw_root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let (root, relocated_from) = if LauncherPaths::probe_dir_writable(&raw_root) {
        (raw_root, None)
    } else {
        let has_data = LauncherPaths::dir_has_user_data(&raw_root);
        let safe_root = LauncherPaths::default_safe_data_dir();
        (safe_root, Some((raw_root, has_data)))
    };
    let paths = LauncherPaths::portable(root);
    let _ = paths.initialize();
    let prefs = i18n::LauncherPrefs::load(&paths);
    let screen_default = app::platform::default_window_size_for_screen();
    let min_w = 1024.0_f32.min(screen_default[0]);
    let min_h = 640.0_f32.min(screen_default[1]);
    const CURRENT_WINDOW_SIZE_SCHEMA: u32 = 2;
    let init_size = if prefs.window_size_schema_version >= CURRENT_WINDOW_SIZE_SCHEMA
        && prefs.remember_window_size
        && prefs.window_size_customized
        && prefs.window_width >= min_w
        && prefs.window_height >= min_h
    {
        [
            prefs.window_width.clamp(min_w, 3840.0),
            prefs.window_height.clamp(min_h, 2160.0),
        ]
    } else {
        screen_default
    };

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("MCW Launcher: Rust Edition")
        .with_inner_size(init_size)
        .with_min_inner_size([min_w, min_h]);
    if let Some(icon) = load_window_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }

    let options = eframe::NativeOptions {
        centered: true,
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "MCW Launcher: Rust Edition",
        options,
        Box::new(move |cc| {
            Ok(Box::new(McwApp::new(
                cc,
                paths,
                init_size,
                relocated_from,
                startup_launch,
            )))
        }),
    )
}

fn load_window_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../../../assets/icons/mcw_launcher.png");
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_startup_launch_args;

    #[test]
    fn parses_direct_launch_cli_flags() {
        assert_eq!(parse_startup_launch_args(["mcw-desktop".into()]), None);
        let req = parse_startup_launch_args([
            "mcw-desktop".into(),
            "--launch".into(),
            "cobblemon-pack".into(),
            "--world=Survival".into(),
        ])
        .unwrap();
        assert_eq!(req.instance_id, "cobblemon-pack");
        assert_eq!(req.quick_play_world.as_deref(), Some("Survival"));
        assert_eq!(req.quick_play_server, None);
    }
}
