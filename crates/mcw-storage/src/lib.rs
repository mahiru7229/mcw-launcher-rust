pub mod atomic_fs;
pub mod backup;
pub mod cleanup;
pub mod content_store;
pub mod health;
pub mod instance_store;
pub mod migration;
pub mod paths;
pub mod run_lock;

pub use atomic_fs::{atomic_write_bytes, atomic_write_json, atomic_write_text};
pub use backup::{BackupInfo, BackupManager, RestoreOutcome};
pub use cleanup::{
    CachedVersionInfo, StorageCleanupReport, StorageCleanupService, StorageSummary,
    UnusedVersionCandidate,
};
pub use content_store::{
    ContentStore, ContentStoreStats, MaterializationResult, SharedFileMaterializer,
};
pub use health::{
    HealthSeverity, InstanceHealthIssue, InstanceHealthManager, InstanceHealthReport,
    InstanceHealthState, StartupRecoveryReport, StartupRecoveryService,
};
pub use instance_store::{InstanceRepository, InstanceStore};
pub use migration::{
    inspect_python_launcher, migrate_from_python_1_8, migrate_from_python_launcher,
    resolve_python_launcher_root, ConflictStrategy, DiscoveredPythonInstance,
    ImportedGlobalSettings, MigrationReport, PythonLayoutKind, PythonMigrationInspection,
    PythonMigrationOptions, PythonMigrationResult,
};
pub use paths::LauncherPaths;
pub use run_lock::{InstanceRunLock, LockState, RunningInstanceInfo};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("JSON serialization/deserialization error at {path}: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("Invalid instance name '{name}': {reason}")]
    InvalidInstanceName { name: String, reason: String },
    #[error("Instance '{0}' not found")]
    InstanceNotFound(String),
    #[error("Instance '{0}' already exists")]
    InstanceAlreadyExists(String),
    #[error("Instance '{0}' is already running")]
    InstanceAlreadyRunning(String),
    #[error("Backup error: {0}")]
    Backup(String),
    #[error("Invalid relative path: {0}")]
    InvalidRelativePath(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;
