use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModLoader {
    pub name: String,
    pub version: String,
}

impl ModLoader {
    pub fn vanilla() -> Self {
        Self {
            name: "vanilla".to_string(),
            version: "-1".to_string(),
        }
    }

    pub fn is_vanilla(&self) -> bool {
        self.name.eq_ignore_ascii_case("vanilla")
    }
}

impl Default for ModLoader {
    fn default() -> Self {
        Self::vanilla()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instance {
    pub instance_id: String,
    pub name: String,
    pub version_id: String,
    #[serde(default)]
    pub mod_loader: ModLoader,
    #[serde(default = "default_icon")]
    pub icon: String,
    #[serde(default)]
    pub icon_origin: String,
    #[serde(default)]
    pub last_played: String,
    #[serde(default)]
    pub last_exit_code: Option<i32>,
    #[serde(default)]
    pub last_launch_crashed: bool,
    #[serde(default = "default_launch_state")]
    pub last_launch_state: String,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub total_playtime_seconds: u64,
}

fn default_icon() -> String {
    "grass_block".to_string()
}

fn default_launch_state() -> String {
    "ready".to_string()
}

impl Instance {
    pub fn new(
        instance_id: impl Into<String>,
        name: impl Into<String>,
        version_id: impl Into<String>,
    ) -> Self {
        Self {
            instance_id: instance_id.into(),
            name: name.into(),
            version_id: version_id.into(),
            mod_loader: ModLoader::vanilla(),
            icon: default_icon(),
            icon_origin: String::new(),
            last_played: String::new(),
            last_exit_code: None,
            last_launch_crashed: false,
            last_launch_state: default_launch_state(),
            favorite: false,
            group: String::new(),
            tags: Vec::new(),
            total_playtime_seconds: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceSettings {
    #[serde(default)]
    pub java_path: Option<PathBuf>,
    #[serde(default = "default_min_memory")]
    pub min_memory: u32,
    #[serde(default = "default_max_memory")]
    pub max_memory: u32,
    #[serde(default)]
    pub jvm_arguments: Vec<String>,
    #[serde(default)]
    pub game_arguments: Vec<String>,
    #[serde(default)]
    pub offline_multiplayer_enabled: bool,
    #[serde(default = "default_lan_auth_mode")]
    pub lan_auth_mode: String,
    #[serde(default = "default_lan_provider")]
    pub lan_connection_provider: String,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(default = "default_inherit")]
    pub modrinth_failure_policy: String,
    #[serde(default = "default_inherit")]
    pub curseforge_failure_policy: String,
    #[serde(default)]
    pub forge_preflight_always_allow: bool,
    #[serde(default)]
    pub disable_launch_checking: Option<bool>,
    #[serde(default)]
    pub disable_mod_repair_and_updates: Option<bool>,
    #[serde(default)]
    pub pre_launch_command: String,
    #[serde(default)]
    pub post_exit_command: String,
    #[serde(default)]
    pub wrapper_command: String,
    #[serde(default)]
    pub env_vars: Vec<(String, String)>,
    #[serde(default)]
    pub enable_gamemode: bool,
    #[serde(default)]
    pub enable_mangohud: bool,
}

impl InstanceSettings {
    pub fn is_mod_repair_and_update_disabled(&self, is_modpack_origin: bool) -> bool {
        self.disable_mod_repair_and_updates
            .unwrap_or(is_modpack_origin)
    }

    pub fn is_launch_checking_disabled(&self, is_modpack_origin: bool) -> bool {
        self.disable_launch_checking.unwrap_or(is_modpack_origin)
    }
}

fn default_inherit() -> String {
    "inherit".to_string()
}

fn default_min_memory() -> u32 {
    1024
}

fn default_max_memory() -> u32 {
    2048
}

fn default_lan_auth_mode() -> String {
    "microsoft_only".to_string()
}

fn default_lan_provider() -> String {
    "manual".to_string()
}

fn default_width() -> u32 {
    854
}

fn default_height() -> u32 {
    480
}

impl Default for InstanceSettings {
    fn default() -> Self {
        Self {
            java_path: None,
            min_memory: default_min_memory(),
            max_memory: default_max_memory(),
            jvm_arguments: Vec::new(),
            game_arguments: Vec::new(),
            offline_multiplayer_enabled: false,
            lan_auth_mode: default_lan_auth_mode(),
            lan_connection_provider: default_lan_provider(),
            width: default_width(),
            height: default_height(),
            fullscreen: false,
            modrinth_failure_policy: default_inherit(),
            curseforge_failure_policy: default_inherit(),
            forge_preflight_always_allow: false,
            disable_launch_checking: None,
            disable_mod_repair_and_updates: None,
            pre_launch_command: String::new(),
            post_exit_command: String::new(),
            wrapper_command: String::new(),
            env_vars: Vec::new(),
            enable_gamemode: false,
            enable_mangohud: false,
        }
    }
}
