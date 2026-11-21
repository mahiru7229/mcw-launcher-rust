use crate::cache::ApiCache;
use crate::modpack::{
    normalize_modloader, ModpackInstallOptions, ModpackInstallReport, ModpackOptionalFile,
    ModpackProgressCallback,
};
use crate::pack_registry::{AtLauncherManagedFile, AtLauncherPackRecord, AtLauncherPackRegistry};
use crate::{ProviderError, Result};
use mcw_models::instance::ModLoader;
use mcw_net::{DownloadClient, DownloadRequest};
use mcw_storage::{InstanceStore, LauncherPaths};
use serde::{Deserialize, Deserializer, Serialize};
use std::fs;
use std::sync::Arc;
use std::time::Duration;

const ATLAUNCHER_PACKS_URL: &str =
    "https://download.nodecdn.net/containers/atl/launcher/json/packsnew.json";
const ATLAUNCHER_CDN_BASE: &str = "https://download.nodecdn.net/containers/atl/";
const ATL_TTL_SECS: u64 = 600;

fn null_to_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    T: Default + Deserialize<'de>,
    D: Deserializer<'de>,
{
    let opt = Option::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtLauncherVersionEntry {
    #[serde(default, deserialize_with = "null_to_default")]
    pub version: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub minecraft: String,
    #[serde(
        rename = "isRecommended",
        default,
        deserialize_with = "null_to_default"
    )]
    pub is_recommended: bool,
    #[serde(rename = "isDev", default, deserialize_with = "null_to_default")]
    pub is_dev: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtLauncherPack {
    #[serde(default, deserialize_with = "null_to_default")]
    pub id: u64,
    #[serde(default, deserialize_with = "null_to_default")]
    pub name: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub description: String,
    #[serde(rename = "safeName", default, deserialize_with = "null_to_default")]
    pub safe_name: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub versions: Vec<AtLauncherVersionEntry>,
}

impl AtLauncherPack {
    pub fn effective_safe_name(&self) -> String {
        if self.safe_name.is_empty() {
            self.name.replace(' ', "")
        } else {
            self.safe_name.clone()
        }
    }

    pub fn icon_url(&self) -> String {
        format!(
            "{ATLAUNCHER_CDN_BASE}launcher/images/{}.png",
            self.effective_safe_name().to_ascii_lowercase()
        )
    }

    pub fn website_url(&self) -> String {
        format!("https://atlauncher.com/pack/{}", self.effective_safe_name())
    }
}

pub struct AtLauncherClient {
    http: reqwest::Client,
    cache: Option<Arc<ApiCache>>,
}

impl AtLauncherClient {
    pub fn new(cache: Option<Arc<ApiCache>>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(mcw_models::user_agent())
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self { http, cache })
    }

    pub async fn list_packs(&self) -> Result<Vec<AtLauncherPack>> {
        let key = "atlauncher:packsnew";
        if let Some(ref cache) = self.cache {
            if let Some(hit) = cache.get(key) {
                return Ok(serde_json::from_str(&hit)?);
            }
        }
        let resp = self.http.get(ATLAUNCHER_PACKS_URL).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(ProviderError::ApiError {
                status: status.as_u16(),
                message: text,
            });
        }
        if let Some(ref cache) = self.cache {
            let _ = cache.set(key, &text, ATL_TTL_SECS);
        }
        Ok(serde_json::from_str(&text)?)
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<AtLauncherPack>> {
        let (page, _) = self.search_paged(query, 0, limit).await?;
        Ok(page)
    }

    pub async fn search_paged(
        &self,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<AtLauncherPack>, u32)> {
        let q = query.trim().to_ascii_lowercase();
        let packs = self.list_packs().await?;
        let matched: Vec<AtLauncherPack> = packs
            .into_iter()
            .filter(|p| {
                q.is_empty()
                    || p.name.to_ascii_lowercase().contains(&q)
                    || p.description.to_ascii_lowercase().contains(&q)
            })
            .collect();
        let total = matched.len() as u32;
        let page = matched.into_iter().skip(offset).take(limit).collect();
        Ok((page, total))
    }

    /// Inspect optional client mods defined in ATLauncher `Configs.json` for a pack version.
    pub async fn inspect_optional_mods(
        &self,
        pack: &AtLauncherPack,
        version_override: Option<&str>,
    ) -> Result<Vec<ModpackOptionalFile>> {
        let chosen = if let Some(v) = version_override {
            pack.versions
                .iter()
                .find(|entry| entry.version == v)
                .or_else(|| pack.versions.first())
        } else {
            pack.versions.first()
        }
        .ok_or_else(|| {
            ProviderError::Io(std::io::Error::other(format!(
                "ATLauncher pack '{}' has no published versions",
                pack.name
            )))
        })?;
        let safe_name = pack.effective_safe_name();
        let cfg_url = format!(
            "{ATLAUNCHER_CDN_BASE}packs/{safe_name}/versions/{}/Configs.json",
            chosen.version
        );
        let resp = self.http.get(&cfg_url).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(ProviderError::ApiError {
                status: status.as_u16(),
                message: format!("Failed to fetch ATLauncher Configs.json: {text}"),
            });
        }
        let cfg: serde_json::Value = serde_json::from_str(&text)?;
        let mut optionals = Vec::new();
        if let Some(mods) = cfg.get("mods").and_then(|v| v.as_array()) {
            for m in mods {
                if !m.get("client").and_then(|v| v.as_bool()).unwrap_or(true) {
                    continue;
                }
                let mod_type = m.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let mod_name = m.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if is_atlauncher_loader_entry(mod_type, mod_name) {
                    continue;
                }
                if !m.get("optional").and_then(|v| v.as_bool()).unwrap_or(false) {
                    continue;
                }
                let file = m.get("file").and_then(|v| v.as_str()).unwrap_or("");
                if file.is_empty() {
                    continue;
                }
                let display = if mod_name.trim().is_empty() {
                    file.to_string()
                } else {
                    format!("{} ({file})", mod_name.trim())
                };
                let default_selected = m.get("selected").and_then(|v| v.as_bool()).unwrap_or(false);
                optionals.push(ModpackOptionalFile {
                    id: file.to_string(),
                    name: display,
                    path: format!("mods/{file}"),
                    default_selected,
                });
            }
        }
        Ok(optionals)
    }

    /// Download and install the latest version of an ATLauncher modpack into a new Instance.
    pub async fn install_pack_with_progress(
        &self,
        pack: &AtLauncherPack,
        paths: &LauncherPaths,
        downloader: &DownloadClient,
        progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackInstallReport> {
        self.install_pack_version_with_options(
            pack,
            None,
            None,
            paths,
            downloader,
            &ModpackInstallOptions::default(),
            progress,
        )
        .await
    }

    /// Download and install a specific version (or latest if `None`) of an ATLauncher modpack.
    pub async fn install_pack_version_with_progress(
        &self,
        pack: &AtLauncherPack,
        version_override: Option<&str>,
        name_override: Option<&str>,
        paths: &LauncherPaths,
        downloader: &DownloadClient,
        progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackInstallReport> {
        self.install_pack_version_with_options(
            pack,
            version_override,
            name_override,
            paths,
            downloader,
            &ModpackInstallOptions::default(),
            progress,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn install_pack_version_with_options(
        &self,
        pack: &AtLauncherPack,
        version_override: Option<&str>,
        name_override: Option<&str>,
        paths: &LauncherPaths,
        downloader: &DownloadClient,
        options: &ModpackInstallOptions,
        progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackInstallReport> {
        let emit = |cur: u64, tot: u64, msg: String| {
            if let Some(ref cb) = progress {
                cb(cur, tot, msg);
            }
        };

        let chosen = if let Some(v) = version_override {
            pack.versions
                .iter()
                .find(|entry| entry.version == v)
                .or_else(|| pack.versions.first())
        } else {
            pack.versions.first()
        }
        .ok_or_else(|| {
            ProviderError::Io(std::io::Error::other(format!(
                "ATLauncher pack '{}' has no published versions",
                pack.name
            )))
        })?;
        let safe_name = pack.effective_safe_name();

        emit(
            0,
            0,
            format!(
                "Fetching ATLauncher manifest ({} v{})...",
                pack.name, chosen.version
            ),
        );
        let cfg_url = format!(
            "{ATLAUNCHER_CDN_BASE}packs/{safe_name}/versions/{}/Configs.json",
            chosen.version
        );
        let resp = self.http.get(&cfg_url).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(ProviderError::ApiError {
                status: status.as_u16(),
                message: format!("Failed to fetch ATLauncher Configs.json: {text}"),
            });
        }
        let cfg: serde_json::Value = serde_json::from_str(&text)?;

        let game_version = cfg
            .get("minecraft")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(if chosen.minecraft.is_empty() {
                "1.20.1"
            } else {
                &chosen.minecraft
            })
            .to_string();

        let loader = parse_atlauncher_loader(&cfg, &game_version);

        let store = InstanceStore::new(paths);
        let base_name = name_override
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| pack.name.trim());
        let mut candidate_name = if base_name.is_empty() {
            safe_name.clone()
        } else {
            base_name.to_string()
        };
        let mut idx = 2usize;
        while store.load_instance(&candidate_name).is_ok() {
            candidate_name = format!("{base_name} ({idx})");
            idx += 1;
        }
        let (inst, _) = store
            .create_instance(&candidate_name, &game_version, loader.clone())
            .map_err(|e| ProviderError::Io(std::io::Error::other(e.to_string())))?;
        let mc_dir = paths.minecraft_dir(&inst.instance_id);
        let mods_dir = mc_dir.join("mods");
        fs::create_dir_all(&mods_dir)?;

        let mut requests = Vec::new();
        let mut managed_files = Vec::new();
        if let Some(mods) = cfg.get("mods").and_then(|v| v.as_array()) {
            for m in mods {
                if !m.get("client").and_then(|v| v.as_bool()).unwrap_or(true) {
                    continue;
                }
                // Skip loader JAR entries (matching Python ATLauncherClient._is_loader_file)
                let mod_type = m.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let mod_name = m.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if is_atlauncher_loader_entry(mod_type, mod_name) {
                    continue;
                }
                let is_optional = m.get("optional").and_then(|v| v.as_bool()).unwrap_or(false);
                let file = m.get("file").and_then(|v| v.as_str()).unwrap_or("");
                if is_optional {
                    if !options.include_optional {
                        continue;
                    }
                    if let Some(ref set) = options.selected_optional_paths {
                        if !set.contains(file) && !set.contains(&format!("mods/{file}")) {
                            continue;
                        }
                    } else if !m.get("selected").and_then(|v| v.as_bool()).unwrap_or(false) {
                        continue;
                    }
                }
                let raw_url = m.get("url").and_then(|v| v.as_str()).unwrap_or("");
                let download_kind = m.get("download").and_then(|v| v.as_str()).unwrap_or("");
                if file.is_empty() || raw_url.is_empty() || download_kind == "browser" {
                    continue;
                }
                let full_url = if raw_url.starts_with("http://") || raw_url.starts_with("https://")
                {
                    raw_url.to_string()
                } else {
                    format!("{ATLAUNCHER_CDN_BASE}{}", raw_url.trim_start_matches('/'))
                };
                let dest = mods_dir.join(file);
                let sha1 = m
                    .get("sha1")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(String::from);
                let size = m.get("filesize").and_then(|v| v.as_u64()).unwrap_or(0);
                managed_files.push(AtLauncherManagedFile {
                    file_id: file.to_string(),
                    file_name: file.to_string(),
                    name: if mod_name.is_empty() {
                        file.to_string()
                    } else {
                        mod_name.to_string()
                    },
                    path: format!("mods/{file}"),
                    sha1: sha1.clone().unwrap_or_default(),
                    size,
                    urls: vec![full_url.clone()],
                    optional: is_optional,
                    client_only: true,
                    provider: "atlauncher".to_string(),
                });
                requests.push(
                    DownloadRequest::new(full_url, dest)
                        .with_sha1(sha1)
                        .with_size(size),
                );
            }
        }

        let total = requests.len() as u64;
        emit(0, total, format!("Downloading {total} ATLauncher mods..."));
        let prog_clone = progress.clone();
        downloader
            .download_batch(requests, 16, move |done, tot| {
                if let Some(ref cb) = prog_clone {
                    cb(
                        done,
                        tot,
                        format!("Downloading ATLauncher mods ({done}/{tot})..."),
                    );
                }
            })
            .await?;

        let _ = AtLauncherPackRegistry::save(
            &paths.instance_dir(&inst.instance_id),
            &AtLauncherPackRecord {
                schema_version: 1,
                source: "atlauncher".to_string(),
                pack_id: pack.id.to_string(),
                pack_name: pack.name.clone(),
                safe_name,
                version_id: chosen.version.clone(),
                version_name: chosen.version.clone(),
                managed_files,
                updated_at: String::new(),
            },
        );

        Ok(ModpackInstallReport {
            instance_id: inst.instance_id,
            game_version,
            loader,
            downloaded_files: total as usize,
            extracted_overrides: 0,
            manual_downloads: Vec::new(),
        })
    }
}

fn is_atlauncher_loader_entry(mod_type: &str, mod_name: &str) -> bool {
    let t = mod_type.trim().to_ascii_lowercase();
    let n = mod_name.trim().to_ascii_lowercase();
    t == "forge"
        || t == "neoforge"
        || n == "minecraft forge"
        || n == "forge"
        || n.contains("neoforge")
}

/// Extract and normalize ATLauncher modloader from `Configs.json` (matching Python `ATLauncherClient._loader`).
fn parse_atlauncher_loader(cfg: &serde_json::Value, game_version: &str) -> ModLoader {
    if let Some(l) = cfg.get("loader").and_then(|v| v.as_object()) {
        let ltype = l.get("type").and_then(|v| v.as_str()).unwrap_or("").trim();
        let meta = l.get("metadata").and_then(|v| v.as_object());
        let raw_ver = meta
            .and_then(|m| m.get("version").and_then(|v| v.as_str()))
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                meta.and_then(|m| m.get("loader").and_then(|v| v.as_str()))
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| {
                meta.and_then(|m| m.get("rawVersion").and_then(|v| v.as_str()))
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| {
                l.get("version")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
            })
            .unwrap_or("");
        if !ltype.is_empty() {
            return normalize_modloader(ltype, raw_ver, game_version);
        }
    }
    // Legacy pack fallback: inspect `mods[]` for embedded Forge/NeoForge entries
    if let Some(mods) = cfg.get("mods").and_then(|v| v.as_array()) {
        for m in mods {
            let mtype = m.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let mname = m.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let mver = m.get("version").and_then(|v| v.as_str()).unwrap_or("");
            let lower_name = mname.to_ascii_lowercase();
            if lower_name.contains("neoforge") {
                return normalize_modloader("neoforge", mver, game_version);
            }
            if mtype.eq_ignore_ascii_case("forge") || lower_name == "minecraft forge" {
                return normalize_modloader("forge", mver, game_version);
            }
        }
    }
    ModLoader::vanilla()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_atlauncher_packs_with_null_string_fields() {
        let json = r#"[
            {
                "id": 101,
                "name": "Demo Pack",
                "description": null,
                "safeName": null,
                "versions": [
                    { "version": "1.0.0", "minecraft": null }
                ]
            },
            {
                "id": 102,
                "name": "Empty Versions",
                "description": "Valid desc",
                "safeName": "EmptyVersions",
                "versions": null
            }
        ]"#;
        let packs: Vec<AtLauncherPack> = serde_json::from_str(json).unwrap();
        assert_eq!(packs.len(), 2);
        assert_eq!(packs[0].description, "");
        assert_eq!(packs[0].versions[0].minecraft, "");
        assert!(packs[1].versions.is_empty());
    }

    #[test]
    fn parses_modern_and_legacy_atlauncher_loaders() {
        let modern: serde_json::Value = serde_json::json!({
            "minecraft": "1.20.1",
            "loader": {
                "type": "MinecraftForge",
                "metadata": { "rawVersion": "1.20.1-47.3.0" }
            }
        });
        let l1 = parse_atlauncher_loader(&modern, "1.20.1");
        assert_eq!(l1.name, "forge");
        assert_eq!(l1.version, "47.3.0");

        let legacy: serde_json::Value = serde_json::json!({
            "minecraft": "1.12.2",
            "mods": [
                { "name": "Minecraft Forge", "type": "forge", "version": "14.23.5.2860" }
            ]
        });
        let l2 = parse_atlauncher_loader(&legacy, "1.12.2");
        assert_eq!(l2.name, "forge");
        assert_eq!(l2.version, "14.23.5.2860");
    }
}
