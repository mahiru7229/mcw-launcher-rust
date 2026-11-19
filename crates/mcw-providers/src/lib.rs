pub mod cache;
pub mod curseforge;
pub mod modrinth;

pub use cache::ApiCache;
pub use curseforge::{
    CurseForgeClient, CurseForgeFile, CurseForgeMod, CurseForgeSearchParams,
    CurseForgeSearchResult,
};
pub use modrinth::{
    InstalledModFile, ModrinthClient, ModrinthDependency, ModrinthFile, ModrinthSearchHit,
    ModrinthSearchParams, ModrinthSearchResult, ModrinthVersion,
};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("No compatible version found for '{project}' (MC {game_version}, loader {loader})")]
    NoCompatibleVersion {
        project: String,
        game_version: String,
        loader: String,
    },
    #[error("CurseForge API key is not configured (set CURSEFORGE_API_KEY)")]
    MissingApiKey,
    #[error("Third-party distribution is disabled for '{file_name}' (mod {mod_id}, file {file_id}). Download manually at: {website_url}")]
    ManualDownloadRequired {
        mod_id: u64,
        file_id: u64,
        file_name: String,
        website_url: String,
    },
    #[error("Provider HTTP error (HTTP {status}): {message}")]
    ApiError { status: u16, message: String },
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("SQLite cache error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Network error: {0}")]
    Net(#[from] mcw_net::NetError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ProviderError>;
