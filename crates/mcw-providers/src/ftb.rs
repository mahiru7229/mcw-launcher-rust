use crate::cache::ApiCache;
use crate::modpack::{ModpackInstallReport, ModpackProgressCallback};
use crate::{ProviderError, Result};
use mcw_models::instance::ModLoader;
use mcw_net::{DownloadClient, DownloadRequest};
use mcw_storage::{InstanceStore, LauncherPaths};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

const FTB_API_BASE: &str = "https://api.modpacks.ch/public";
const FTB_TTL_SECS: u64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtbPackSummary {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub synopsis: String,
    #[serde(default)]
    pub installs: u64,
}

pub struct FtbClient {
    http: reqwest::Client,
    cache: Option<Arc<ApiCache>>,
}

impl FtbClient {
    pub fn new(cache: Option<Arc<ApiCache>>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(mcw_models::user_agent())
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self { http, cache })
    }

    pub async fn search_ids(&self, term: &str, limit: u32) -> Result<Vec<u64>> {
        let mut url =
            Url::parse(&format!("{FTB_API_BASE}/modpack/search/{}", limit.min(50))).unwrap();
        url.query_pairs_mut().append_pair("term", term.trim());
        let raw = self.cached_get(url.as_str()).await?;
        let val: serde_json::Value = serde_json::from_str(&raw)?;
        let ids = val
            .get("packs")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
            .unwrap_or_default();
        Ok(ids)
    }

    pub async fn get_pack(&self, pack_id: u64) -> Result<FtbPackSummary> {
        let url = format!("{FTB_API_BASE}/modpack/{pack_id}");
        let raw = self.cached_get(&url).await?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Download and install the latest version of an FTB modpack directly into a new Instance.
    pub async fn install_pack_with_progress(
        &self,
        pack_id: u64,
        pack_name: &str,
        paths: &LauncherPaths,
        downloader: &DownloadClient,
        progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackInstallReport> {
        let emit = |cur: u64, tot: u64, msg: String| {
            if let Some(ref cb) = progress {
                cb(cur, tot, msg);
            }
        };

        emit(0, 0, format!("Fetching FTB pack #{pack_id} metadata..."));
        let pack_raw = self
            .cached_get(&format!("{FTB_API_BASE}/modpack/{pack_id}"))
            .await?;
        let pack_json: serde_json::Value = serde_json::from_str(&pack_raw)?;
        let latest_ver_id = pack_json
            .get("versions")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.last())
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ProviderError::Io(std::io::Error::other(format!(
                    "FTB pack #{pack_id} has no downloadable versions"
                )))
            })?;

        emit(0, 0, format!("Fetching FTB manifest v{latest_ver_id}..."));
        let ver_raw = self
            .cached_get(&format!("{FTB_API_BASE}/modpack/{pack_id}/{latest_ver_id}"))
            .await?;
        let ver_json: serde_json::Value = serde_json::from_str(&ver_raw)?;

        let mut game_version = "1.20.1".to_string();
        let mut loader = ModLoader::vanilla();
        if let Some(targets) = ver_json.get("targets").and_then(|v| v.as_array()) {
            for t in targets {
                let kind = t.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let ver = t.get("version").and_then(|v| v.as_str()).unwrap_or("");
                if kind.eq_ignore_ascii_case("game") && name.eq_ignore_ascii_case("minecraft") {
                    game_version = ver.to_string();
                } else if kind.eq_ignore_ascii_case("modloader") && !name.is_empty() {
                    loader = ModLoader {
                        name: name.to_ascii_lowercase(),
                        version: ver.to_string(),
                    };
                }
            }
        }

        let store = InstanceStore::new(paths);
        let mut candidate_name = pack_name.trim().to_string();
        if candidate_name.is_empty() {
            candidate_name = format!("ftb-{pack_id}");
        }
        let mut idx = 2usize;
        while store.load_instance(&candidate_name).is_ok() {
            candidate_name = format!("{} ({idx})", pack_name.trim());
            idx += 1;
        }
        let (inst, _) = store
            .create_instance(&candidate_name, &game_version, loader.clone())
            .map_err(|e| ProviderError::Io(std::io::Error::other(e.to_string())))?;
        let mc_dir = paths.minecraft_dir(&inst.instance_id);
        fs::create_dir_all(&mc_dir)?;

        let mut requests = Vec::new();
        if let Some(files) = ver_json.get("files").and_then(|v| v.as_array()) {
            for f in files {
                if f.get("serveronly")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    continue;
                }
                let url = f.get("url").and_then(|v| v.as_str()).unwrap_or("");
                if url.is_empty() {
                    continue;
                }
                let rel_dir = f
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("./")
                    .trim_start_matches("./");
                let name = f.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if name.is_empty() {
                    continue;
                }
                let rel = if rel_dir.is_empty() {
                    name.to_string()
                } else {
                    format!("{}/{name}", rel_dir.trim_end_matches('/'))
                };
                if let Some(dest) = safe_join(&mc_dir, &rel) {
                    let sha1 = f
                        .get("sha1")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(String::from);
                    let size = f.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                    requests.push(
                        DownloadRequest::new(url, dest)
                            .with_sha1(sha1)
                            .with_size(size),
                    );
                }
            }
        }

        let total = requests.len() as u64;
        emit(0, total, format!("Downloading {total} FTB pack files..."));
        let prog_clone = progress.clone();
        downloader
            .download_batch(requests, 16, move |done, tot| {
                if let Some(ref cb) = prog_clone {
                    cb(
                        done,
                        tot,
                        format!("Downloading FTB files ({done}/{tot})..."),
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

    async fn cached_get(&self, url: &str) -> Result<String> {
        let key = format!("ftb:{url}");
        if let Some(ref cache) = self.cache {
            if let Some(hit) = cache.get(&key) {
                return Ok(hit);
            }
        }
        let resp = self.http.get(url).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(ProviderError::ApiError {
                status: status.as_u16(),
                message: text,
            });
        }
        if let Some(ref cache) = self.cache {
            let _ = cache.set(&key, &text, FTB_TTL_SECS);
        }
        Ok(text)
    }
}

fn safe_join(base: &Path, relative: &str) -> Option<PathBuf> {
    let normalized = relative.replace('\\', "/");
    let mut out = base.to_path_buf();
    for comp in Path::new(&normalized).components() {
        match comp {
            Component::Normal(p) => out.push(p),
            _ => return None,
        }
    }
    Some(out)
}
