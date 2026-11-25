pub mod atlauncher;
pub mod cache;
pub mod curseforge;
pub mod ftb;
pub mod mod_updates;
pub mod modpack;
pub mod modpack_update;
pub mod modrinth;
pub mod pack_registry;
pub mod technic;

pub use atlauncher::{AtLauncherClient, AtLauncherPack};
pub use cache::ApiCache;
pub use curseforge::{
    load_curseforge_api_key, load_custom_curseforge_api_key, save_curseforge_api_key_to_dotenv,
    save_custom_curseforge_api_key, CurseForgeClient, CurseForgeFile, CurseForgeMod,
    CurseForgePagination, CurseForgeSearchParams, CurseForgeSearchResult,
};
pub use ftb::{FtbClient, FtbPackSummary, FtbVersionSummary};
pub use mod_updates::{
    LanHostingManager, ModProvenanceFile, ModProvenanceRegistry, ModUpdateCandidate,
    ModUpdateManager, TrackedModEntry,
};
pub use modpack::{
    inspect_archive_name, normalize_modloader, parse_cf_loader_id, suggest_short_instance_name,
    ManualDownloadItem, ModpackExportMode, ModpackExportOptions, ModpackInstallOptions,
    ModpackInstallReport, ModpackInstaller, ModpackOptionalFile, ModpackProgressCallback,
};
pub use modpack_update::{
    ModpackUpdateCandidate, ModpackUpdateCheck, ModpackUpdateReport, ModpackUpdateService,
};
pub use modrinth::{
    InstalledModFile, ModrinthClient, ModrinthDependency, ModrinthFile, ModrinthSearchHit,
    ModrinthSearchParams, ModrinthSearchResult, ModrinthVersion,
};
pub use pack_registry::{
    detect_export_origin, detect_installed_pack, is_verified_or_imported_modpack,
    AtLauncherManagedFile, AtLauncherPackRecord, AtLauncherPackRegistry, CurseForgePackRecord,
    CurseForgePackRegistry, FtbManagedFile, FtbPackRecord, FtbPackRegistry, InstalledPackOrigin,
    InstanceExportOrigin, ModrinthPackRecord, ModrinthPackRegistry, TechnicManagedFile,
    TechnicPackRecord, TechnicPackRegistry,
};
pub use technic::{
    detect_loader_from_modpack_jar, SolderBuildInfo, SolderModEntry, SolderPackInfo, TechnicClient,
    TechnicModpackDetail, TechnicModpackSummary,
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
