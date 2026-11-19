pub mod atomic_fs;
pub mod instance_store;
pub mod paths;

pub use atomic_fs::{atomic_write_bytes, atomic_write_json, atomic_write_text};
pub use instance_store::InstanceStore;
pub use paths::LauncherPaths;

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
    #[error("Invalid relative path: {0}")]
    InvalidRelativePath(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;
