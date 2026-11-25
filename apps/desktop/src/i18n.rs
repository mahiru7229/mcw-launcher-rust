use mcw_storage::LauncherPaths;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const EMBEDDED_EN_US: &str = include_str!("../../../lang/en-US.json");
const EMBEDDED_VI_VN: &str = include_str!("../../../lang/vi-VN.json");

pub const DEFAULT_LOCALE: &str = "en-US";

#[derive(Debug, Clone, Deserialize)]
struct LanguagePackMeta {
    #[serde(default)]
    locale: String,
    #[serde(default)]
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct LanguagePackFile {
    meta: LanguagePackMeta,
    #[serde(default)]
    translations: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct LanguageInfo {
    pub locale: String,
    pub name: String,
    pub translations: HashMap<String, String>,
}

/// Rust port of MCW Launcher Python's `LanguageManager` (`src/core/language/language_manager.py`).
///
/// Loads official `lang/en-US.json` and `lang/vi-VN.json` (embedded in the binary and optionally
/// overridden/extended by any `lang/*.json` files next to the launcher). Builds the same
/// reverse-rendered English-value index (`_rendered_value_keys`) so both official dot-keys
/// (e.g. `"navigation.launcher_settings"`) and exact English strings from `en-US.json`
/// resolve directly to the official translations without inventing new dictionaries.
pub struct LanguageManager {
    current_locale: String,
    languages: HashMap<String, LanguageInfo>,
    default_translations: HashMap<String, String>,
    rendered_value_keys: HashMap<String, String>,
}

impl LanguageManager {
    pub fn load(paths: &LauncherPaths, preferred_locale: &str) -> Self {
        let mut languages = HashMap::new();

        // 1. Load embedded official packs
        for raw in [EMBEDDED_EN_US, EMBEDDED_VI_VN] {
            if let Some(info) = Self::parse_pack(raw, None) {
                languages.insert(info.locale.clone(), info);
            }
        }

        // 2. Load/override with external `lang/*.json` files (matching Python LanguageManager)
        let external_dir = paths.project_root.join("lang");
        if let Ok(entries) = fs::read_dir(&external_dir) {
            let mut files: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && p.extension()
                            .and_then(|e| e.to_str())
                            .map(|e| e.eq_ignore_ascii_case("json"))
                            .unwrap_or(false)
                })
                .collect();
            files.sort();
            for path in files {
                if let Ok(raw) = fs::read_to_string(&path) {
                    let fallback = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_string());
                    if let Some(info) = Self::parse_pack(&raw, fallback.as_deref()) {
                        languages.insert(info.locale.clone(), info);
                    }
                }
            }
        }

        let default_translations = languages
            .get(DEFAULT_LOCALE)
            .map(|l| l.translations.clone())
            .unwrap_or_default();

        // Build reverse English-text -> key index (same strategy as Python `_build_rendered_text_index`)
        let mut rendered_value_keys = HashMap::new();
        for (k, v) in &default_translations {
            let trimmed = v.trim();
            if !trimmed.is_empty() && !trimmed.contains('{') {
                rendered_value_keys
                    .entry(trimmed.to_string())
                    .or_insert_with(|| k.clone());
            }
        }

        let mut mgr = Self {
            current_locale: DEFAULT_LOCALE.to_string(),
            languages,
            default_translations,
            rendered_value_keys,
        };
        mgr.set_language(preferred_locale);
        mgr
    }

    fn parse_pack(raw: &str, fallback_locale: Option<&str>) -> Option<LanguageInfo> {
        let pack: LanguagePackFile = serde_json::from_str(raw).ok()?;
        let locale = if !pack.meta.locale.trim().is_empty() {
            pack.meta.locale.trim().to_string()
        } else {
            fallback_locale?.trim().to_string()
        };
        if locale.is_empty() {
            return None;
        }
        let name = if !pack.meta.name.trim().is_empty() {
            pack.meta.name.trim().to_string()
        } else {
            locale.clone()
        };
        Some(LanguageInfo {
            locale,
            name,
            translations: pack.translations,
        })
    }

    pub fn normalize_locale(locale: &str) -> String {
        match locale.trim().to_ascii_lowercase().as_str() {
            "vi" | "vi-vn" | "vi_vn" => "vi-VN".to_string(),
            "en" | "en-us" | "en_us" | "" => "en-US".to_string(),
            _ => locale.trim().to_string(),
        }
    }

    pub fn set_language(&mut self, locale: &str) {
        let normalized = Self::normalize_locale(locale);
        if self.languages.contains_key(&normalized) {
            self.current_locale = normalized;
        } else {
            self.current_locale = DEFAULT_LOCALE.to_string();
        }
    }

    pub fn current_locale(&self) -> &str {
        &self.current_locale
    }

    /// Re-scans embedded + external `lang/*.json` files while keeping the active locale if available.
    pub fn reload(&mut self, paths: &LauncherPaths) {
        let current = self.current_locale.clone();
        *self = Self::load(paths, &current);
    }

    pub fn available_languages(&self) -> Vec<(&str, &str)> {
        let mut list: Vec<(&str, &str)> = self
            .languages
            .values()
            .map(|l| (l.locale.as_str(), l.name.as_str()))
            .collect();
        list.sort_by_key(|a| a.1.to_lowercase());
        list
    }

    /// Translates either an official MCW key (e.g. `"navigation.launcher_settings"`)
    /// or an exact English string from `en-US.json` into the active locale.
    pub fn tr<'a>(&'a self, key: &'a str) -> &'a str {
        let resolved_key = self
            .rendered_value_keys
            .get(key)
            .map(String::as_str)
            .unwrap_or(key);

        if let Some(pack) = self.languages.get(&self.current_locale) {
            if let Some(val) = pack.translations.get(resolved_key) {
                return val.as_str();
            }
        }
        if let Some(val) = self.default_translations.get(resolved_key) {
            return val.as_str();
        }
        key
    }

    /// Translates a key and substitutes `{name}` placeholders (compatible with Python `.format()`).
    pub fn tr_fmt(&self, key: &str, vars: &[(&str, &str)]) -> String {
        let mut text = self.tr(key).to_string();
        for (k, v) in vars {
            text = text.replace(&format!("{{{k}}}"), v);
        }
        text
    }

    /// Computes translation completeness `(translated_keys, total_default_keys, percent)`
    /// for `locale` relative to `en-US`. Untranslated keys automatically fall back to English.
    pub fn translation_completion(&self, locale: &str) -> (usize, usize, u8) {
        let total = self.default_translations.len();
        if total == 0 {
            return (0, 0, 100);
        }
        let normalized = Self::normalize_locale(locale);
        if normalized.eq_ignore_ascii_case(DEFAULT_LOCALE) {
            return (total, total, 100);
        }
        let translated = self
            .languages
            .get(&normalized)
            .map(|pack| {
                self.default_translations
                    .keys()
                    .filter(|k| {
                        pack.translations
                            .get(*k)
                            .map(|v| !v.trim().is_empty())
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0);
        let percent = ((translated * 100) / total).min(100) as u8;
        (translated, total, percent)
    }
}

pub const LAUNCHER_SETTINGS_SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/mahiru7229/mcw-launcher-rust/main/schemas/launcher-settings.schema.json";

fn default_launcher_settings_schema() -> String {
    LAUNCHER_SETTINGS_SCHEMA_URL.to_string()
}

/// Persisted global launcher preferences (`config/launcher_settings.json`),
/// mirroring MCW Launcher Python's `LauncherSettingsManager` schema (only active Rust Edition options).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherPrefs {
    #[serde(rename = "$schema", default = "default_launcher_settings_schema")]
    pub schema: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_lang")]
    pub language: String,
    #[serde(default)]
    pub show_snapshots: bool,
    #[serde(default)]
    pub debug_mode: bool,
    #[serde(default)]
    pub download_concurrency: u32,
    #[serde(default = "default_true")]
    pub show_content_descriptions: bool,
    #[serde(default)]
    pub modrinth_include_beta: bool,
    #[serde(default)]
    pub modrinth_include_alpha: bool,
    #[serde(default = "default_min_mem")]
    pub default_min_memory: u32,
    #[serde(default = "default_max_mem")]
    pub default_max_memory: u32,
    #[serde(default = "default_width")]
    pub default_width: u32,
    #[serde(default = "default_height")]
    pub default_height: u32,
    #[serde(default)]
    pub default_fullscreen: bool,
    #[serde(default = "default_notification_timeout")]
    pub notification_timeout_secs: u32,
    #[serde(default = "default_true")]
    pub auto_check_updates: bool,
    #[serde(default)]
    pub auto_update_v110_migrated: bool,
    #[serde(default)]
    pub v121_defaults_migrated: bool,
    #[serde(default = "default_update_channel")]
    pub update_channel: String,
    #[serde(default)]
    pub first_run_completed: bool,
    #[serde(default = "default_true")]
    pub discord_rpc_enabled: bool,
    #[serde(default)]
    pub discord_client_id: String,
    #[serde(default = "default_retention_days")]
    pub unused_version_retention_days: u32,
    #[serde(default)]
    pub custom_accent_hex: String,
    #[serde(default = "default_motion_mode")]
    pub motion_mode: String,
    #[serde(default)]
    pub download_limit_mbps: f64,
    #[serde(default)]
    pub prefer_dedicated_gpu: bool,
    #[serde(default)]
    pub minimize_on_launch: bool,
    #[serde(default = "default_true")]
    pub remember_window_size: bool,
    #[serde(default)]
    pub window_size_customized: bool,
    #[serde(default = "default_window_width")]
    pub window_width: f32,
    #[serde(default = "default_window_height")]
    pub window_height: f32,
    #[serde(default)]
    pub default_java_path: String,
    #[serde(default = "default_policy_block")]
    pub modrinth_failure_policy: String,
    #[serde(default = "default_policy_block")]
    pub curseforge_failure_policy: String,
    #[serde(default = "default_policy_warn")]
    pub forge_preflight_failure_policy: String,
    #[serde(default = "default_true")]
    pub background_enabled: bool,
    #[serde(default)]
    pub background_image_path: String,
    #[serde(default = "default_panel_opacity")]
    pub panel_opacity: f32,
    #[serde(default = "default_background_dim")]
    pub background_dim: f32,
    #[serde(default)]
    pub window_size_schema_version: u32,
    #[serde(default = "default_font_family")]
    pub custom_font_family: String,
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_segments_per_file")]
    pub download_segments_per_file: usize,
    #[serde(default = "default_true")]
    pub auto_cleanup_reminders: bool,
    #[serde(default = "default_junk_retention_days")]
    pub junk_retention_days: u32,
    #[serde(default)]
    pub junk_reminder_snooze_until: u64,
    #[serde(default = "default_home_view_mode")]
    pub home_view_mode: String,
    #[serde(default = "default_home_sort_by")]
    pub home_sort_by: String,
    #[serde(default = "default_proxy_mode")]
    pub proxy_mode: String,
    #[serde(default)]
    pub proxy_host: String,
    #[serde(default = "default_proxy_port")]
    pub proxy_port: u16,
    #[serde(default)]
    pub proxy_username: String,
    #[serde(default)]
    pub proxy_password: String,
    #[serde(default = "default_true")]
    pub auto_open_logs_on_crash: bool,
    #[serde(default = "default_true")]
    pub mitigate_minecraft2shell: bool,
    #[serde(default = "default_true")]
    pub warn_auto_accept_server_resource_packs: bool,
}

fn default_proxy_mode() -> String {
    "none".to_string()
}

fn default_proxy_port() -> u16 {
    8080
}

fn default_home_view_mode() -> String {
    "workspace".to_string()
}

fn default_home_sort_by() -> String {
    "last_played".to_string()
}

fn default_segments_per_file() -> usize {
    6
}

fn default_junk_retention_days() -> u32 {
    14
}

fn default_ui_scale() -> f32 {
    1.0
}

fn default_font_size() -> f32 {
    13.0
}

fn default_panel_opacity() -> f32 {
    0.78
}

fn default_background_dim() -> f32 {
    0.35
}

fn default_policy_block() -> String {
    "block".to_string()
}

fn default_policy_warn() -> String {
    "warn".to_string()
}

fn default_theme() -> String {
    "violet".to_string()
}

fn default_lang() -> String {
    DEFAULT_LOCALE.to_string()
}

fn default_true() -> bool {
    true
}

fn default_min_mem() -> u32 {
    1024
}

fn default_max_mem() -> u32 {
    2048
}

fn default_width() -> u32 {
    1280
}

fn default_height() -> u32 {
    720
}

fn default_notification_timeout() -> u32 {
    8
}

fn default_update_channel() -> String {
    "stable".to_string()
}

fn default_font_family() -> String {
    crate::theme::BUNDLED_JETBRAINS_MONO_OPTION.to_string()
}

fn default_retention_days() -> u32 {
    14
}

fn default_motion_mode() -> String {
    "full".to_string()
}

fn default_window_width() -> f32 {
    0.0
}

fn default_window_height() -> f32 {
    0.0
}

impl Default for LauncherPrefs {
    fn default() -> Self {
        Self {
            schema: default_launcher_settings_schema(),
            theme: default_theme(),
            language: default_lang(),
            show_snapshots: false,
            debug_mode: false,
            download_concurrency: 0,
            show_content_descriptions: true,
            modrinth_include_beta: false,
            modrinth_include_alpha: false,
            default_min_memory: default_min_mem(),
            default_max_memory: default_max_mem(),
            default_width: default_width(),
            default_height: default_height(),
            default_fullscreen: false,
            notification_timeout_secs: default_notification_timeout(),
            auto_check_updates: true,
            auto_update_v110_migrated: true,
            v121_defaults_migrated: true,
            update_channel: default_update_channel(),
            first_run_completed: false,
            discord_rpc_enabled: true,
            discord_client_id: String::new(),
            unused_version_retention_days: default_retention_days(),
            custom_accent_hex: String::new(),
            motion_mode: default_motion_mode(),
            download_limit_mbps: 0.0,
            prefer_dedicated_gpu: false,
            minimize_on_launch: false,
            remember_window_size: true,
            window_size_customized: false,
            window_width: default_window_width(),
            window_height: default_window_height(),
            default_java_path: String::new(),
            modrinth_failure_policy: default_policy_block(),
            curseforge_failure_policy: default_policy_block(),
            forge_preflight_failure_policy: default_policy_warn(),
            background_enabled: true,
            background_image_path: String::new(),
            panel_opacity: default_panel_opacity(),
            background_dim: default_background_dim(),
            window_size_schema_version: 2,
            custom_font_family: default_font_family(),
            ui_scale: default_ui_scale(),
            font_size: default_font_size(),
            download_segments_per_file: default_segments_per_file(),
            auto_cleanup_reminders: true,
            junk_retention_days: default_junk_retention_days(),
            junk_reminder_snooze_until: 0,
            home_view_mode: default_home_view_mode(),
            home_sort_by: default_home_sort_by(),
            proxy_mode: default_proxy_mode(),
            proxy_host: String::new(),
            proxy_port: default_proxy_port(),
            proxy_username: String::new(),
            proxy_password: String::new(),
            auto_open_logs_on_crash: true,
            mitigate_minecraft2shell: true,
            warn_auto_accept_server_resource_packs: true,
        }
    }
}

impl LauncherPrefs {
    pub fn load(paths: &LauncherPaths) -> Self {
        let path = paths.config_root.join("launcher_settings.json");
        if let Some((mut loaded, missing_schema)) = fs::read_to_string(&path).ok().and_then(|raw| {
            let mut prefs = serde_json::from_str::<Self>(&raw).ok()?;
            let missing_schema = serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .and_then(|v| {
                    v.get("$schema")
                        .and_then(|s| s.as_str())
                        .map(str::trim)
                        .map(str::to_string)
                })
                .is_none_or(|s| s.is_empty());
            if missing_schema {
                prefs.schema = default_launcher_settings_schema();
            }
            Some((prefs, missing_schema))
        }) {
            let mut needs_save = missing_schema;
            if !loaded.auto_update_v110_migrated {
                loaded.auto_check_updates = true;
                loaded.auto_update_v110_migrated = true;
                needs_save = true;
            }
            if !loaded.v121_defaults_migrated {
                loaded.custom_font_family = default_font_family();
                loaded.update_channel = default_update_channel();
                loaded.v121_defaults_migrated = true;
                needs_save = true;
            }
            if needs_save {
                loaded.save(paths);
            }
            loaded
        } else {
            Self::default()
        }
    }

    pub fn save(&self, paths: &LauncherPaths) {
        let path = paths.config_root.join("launcher_settings.json");
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = mcw_storage::atomic_write_text(&path, &json);
        }
    }

    pub fn normalized_locale(&self) -> String {
        LanguageManager::normalize_locale(&self.language)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_official_mcw_en_and_vi_packs_and_resolves_keys_and_rendered_text() {
        let tmp = tempdir().unwrap();
        let paths = LauncherPaths::portable(tmp.path());

        let mut mgr = LanguageManager::load(&paths, "en-US");
        assert_eq!(mgr.tr("navigation.launcher_settings"), "Launcher Settings");
        assert_eq!(mgr.tr("launch.button"), "Launch");

        mgr.set_language("vi-VN");
        assert_eq!(mgr.tr("navigation.launcher_settings"), "Cài đặt launcher");
        assert_eq!(mgr.tr("Launcher Settings"), "Cài đặt launcher");
        assert_ne!(
            mgr.tr("modpack_package.export.title"),
            "modpack_package.export.title"
        );

        let (en_tr, en_tot, en_pct) = mgr.translation_completion("en-US");
        assert_eq!(en_tr, en_tot);
        assert_eq!(en_pct, 100);
        let (vi_tr, vi_tot, vi_pct) = mgr.translation_completion("vi-VN");
        assert_eq!(vi_tot, en_tot);
        assert!(vi_tr < vi_tot);
        assert!(vi_pct < 100);
    }

    #[test]
    fn defaults_and_migrates_auto_check_updates_for_v110() {
        let tmp = tempdir().unwrap();
        let paths = LauncherPaths::portable(tmp.path());
        paths.initialize().unwrap();

        let fresh = LauncherPrefs::default();
        assert!(fresh.auto_check_updates);
        assert!(fresh.auto_update_v110_migrated);

        // Simulate existing v1.0.0 user config with `auto_check_updates: false` and no migration flag.
        let legacy_path = paths.config_root.join("launcher_settings.json");
        fs::write(&legacy_path, r#"{"auto_check_updates": false}"#).unwrap();

        let migrated = LauncherPrefs::load(&paths);
        assert!(migrated.auto_check_updates);
        assert!(migrated.auto_update_v110_migrated);

        // Subsequent explicit opt-out by user stays respected.
        let mut opted_out = migrated;
        opted_out.auto_check_updates = false;
        opted_out.save(&paths);
        let reloaded = LauncherPrefs::load(&paths);
        assert!(!reloaded.auto_check_updates);
        assert!(reloaded.auto_update_v110_migrated);
    }

    #[test]
    fn defaults_and_migrates_font_and_stable_channel_for_v121() {
        let tmp = tempdir().unwrap();
        let paths = LauncherPaths::portable(tmp.path());
        paths.initialize().unwrap();

        let fresh = LauncherPrefs::default();
        assert_eq!(
            fresh.custom_font_family,
            crate::theme::BUNDLED_JETBRAINS_MONO_OPTION
        );
        assert_eq!(fresh.update_channel, "stable");
        assert!(fresh.v121_defaults_migrated);

        // Simulate existing v1.2.0 config with beta channel, blank font, and no v121 flag.
        let legacy_path = paths.config_root.join("launcher_settings.json");
        fs::write(
            &legacy_path,
            r#"{"auto_update_v110_migrated": true, "update_channel": "beta", "custom_font_family": ""}"#,
        )
        .unwrap();

        let migrated = LauncherPrefs::load(&paths);
        assert_eq!(
            migrated.custom_font_family,
            crate::theme::BUNDLED_JETBRAINS_MONO_OPTION
        );
        assert_eq!(migrated.update_channel, "stable");
        assert!(migrated.v121_defaults_migrated);

        // Subsequent explicit user changes stay respected.
        let mut custom = migrated;
        custom.update_channel = "beta".to_string();
        custom.custom_font_family = "Segoe UI".to_string();
        custom.save(&paths);
        let reloaded = LauncherPrefs::load(&paths);
        assert_eq!(reloaded.update_channel, "beta");
        assert_eq!(reloaded.custom_font_family, "Segoe UI");
        assert!(reloaded.v121_defaults_migrated);
    }

    #[test]
    fn launcher_prefs_backfills_schema_and_defaults_security_flags() {
        let tmp = tempdir().unwrap();
        let paths = LauncherPaths::portable(tmp.path());
        paths.initialize().unwrap();

        let legacy_path = paths.config_root.join("launcher_settings.json");
        fs::write(&legacy_path, r#"{"theme": "emerald"}"#).unwrap();

        let loaded = LauncherPrefs::load(&paths);
        assert_eq!(loaded.schema, LAUNCHER_SETTINGS_SCHEMA_URL);
        assert!(loaded.mitigate_minecraft2shell);
        assert!(loaded.warn_auto_accept_server_resource_packs);

        let persisted = fs::read_to_string(&legacy_path).unwrap();
        assert!(persisted.contains("\"$schema\""));
        assert!(persisted.contains(LAUNCHER_SETTINGS_SCHEMA_URL));
    }
}
