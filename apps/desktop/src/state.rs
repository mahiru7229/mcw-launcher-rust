use eframe::egui::Context;
use mcw_auth::{DeviceCodeResponse, StoredAccount};
use mcw_minecraft::{OptiFineState, RepairReport, SavedServer, ScreenshotInfo, WorldInfo};
use mcw_models::progress::{ProgressEvent, ProgressSink};
use mcw_mods::{ContentKind, ContentPackInfo, InstalledContentItem, ModInfo};
use mcw_net::{ConnectivityStatus, TaskControl};
use mcw_providers::{
    AtLauncherPack, CurseForgeMod, FtbPackSummary, ModUpdateCandidate, ModpackInstallReport,
    ModpackOptionalFile, ModpackUpdateCheck, ModpackUpdateReport, ModrinthSearchHit,
    TechnicModpackSummary,
};
use mcw_runtime::{
    FirstRunRecommendation, JavaDiagnostic, JavaInstallation, LogLine, LogSink, McLogsUploadResult,
    ProcessControl, ProcessExitReport,
};
use mcw_storage::{
    BackupInfo, CachedVersionInfo, ContentStoreStats, JunkScanReport, PythonMigrationInspection,
    PythonMigrationResult, RestoreOutcome, StorageCleanupReport, StorageSummary,
    UnusedVersionCandidate,
};
use mcw_updater::{StagedUpdate, UpdateCheckResult};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct ActiveTask {
    pub id: u64,
    pub label: String,
    pub message: String,
    pub current: u64,
    pub total: u64,
    pub is_bytes: bool,
    pub eta_base_at: Instant,
    pub eta_base_current: u64,
    pub rate_ema: f64,
    pub last_prog_at: Instant,
    pub last_prog_current: u64,
    pub control: Arc<TaskControl>,
}

impl ActiveTask {
    pub fn new(
        id: u64,
        label: impl Into<String>,
        message: impl Into<String>,
        control: Arc<TaskControl>,
    ) -> Self {
        let now = Instant::now();
        Self {
            id,
            label: label.into(),
            message: message.into(),
            current: 0,
            total: 0,
            is_bytes: false,
            eta_base_at: now,
            eta_base_current: 0,
            rate_ema: 0.0,
            last_prog_at: now,
            last_prog_current: 0,
            control,
        }
    }

    pub fn record_progress(
        &mut self,
        message: String,
        current: u64,
        total: u64,
        next_is_bytes: bool,
    ) {
        let now = Instant::now();
        let phase_changed = self.is_bytes != next_is_bytes
            || (self.total == 0 && total > 0 && current <= total)
            || (!next_is_bytes && total > 0 && self.total > 0 && total != self.total)
            || (total > 0 && self.total > 0 && current < self.current);

        if phase_changed {
            self.is_bytes = next_is_bytes;
            self.eta_base_at = now;
            self.eta_base_current = current;
            self.rate_ema = 0.0;
            self.last_prog_at = now;
            self.last_prog_current = current;
        } else if current > self.last_prog_current {
            let dt = now.duration_since(self.last_prog_at).as_secs_f64();
            let min_dt = if next_is_bytes { 0.15 } else { 0.25 };
            if dt >= min_dt {
                let inst_rate = (current - self.last_prog_current) as f64 / dt;
                self.rate_ema = if self.rate_ema <= 0.0 {
                    inst_rate
                } else {
                    self.rate_ema * 0.72 + inst_rate * 0.28
                };
                self.last_prog_at = now;
                self.last_prog_current = current;
            }
        }

        self.message = message;
        self.current = current;
        self.total = total;
    }

    /// Estimate remaining seconds for the current phase; returns `None` during short warmup.
    pub fn estimated_remaining_secs(&self) -> Option<u64> {
        if self.total == 0 || self.current >= self.total || self.current <= self.eta_base_current {
            return None;
        }
        let elapsed = self.eta_base_at.elapsed().as_secs_f64();
        let min_warmup = if self.is_bytes { 0.8 } else { 1.5 };
        if elapsed < min_warmup {
            return None;
        }
        let delta = self.current.saturating_sub(self.eta_base_current) as f64;
        let avg_rate = if delta > 0.0 { delta / elapsed } else { 0.0 };
        let blended_rate = if self.rate_ema > 0.0 && avg_rate > 0.0 {
            if self.is_bytes {
                self.rate_ema * 0.55 + avg_rate * 0.45
            } else {
                self.rate_ema * 0.7 + avg_rate * 0.3
            }
        } else if self.rate_ema > 0.0 {
            self.rate_ema
        } else {
            avg_rate
        };
        if blended_rate <= 0.000_001 {
            return None;
        }
        let remaining_units = self.total.saturating_sub(self.current) as f64;
        let rem_secs = (remaining_units / blended_rate).ceil();
        if rem_secs.is_finite() && rem_secs >= 0.0 {
            Some((rem_secs as u64).max(1))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModpackChannel {
    Release,
    Beta,
    Alpha,
}

#[derive(Debug, Clone)]
pub struct ModpackVersionOption {
    pub id: String,
    pub label: String,
    #[allow(dead_code)]
    pub mc_version: String,
    pub loader: String,
    pub channel: ModpackChannel,
    pub download_url: Option<String>,
    pub file_name: String,
    pub file_size: u64,
    #[allow(dead_code)]
    pub recommended_ram_mb: u32,
}

#[derive(Debug, Clone)]
pub struct ModVersionChoice {
    pub id: String,
    pub title: String,
    pub version_number: String,
    pub channel: ModpackChannel,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub file_name: String,
    pub download_url: Option<String>,
    pub file_size: u64,
    pub sha1: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrowserCategory {
    Mods,
    Modpacks,
    ResourcePacks,
    ShaderPacks,
    DataPacks,
}

impl BrowserCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mods => "Mods",
            Self::Modpacks => "Modpacks",
            Self::ResourcePacks => "Resource packs",
            Self::ShaderPacks => "Shader packs",
            Self::DataPacks => "Data packs",
        }
    }

    pub fn modrinth_project_type(self) -> &'static str {
        match self {
            Self::Mods => "mod",
            Self::Modpacks => "modpack",
            Self::ResourcePacks => "resourcepack",
            Self::ShaderPacks => "shader",
            Self::DataPacks => "datapack",
        }
    }

    pub fn curseforge_kind(self) -> &'static str {
        match self {
            Self::Mods => "mod",
            Self::Modpacks => "modpack",
            Self::ResourcePacks => "resourcepack",
            Self::ShaderPacks => "shader",
            Self::DataPacks => "datapack",
        }
    }

    pub fn content_kind(self) -> Option<ContentKind> {
        match self {
            Self::ResourcePacks => Some(ContentKind::ResourcePack),
            Self::ShaderPacks => Some(ContentKind::ShaderPack),
            Self::DataPacks => Some(ContentKind::DataPack),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrowserProvider {
    Modrinth,
    CurseForge,
    Ftb,
    AtLauncher,
    Technic,
}

impl BrowserProvider {
    pub fn label(self) -> &'static str {
        match self {
            Self::Modrinth => "Modrinth",
            Self::CurseForge => "CurseForge",
            Self::Ftb => "FTB",
            Self::AtLauncher => "ATLauncher",
            Self::Technic => "Technic",
        }
    }
}

pub enum AppEvent {
    VersionsLoaded(Result<Vec<(String, String)>, String>),
    ThumbnailLoaded {
        url: String,
        size: [usize; 2],
        rgba: Vec<u8>,
    },
    ModVersionsLoaded {
        target_key: String,
        versions: Result<Vec<ModVersionChoice>, String>,
    },
    ModpackInstallVersionsLoaded {
        request_id: u64,
        versions: Result<Vec<ModpackVersionOption>, String>,
    },
    DirectModFileInstalled {
        task_id: u64,
        instance_id: String,
        result: Result<String, String>,
    },
    ModUpdatesChecked {
        instance_id: String,
        candidates: Vec<ModUpdateCandidate>,
    },
    ModUpdatesApplied {
        task_id: u64,
        instance_id: String,
        result: Result<usize, String>,
    },
    ModsLoaded {
        instance_id: String,
        mods: Vec<ModInfo>,
    },
    BackupsLoaded {
        instance_id: String,
        backups: Vec<BackupInfo>,
    },
    WorldsLoaded {
        instance_id: String,
        worlds: Vec<WorldInfo>,
    },
    ScreenshotsLoaded {
        instance_id: String,
        screenshots: Vec<ScreenshotInfo>,
    },
    ContentPacksLoaded {
        instance_id: String,
        kind: ContentKind,
        packs: Vec<ContentPackInfo>,
    },
    ContentPackInstallFinished {
        task_id: u64,
        item_key: String,
        instance_id: String,
        kind: ContentKind,
        result: Result<String, String>,
    },
    McLogsUploaded(Result<McLogsUploadResult, String>),
    TaskProgress {
        task_id: u64,
        message: String,
        current: u64,
        total: u64,
    },
    LaunchProgress {
        instance_id: String,
        event: ProgressEvent,
    },
    LaunchStarted {
        instance_id: String,
        pid: u32,
        control: ProcessControl,
    },
    LogReceived {
        instance_id: String,
        line: LogLine,
    },
    LaunchFinished {
        instance_id: String,
        report: Result<ProcessExitReport, String>,
    },
    RepairFinished {
        instance_id: String,
        report: Result<RepairReport, String>,
    },
    BackupCreated {
        instance_id: String,
        result: Result<BackupInfo, String>,
    },
    BackupRestored {
        instance_id: String,
        result: Result<RestoreOutcome, String>,
    },
    ModpackInstalled {
        task_id: u64,
        cf_pack_id: Option<u64>,
        result: Result<ModpackInstallReport, String>,
    },
    ExternalModpackInstalled {
        task_id: u64,
        pack_key: String,
        result: Result<ModpackInstallReport, String>,
    },
    MrpackExportFinished {
        task_id: u64,
        result: Result<(std::path::PathBuf, usize), String>,
    },
    DeviceCodeReady(DeviceCodeResponse),
    MicrosoftLoginFinished(Result<StoredAccount, String>),
    JavaInstallFinished {
        task_id: u64,
        major: u32,
        result: Result<JavaInstallation, String>,
    },
    UpdateCheckFinished {
        manual: bool,
        result: Result<UpdateCheckResult, String>,
    },
    UpdateInstallPrepared {
        task_id: u64,
        result: Result<StagedUpdate, String>,
    },
    AccountAvatarLoaded {
        account_uuid: String,
        size: [usize; 2],
        rgba: Vec<u8>,
    },
    FirstRunRecommendationReady(FirstRunRecommendation),
    StorageUsageScanned {
        summary: StorageSummary,
        unused_versions: Vec<UnusedVersionCandidate>,
    },
    StorageCleanupFinished {
        kind: String,
        report: StorageCleanupReport,
    },
    JavaDiagnosticsScanned(Vec<JavaDiagnostic>),
    OptiFineInstallFinished {
        task_id: u64,
        instance_id: String,
        result: Result<OptiFineState, String>,
    },
    ContentStoreDedupFinished {
        task_id: u64,
        result: Result<ContentStoreStats, String>,
    },
    ModpackOptionalFilesLoaded {
        request_id: u64,
        files: Result<Vec<ModpackOptionalFile>, String>,
    },
    ServersLoaded {
        instance_id: String,
        servers: Vec<SavedServer>,
    },
    CachedVersionsLoaded(Vec<CachedVersionInfo>),
    LatestJavaReleaseLoaded(Result<u32, String>),
    BackgroundPicturePicked(Option<std::path::PathBuf>),
    BackgroundImageLoaded {
        key: String,
        result: Result<eframe::egui::ColorImage, String>,
    },
    ContentBrowserSearchFinished {
        request_id: u64,
        category: BrowserCategory,
        provider: BrowserProvider,
        total_hits: u32,
        modrinth: Result<Vec<ModrinthSearchHit>, String>,
        curseforge: Result<Vec<CurseForgeMod>, String>,
        ftb: Result<Vec<FtbPackSummary>, String>,
        atl: Result<Vec<AtLauncherPack>, String>,
        technic: Result<Vec<TechnicModpackSummary>, String>,
    },
    ConnectivityChecked(ConnectivityStatus),
    ContentLibraryLoaded(Vec<(String, String, InstalledContentItem)>),
    ModpackUpdateChecked {
        instance_id: String,
        result: Result<ModpackUpdateCheck, String>,
    },
    ModpackUpdateApplied {
        task_id: u64,
        instance_id: String,
        result: Result<ModpackUpdateReport, String>,
    },
    PreflightConfirmationRequired {
        instance_id: String,
        quick_play_world: Option<String>,
        quick_play_server: Option<String>,
        issues: Vec<String>,
    },
    PythonFolderPicked(Option<std::path::PathBuf>),
    PythonInspectionFinished {
        source_root: String,
        result: Result<PythonMigrationInspection, String>,
    },
    PythonMigrationFinished {
        task_id: u64,
        result: Result<PythonMigrationResult, String>,
    },
    JunkScanCompleted {
        report: JunkScanReport,
        manual: bool,
    },
    JunkCleanCompleted(Result<(usize, u64), String>),
    WatchedFolderChanged(String),
    ChangelogRefreshed(Result<String, String>),
    StorageRelocationFinished(Result<(bool, PathBuf), String>),
}

pub struct ChannelProgressSink {
    pub instance_id: String,
    pub tx: Sender<AppEvent>,
    pub ctx: Context,
}

impl ProgressSink for ChannelProgressSink {
    fn on_progress(&self, event: ProgressEvent) {
        let _ = self.tx.send(AppEvent::LaunchProgress {
            instance_id: self.instance_id.clone(),
            event,
        });
        self.ctx.request_repaint();
    }
}

pub struct ChannelLogSink {
    pub instance_id: String,
    pub tx: Sender<AppEvent>,
    pub ctx: Context,
}

impl LogSink for ChannelLogSink {
    fn on_log(&self, line: LogLine) {
        let _ = self.tx.send(AppEvent::LogReceived {
            instance_id: self.instance_id.clone(),
            line,
        });
        self.ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn active_task_resets_phase_and_eta_from_bytes_to_batch_items() {
        let mut task = ActiveTask::new(1, "CurseForge Modpack", "Downloading", TaskControl::new());

        // Phase 1: archive byte download sets `is_bytes = true`
        task.record_progress(
            "Downloading zip (2.0 / 10.0 MB)".to_string(),
            2_000_000,
            10_000_000,
            true,
        );
        assert!(task.is_bytes);

        // Phase 2: mod batch items 35/567 clears `is_bytes` and avoids stale byte ETA (1s)
        task.record_progress("Downloading mods (35/567)".to_string(), 0, 567, false);
        assert!(!task.is_bytes);
        task.eta_base_at = Instant::now() - Duration::from_secs(7);
        task.last_prog_at = Instant::now() - Duration::from_secs(7);
        task.record_progress("Downloading mods (35/567)".to_string(), 35, 567, false);

        let eta = task
            .estimated_remaining_secs()
            .expect("batch ETA should be present");
        // At 5 items/sec (35 items / 7s), 532 remaining items should take ~106s, never 1s!
        assert!(eta >= 50, "expected realistic batch ETA, got {eta}s");
    }
}
