pub mod arguments;
pub mod assets;
pub mod classpath;
pub mod forge;
pub mod installer;
pub mod lan_targets;
pub mod legacy_libloader;
pub mod libraries;
pub mod manifest;
pub mod modloader;
pub mod optifine;
pub mod preflight;
pub mod repair;
pub mod rules;
pub mod screenshots;
pub mod security;
pub mod security_policy;
pub mod servers;
pub mod worlds;

pub use arguments::ArgumentBuilder;
pub use assets::AssetManager;
pub use classpath::ClasspathBuilder;
pub use forge::{ForgeFamily, ForgeLoaderService, ForgeLoaderVersion};
pub use installer::{PreparedLaunch, VanillaLaunchPipeline};
pub use lan_targets::{LanAgentTarget, LanAgentTargetResolution, LanAgentTargetResolver};
pub use legacy_libloader::{LegacyLibLoaderDependency, LegacyLibLoaderManager};
pub use libraries::LibraryManager;
pub use manifest::VersionManifestService;
pub use modloader::{LoaderKind, LoaderVersionEntry, ModLoaderService};
pub use optifine::{
    merge_optifine_profile, InspectedOptiFine, OptiFineManager, OptiFineRegistry, OptiFineState,
    OptiFineTransaction, OptiFineVersion, OFFICIAL_OPTIFINE_URL,
};
pub use preflight::{
    ForgePreflightIssue, ForgePreflightManager, ForgePreflightReport, LoaderDiagnosticsExporter,
};
pub use repair::{RepairReport, RepairService};
pub use rules::LibraryRules;
pub use screenshots::{ScreenshotInfo, ScreenshotManager};
pub use security::{
    FreetypeLibraryStatus, InstanceSecurityAssessment, LibrarySyncOutcome, MojangLibrarySync,
    SecurityManager, SecurityStatus,
};
pub use security_policy::{
    AgentLifecycleStage, PolicyValidity, ResolvedSecurityPolicy, SecurityPolicy,
    SecurityPolicyService, SignedPolicyEnvelope, REMOTE_SECURITY_POLICY_URL,
};
pub use servers::{SavedServer, ServerManager};
pub use worlds::{
    inspect_zip_content_kind, is_quick_play_supported, WorldDataPackInfo, WorldInfo, WorldManager,
    ZipContentKind,
};

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
