use eframe::egui::Context;
use mcw_auth::{DeviceCodeResponse, StoredAccount};
use mcw_minecraft::RepairReport;
use mcw_models::progress::{ProgressEvent, ProgressSink};
use mcw_providers::{
    AtLauncherPack, CurseForgeMod, FtbPackSummary, ModpackInstallReport, ModrinthSearchHit,
};
use mcw_runtime::{JavaInstallation, LogLine, LogSink, ProcessControl, ProcessExitReport};
use mcw_storage::{BackupInfo, MigrationReport, RestoreOutcome};
use mcw_updater::UpdateCheckResult;
use std::sync::mpsc::Sender;

pub enum AppEvent {
    VersionsLoaded(Result<Vec<String>, String>),
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
    ModpackInstalled(Result<ModpackInstallReport, String>),
    MigrationFinished {
        dry_run: bool,
        result: Result<MigrationReport, String>,
    },
    DeviceCodeReady(DeviceCodeResponse),
    MicrosoftLoginFinished(Result<StoredAccount, String>),
    ModrinthSearchFinished(Result<Vec<ModrinthSearchHit>, String>),
    CurseForgeSearchFinished(Result<Vec<CurseForgeMod>, String>),
    FtbSearchFinished(Result<Vec<FtbPackSummary>, String>),
    AtLauncherSearchFinished(Result<Vec<AtLauncherPack>, String>),
    ModrinthInstallFinished {
        instance_id: String,
        result: Result<usize, String>,
    },
    JavaInstallFinished(Result<JavaInstallation, String>),
    UpdateCheckFinished(Result<UpdateCheckResult, String>),
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
