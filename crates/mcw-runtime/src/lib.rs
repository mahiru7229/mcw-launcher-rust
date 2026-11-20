pub mod bytecode;
pub mod compactor;
pub mod diagnostics;
pub mod discord_rpc;
pub mod first_run;
pub mod gpu;
pub mod java;
pub mod jvm_presets;
pub mod lan_agent;
pub mod launcher;
pub mod mclogs;
pub mod provisioner;
pub mod supervisor;
pub mod window_activator;

pub use bytecode::{detect_jar_required_java, detect_mods_required_java};
pub use compactor::JavaCommandCompactor;
pub use diagnostics::{
    find_latest_crash_report, DiagnosticsExporter, LaunchErrorPresenter, LaunchErrorView,
};
pub use discord_rpc::{DiscordActivity, DiscordRpcService, DEFAULT_DISCORD_CLIENT_ID};
pub use first_run::{
    detect_available_memory_mb, FirstRunRecommendation, FirstRunRecommendationService,
};
pub use gpu::GpuPreferenceManager;
pub use java::{
    prefer_javaw_on_windows, JavaDiscovery, JavaInstallation, JavaMajorPolicy, JavaResolver,
};
pub use jvm_presets::{
    detect_preset, get_preset_flags, JvmPresetId, AIKAR_FLAGS, SHENANDOAH_FLAGS, ZGC_FLAGS,
};
pub use lan_agent::LanAgentManager;
pub use launcher::{LaunchSpec, ProcessLauncher};
pub use mclogs::{sanitize_log_text, McLogsClient, McLogsUploadResult};
pub use provisioner::{AdoptiumRelease, JavaProvisioner};
pub use supervisor::{
    detect_unsupported_class_version, ExitClassification, LogLine, LogSink, LogStream, NoopLogSink,
    ProcessControl, ProcessExitReport, SupervisedProcess,
};
pub use window_activator::GameWindowActivator;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("Invalid Java major version: {0}")]
    InvalidJavaMajor(String),
    #[error("Java {0} is not supported (supported majors: 8, 17, 21, 25)")]
    UnsupportedJavaMajor(u32),
    #[error("No compatible Java runtime found (required Java {required}+). Install Java {managed} or pass --java <path>.")]
    NoCompatibleJava { required: u32, managed: u32 },
    #[error("Configured Java executable is invalid at {path}: {reason}")]
    InvalidJavaPath { path: String, reason: String },
    #[error("Managed Java provisioning failed: {0}")]
    ProvisionFailed(String),
    #[error("Failed to spawn Minecraft process: {0}")]
    SpawnFailed(#[from] std::io::Error),
    #[error("{0}")]
    Process(String),
    #[error("Network error: {0}")]
    Net(#[from] mcw_net::NetError),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

pub type Result<T> = std::result::Result<T, RuntimeError>;

/// Detect total physical system RAM in megabytes (MB). Falls back to 4096 MB if unavailable.
pub fn detect_total_memory_mb() -> u32 {
    #[cfg(target_os = "windows")]
    {
        #[repr(C)]
        struct MemoryStatusEx {
            dw_length: u32,
            dw_memory_load: u32,
            ull_total_phys: u64,
            ull_avail_phys: u64,
            ull_total_page_file: u64,
            ull_avail_page_file: u64,
            ull_total_virtual: u64,
            ull_avail_virtual: u64,
            ull_avail_extended_virtual: u64,
        }
        extern "system" {
            fn GlobalMemoryStatusEx(lp_buffer: *mut MemoryStatusEx) -> i32;
        }
        let mut status = MemoryStatusEx {
            dw_length: std::mem::size_of::<MemoryStatusEx>() as u32,
            dw_memory_load: 0,
            ull_total_phys: 0,
            ull_avail_phys: 0,
            ull_total_page_file: 0,
            ull_avail_page_file: 0,
            ull_total_virtual: 0,
            ull_avail_virtual: 0,
            ull_avail_extended_virtual: 0,
        };
        // SAFETY: `status` is a valid, properly sized `MEMORYSTATUSEX` struct on the stack.
        let ok = unsafe { GlobalMemoryStatusEx(&mut status as *mut MemoryStatusEx) };
        if ok != 0 && status.ull_total_phys > 0 {
            let mb = (status.ull_total_phys / (1024 * 1024)) as u32;
            return mb.max(512);
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    if let Some(kb_str) = rest.split_whitespace().next() {
                        if let Ok(kb) = kb_str.parse::<u64>() {
                            return ((kb / 1024) as u32).max(512);
                        }
                    }
                }
            }
        }
    }
    4096
}

/// Recommended `(min_mb, max_mb)` RAM for a modpack based on loader and system PC RAM:
/// - Forge / NeoForge: up to `8192 MB` clamped to PC RAM (`min(pc_ram_mb, 8192)`).
/// - Fabric / Quilt / Vanilla / other: up to `4096 MB` clamped to PC RAM (`min(pc_ram_mb, 4096)`).
pub fn recommended_modpack_ram_mb(loader: &str, pc_ram_mb: u32) -> (u32, u32) {
    let l = loader.trim().to_ascii_lowercase();
    let cap = if l == "forge" || l == "neoforge" {
        8192
    } else {
        4096
    };
    let pc = pc_ram_mb.max(512);
    let raw_max = pc.min(cap).max(512);
    // Snap down to nearest 256 MB step
    let max_mb = ((raw_max / 256) * 256).max(512);
    let min_mb = 1024u32.min(max_mb);
    (min_mb, max_mb)
}

#[cfg(test)]
mod mem_tests {
    use super::*;

    #[test]
    fn computes_recommended_modpack_ram_by_loader() {
        assert_eq!(recommended_modpack_ram_mb("forge", 16384), (1024, 8192));
        assert_eq!(recommended_modpack_ram_mb("neoforge", 16384), (1024, 8192));
        assert_eq!(recommended_modpack_ram_mb("forge", 4096), (1024, 4096));
        assert_eq!(recommended_modpack_ram_mb("fabric", 16384), (1024, 4096));
        assert_eq!(recommended_modpack_ram_mb("quilt", 8192), (1024, 4096));
        assert_eq!(recommended_modpack_ram_mb("fabric", 2048), (1024, 2048));
        assert!(detect_total_memory_mb() >= 512);
    }
}
