use eframe::egui::Context;
use mcw_auth::{DeviceCodeResponse, StoredAccount};
use mcw_minecraft::{RepairReport, ScreenshotInfo, WorldInfo};
use mcw_models::progress::{ProgressEvent, ProgressSink};
use mcw_mods::{ContentKind, ContentPackInfo, ModInfo};
use mcw_net::TaskControl;
use mcw_providers::{
    AtLauncherPack, CurseForgeMod, FtbPackSummary, ModUpdateCandidate, ModpackInstallReport,
    ModrinthSearchHit,
};
use mcw_runtime::{
    JavaInstallation, LogLine, LogSink, McLogsUploadResult, ProcessControl, ProcessExitReport,
};
use mcw_storage::{BackupInfo, RestoreOutcome};
use std::sync::mpsc::Sender;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ActiveTask {
    pub id: u64,
    pub label: String,
    pub message: String,
    pub current: u64,
    pub total: u64,
    pub control: Arc<TaskControl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModpackChannel {
    Release,
    Beta,
    Alpha,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ModpackVersionOption {
    pub id: String,
    pub label: String,
    pub mc_version: String,
    pub loader: String,
    pub channel: ModpackChannel,
    pub download_url: Option<String>,
    pub file_name: String,
    pub file_size: u64,
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

pub enum AppEvent {
    VersionsLoaded(Result<Vec<(String, String)>, String>),
    ModrinthModpacksSearchFinished(Result<Vec<ModrinthSearchHit>, String>),
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
    LanHostingPrepared {
        task_id: u64,
        instance_id: String,
        result: Result<String, String>,
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
    ContentPackSearchFinished {
        provider_is_cf: bool,
        modrinth: Result<Vec<ModrinthSearchHit>, String>,
        curseforge: Result<Vec<CurseForgeMod>, String>,
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
    ModrinthSearchFinished(Result<Vec<ModrinthSearchHit>, String>),
    CurseForgeSearchFinished(Result<Vec<CurseForgeMod>, String>),
    CurseForgeModpacksSearchFinished(Result<Vec<CurseForgeMod>, String>),
    #[allow(dead_code)]
    CurseForgeModInstallFinished {
        task_id: u64,
        mod_id: u64,
        instance_id: String,
        result: Result<String, String>,
    },
    FtbSearchFinished(Result<Vec<FtbPackSummary>, String>),
    AtLauncherSearchFinished(Result<Vec<AtLauncherPack>, String>),
    #[allow(dead_code)]
    ModrinthInstallFinished {
        task_id: u64,
        slug: String,
        instance_id: String,
        result: Result<usize, String>,
    },
    JavaInstallFinished {
        task_id: u64,
        major: u32,
        result: Result<JavaInstallation, String>,
    },
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
