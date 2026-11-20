pub mod account;
pub mod instance;
pub mod minecraft;
pub mod progress;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const LAUNCHER_NAME: &str = "MCW Launcher";
pub const LAUNCHER_SLUG: &str = "mcw-launcher";
pub const DEVELOPER_NAME: &str = "mahiru7229";
pub const GITHUB_REPOSITORY: &str = "mahiru7229/mcw-launcher-rust";
pub const MICROSOFT_CLIENT_ID: &str = "cd379605-ee06-466a-a588-7a1f7c23b48a";

pub const MOJANG_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
pub const MOJANG_RESOURCES_URL: &str = "https://resources.download.minecraft.net";

pub fn user_agent() -> String {
    format!(
        "{}/{}/{} (https://github.com/{})",
        DEVELOPER_NAME, LAUNCHER_SLUG, APP_VERSION, GITHUB_REPOSITORY
    )
}
