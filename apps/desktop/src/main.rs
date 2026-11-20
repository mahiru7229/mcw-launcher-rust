#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod i18n;
mod skin_render;
mod state;
mod theme;

use app::McwApp;
use eframe::egui;
use mcw_storage::LauncherPaths;
use tracing_subscriber::EnvFilter;

fn main() -> eframe::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let paths = LauncherPaths::portable(root);
    let _ = paths.initialize();

    let options = eframe::NativeOptions {
        centered: true,
        viewport: egui::ViewportBuilder::default()
            .with_title("MCW Launcher: Rust Edition")
            .with_inner_size([1080.0, 680.0])
            .with_min_inner_size([840.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "MCW Launcher: Rust Edition",
        options,
        Box::new(move |cc| Ok(Box::new(McwApp::new(cc, paths)))),
    )
}
