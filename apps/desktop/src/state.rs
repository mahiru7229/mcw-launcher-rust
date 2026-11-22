use eframe::egui::Context;
use mcw_auth::{DeviceCodeResponse, StoredAccount};
use mcw_minecraft::{OptiFineState, RepairReport, SavedServer, ScreenshotInfo, WorldInfo};
use mcw_models::progress::{ProgressEvent, ProgressSink};
use mcw_mods::{ContentKind, ContentPackInfo, InstalledContentItem, ModInfo};
use mcw_net::{ConnectivityStatus, TaskControl};
use mcw_providers::{
    AtLauncherPack, CurseForgeMod, FtbPackSummary, ModUpdateCandidate, ModpackInstallReport,
    ModpackOptionalFile, ModpackUpdateCheck, ModpackUpdateReport, ModrinthSearchHit,
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
    pub started_at: Instant,
    pub is_bytes: bool,
    pub control: Arc<TaskControl>,
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
}

impl BrowserProvider {
    pub fn label(self) -> &'static str {
        match self {
            Self::Modrinth => "Modrinth",
            Self::CurseForge => "CurseForge",
            Self::Ftb => "FTB",
            Self::AtLauncher => "ATLauncher",
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
