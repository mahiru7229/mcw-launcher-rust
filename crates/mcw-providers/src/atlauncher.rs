use crate::cache::ApiCache;
use crate::modpack::{ModpackInstallReport, ModpackProgressCallback};
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
        let q = query.trim().to_ascii_lowercase();
        let packs = self.list_packs().await?;
        Ok(packs
            .into_iter()
            .filter(|p| {
                q.is_empty()
                    || p.name.to_ascii_lowercase().contains(&q)
                    || p.description.to_ascii_lowercase().contains(&q)
            })
            .take(limit)
            .collect())
    }

    /// Download and install the latest version of an ATLauncher modpack into a new Instance.
    pub async fn install_pack_with_progress(
        &self,
        pack: &AtLauncherPack,
        paths: &LauncherPaths,
        downloader: &DownloadClient,
        progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackInstallReport> {
        let emit = |cur: u64, tot: u64, msg: String| {
            if let Some(ref cb) = progress {
                cb(cur, tot, msg);
            }
        };

        let latest = pack.versions.first().ok_or_else(|| {
            ProviderError::Io(std::io::Error::other(format!(
                "ATLauncher pack '{}' has no published versions",
                pack.name
            )))
        })?;
        let safe_name = if pack.safe_name.is_empty() {
            pack.name.replace(' ', "")
        } else {
            pack.safe_name.clone()
        };

        emit(
            0,
            0,
            format!(
                "Fetching ATLauncher manifest ({} v{})...",
                pack.name, latest.version
            ),
        );
        let cfg_url = format!(
            "{ATLAUNCHER_CDN_BASE}packs/{safe_name}/versions/{}/Configs.json",
            latest.version
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
            .unwrap_or(if latest.minecraft.is_empty() {
                "1.20.1"
            } else {
                &latest.minecraft
            })
            .to_string();

        let loader = if let Some(l) = cfg.get("loader") {
            let ltype = l
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("vanilla")
                .to_ascii_lowercase();
            let lver = l
                .get("metadata")
                .and_then(|m| m.get("version"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if ltype == "vanilla" || lver.is_empty() {
                ModLoader::vanilla()
            } else {
                ModLoader {
                    name: ltype,
                    version: lver,
                }
            }
        } else {
            ModLoader::vanilla()
        };

        let store = InstanceStore::new(paths);
        let mut candidate_name = pack.name.trim().to_string();
        if candidate_name.is_empty() {
            candidate_name = safe_name.clone();
        }
        let mut idx = 2usize;
        while store.load_instance(&candidate_name).is_ok() {
            candidate_name = format!("{} ({idx})", pack.name.trim());
            idx += 1;
        }
        let (inst, _) = store
            .create_instance(&candidate_name, &game_version, loader.clone())
            .map_err(|e| ProviderError::Io(std::io::Error::other(e.to_string())))?;
        let mc_dir = paths.minecraft_dir(&inst.instance_id);
        let mods_dir = mc_dir.join("mods");
        fs::create_dir_all(&mods_dir)?;

        let mut requests = Vec::new();
        if let Some(mods) = cfg.get("mods").and_then(|v| v.as_array()) {
            for m in mods {
                if !m.get("client").and_then(|v| v.as_bool()).unwrap_or(true) {
                    continue;
                }
                if m.get("optional").and_then(|v| v.as_bool()).unwrap_or(false)
                    && !m.get("selected").and_then(|v| v.as_bool()).unwrap_or(false)
                {
                    continue;
                }
                let file = m.get("file").and_then(|v| v.as_str()).unwrap_or("");
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
}
