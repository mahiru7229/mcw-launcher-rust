pub mod compat;
pub mod content;
pub mod manager;
pub mod scanner;

pub use compat::{ModCompatibilityAnalyzer, ModCompatibilityReport, ModIssue, ModIssueSeverity};
pub use content::{ContentKind, ContentPackInfo, ContentPackManager};
pub use manager::ModManager;
pub use scanner::{is_loader_compatible, ModInfo, ModLoaderTag, ModScanner};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ModError {
    #[error("Mod file '{0}' not found")]
    NotFound(String),
    #[error("Invalid mod filename '{0}'")]
    InvalidFilename(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ModError>;
