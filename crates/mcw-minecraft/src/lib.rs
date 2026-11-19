pub mod arguments;
pub mod assets;
pub mod classpath;
pub mod installer;
pub mod libraries;
pub mod manifest;
pub mod modloader;
pub mod repair;
pub mod rules;

pub use arguments::ArgumentBuilder;
pub use assets::AssetManager;
pub use classpath::ClasspathBuilder;
pub use installer::{PreparedLaunch, VanillaLaunchPipeline};
pub use libraries::LibraryManager;
pub use manifest::VersionManifestService;
pub use modloader::{LoaderKind, LoaderVersionEntry, ModLoaderService};
pub use repair::{RepairReport, RepairService};
pub use rules::LibraryRules;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MinecraftError {
    #[error("Version '{0}' was not found in Mojang version manifest")]
    VersionNotFound(String),
    #[error("Invalid Minecraft metadata: {0}")]
    InvalidMetadata(String),
    #[error("Unsafe archive entry in native library: {0}")]
    UnsafeArchivePath(String),
    #[error("Network error: {0}")]
    Net(#[from] mcw_net::NetError),
    #[error("Storage error: {0}")]
    Storage(#[from] mcw_storage::StorageError),
    #[error("Runtime error: {0}")]
    Runtime(#[from] mcw_runtime::RuntimeError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Zip archive error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

pub type Result<T> = std::result::Result<T, MinecraftError>;
