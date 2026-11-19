use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressStage {
    FetchingManifest,
    DownloadingMetadata,
    DownloadingClient,
    DownloadingLibraries,
    ExtractingNatives,
    DownloadingAssets,
    SelectingJava,
    Launching,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub stage: ProgressStage,
    pub message: String,
    pub current: u64,
    pub total: u64,
}

pub trait ProgressSink: Send + Sync {
    fn on_progress(&self, event: ProgressEvent);
}

#[derive(Debug, Default)]
pub struct NoopProgressSink;

impl ProgressSink for NoopProgressSink {
    fn on_progress(&self, _event: ProgressEvent) {}
}
