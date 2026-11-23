use crate::cache::ApiCache;
use crate::modpack::{normalize_modloader, ModpackInstallReport, ModpackProgressCallback};
use crate::pack_registry::{safe_relative_posix, FtbManagedFile, FtbPackRecord, FtbPackRegistry};
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

const FTB_API_BASE: &str = "https://api.feed-the-beast.com/v1/modpacks/public";
const FTB_TTL_SECS: u64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtbArtEntry {
    #[serde(default)]
    pub url: String,
    #[serde(rename = "type", default)]
    pub art_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtbPackSummary {
    pub id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub synopsis: String,
    #[serde(default)]
    pub installs: u64,
    #[serde(default)]
    pub art: Vec<FtbArtEntry>,
}

impl FtbPackSummary {
    pub fn icon_url(&self) -> Option<&str> {
        self.art
            .iter()
            .find(|a| a.art_type.eq_ignore_ascii_case("square") && !a.url.is_empty())
            .or_else(|| self.art.iter().find(|a| !a.url.is_empty()))
            .map(|a| a.url.as_str())
    }

    pub fn website_url(&self) -> String {
        format!("https://www.feed-the-beast.com/modpacks/{}", self.id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtbVersionSummary {
    pub id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub release_type: String,
    #[serde(default)]
    pub mc_version: String,
    #[serde(default)]
    pub loader: String,
    #[serde(default)]
    pub recommended_memory_mb: u32,
}

#[derive(Clone)]
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
        let lim = limit.clamp(1, 50);
        let url_str = if term.trim().is_empty() {
            format!("{FTB_API_BASE}/modpack/popular/installs/{lim}")
        } else {
            let mut url = Url::parse(&format!("{FTB_API_BASE}/modpack/search/{lim}")).unwrap();
            url.query_pairs_mut().append_pair("term", term.trim());
            url.to_string()
        };
        let raw = self.cached_get(&url_str).await?;
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

    /// Search FTB packs and fetch their metadata concurrently (matching Python FTBClient).
    pub async fn search_packs(&self, term: &str, limit: usize) -> Result<Vec<FtbPackSummary>> {
        let ids = self.search_ids(term, limit.min(50) as u32).await?;
        let mut join_set = tokio::task::JoinSet::new();
        for id in ids.into_iter().take(limit) {
            let client = self.clone();
            join_set.spawn(async move { client.get_pack(id).await.ok() });
        }
        let mut packs = Vec::new();
        while let Some(res) = join_set.join_next().await {
            if let Ok(Some(pack)) = res {
                if !pack.name.is_empty() {
                    packs.push(pack);
                }
            }
        }
        packs.sort_by_key(|p| std::cmp::Reverse(p.installs));
        Ok(packs)
    }

    /// List all downloadable versions of an FTB pack (newest first) for the install dialog.
    pub async fn get_pack_versions(&self, pack_id: u64) -> Result<Vec<FtbVersionSummary>> {
        let pack_raw = self
            .cached_get(&format!("{FTB_API_BASE}/modpack/{pack_id}"))
            .await?;
        let pack_json: serde_json::Value = serde_json::from_str(&pack_raw)?;
        let mut versions = Vec::new();
        if let Some(arr) = pack_json.get("versions").and_then(|v| v.as_array()) {
            for v in arr.iter().rev() {
                let Some(id) = v.get("id").and_then(|x| x.as_u64()) else {
                    continue;
                };
                let name = v
                    .get("name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let release_type = v
                    .get("type")
                    .and_then(|x| x.as_str())
                    .unwrap_or("Release")
                    .to_string();
                let mut mc_version = String::new();
                let mut loader = String::new();
                if let Some(targets) = v.get("targets").and_then(|x| x.as_array()) {
                    for t in targets {
                        let kind = t.get("type").and_then(|x| x.as_str()).unwrap_or("");
                        let tname = t.get("name").and_then(|x| x.as_str()).unwrap_or("");
                        let tver = t.get("version").and_then(|x| x.as_str()).unwrap_or("");
                        if kind.eq_ignore_ascii_case("game")
                            && tname.eq_ignore_ascii_case("minecraft")
                        {
                            mc_version = tver.to_string();
                        } else if kind.eq_ignore_ascii_case("modloader") && !tname.is_empty() {
                            loader = normalize_modloader(tname, tver, &mc_version).name;
                        }
                    }
                }
                let recommended_memory_mb = v
                    .get("specs")
                    .and_then(|s| s.get("recommended"))
                    .and_then(|r| r.as_u64())
                    .unwrap_or(0) as u32;
                versions.push(FtbVersionSummary {
                    id,
                    name: if name.is_empty() {
                        format!("v{id}")
                    } else {
                        name
                    },
                    release_type,
                    mc_version,
                    loader,
                    recommended_memory_mb,
                });
            }
        }
        Ok(versions)
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
        self.install_pack_version_with_progress(
            pack_id, None, pack_name, paths, downloader, progress,
        )
        .await
    }

    /// Download and install a specific version (or latest if `None`) of an FTB modpack.
    pub async fn install_pack_version_with_progress(
        &self,
        pack_id: u64,
        version_id: Option<u64>,
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

        let target_ver_id = if let Some(vid) = version_id {
            vid
        } else {
            emit(0, 0, format!("Fetching FTB pack #{pack_id} metadata..."));
            let pack_raw = self
                .cached_get(&format!("{FTB_API_BASE}/modpack/{pack_id}"))
                .await?;
            let pack_json: serde_json::Value = serde_json::from_str(&pack_raw)?;
            pack_json
                .get("versions")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.last())
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_u64())
                .ok_or_else(|| {
                    ProviderError::Io(std::io::Error::other(format!(
                        "FTB pack #{pack_id} has no downloadable versions"
                    )))
                })?
        };

        emit(0, 0, format!("Fetching FTB manifest v{target_ver_id}..."));
        let ver_raw = self
            .cached_get(&format!("{FTB_API_BASE}/modpack/{pack_id}/{target_ver_id}"))
            .await?;
        let ver_json: serde_json::Value = serde_json::from_str(&ver_raw)?;

        let mut game_version = "1.20.1".to_string();
        let mut loader_raw: Option<(String, String)> = None;
        if let Some(targets) = ver_json.get("targets").and_then(|v| v.as_array()) {
            for t in targets {
                let kind = t.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let ver = t.get("version").and_then(|v| v.as_str()).unwrap_or("");
                if kind.eq_ignore_ascii_case("game") && name.eq_ignore_ascii_case("minecraft") {
                    game_version = ver.to_string();
                } else if kind.eq_ignore_ascii_case("modloader") && !name.is_empty() {
                    loader_raw = Some((name.to_string(), ver.to_string()));
                }
            }
        }
        let loader = match loader_raw {
            Some((name, ver)) => normalize_modloader(&name, &ver, &game_version),
            None => ModLoader::vanilla(),
        };

        let store = InstanceStore::new(paths);
        let mut base_name = pack_name.trim().to_string();
        if base_name.is_empty() {
            base_name = format!("ftb-{pack_id}");
        } else {
            base_name = crate::modpack::suggest_short_instance_name(&base_name, 36);
        }
        let candidate_name = store
            .resolve_unique_instance_name(&base_name)
            .map_err(|e| ProviderError::Io(std::io::Error::other(e.to_string())))?;
        let (inst, mut inst_settings) = store
            .create_instance(&candidate_name, &game_version, loader.clone())
            .map_err(|e| ProviderError::Io(std::io::Error::other(e.to_string())))?;
        inst_settings.disable_mod_repair_and_updates = Some(true);
        inst_settings.disable_launch_checking = Some(true);
        let _ = store.save_settings(&inst.instance_id, &inst_settings);
        let mc_dir = paths.minecraft_dir(&inst.instance_id);
        fs::create_dir_all(&mc_dir)?;

        let mut requests = Vec::new();
        let mut managed_files = Vec::new();
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
                    let file_id = f.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                    let optional = f.get("optional").and_then(|v| v.as_bool()).unwrap_or(false);
                    let client_only = f
                        .get("clientonly")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let file_type = f
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    managed_files.push(FtbManagedFile {
                        file_id,
                        file_name: name.to_string(),
                        path: safe_relative_posix(&rel, name),
                        sha1: sha1.clone().unwrap_or_default(),
                        size,
                        urls: vec![url.to_string()],
                        optional,
                        client_only,
                        file_type,
                        provider: "ftb".to_string(),
                    });
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

        let ver_name = ver_json
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("v{target_ver_id}"));
        let _ = FtbPackRegistry::save(
            &paths.instance_dir(&inst.instance_id),
            &FtbPackRecord {
                schema_version: 2,
                source: "ftb".to_string(),
                pack_id,
                pack_name: pack_name.trim().to_string(),
                version_id: target_ver_id,
                version_name: ver_name,
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

    pub(crate) async fn cached_get(&self, url: &str) -> Result<String> {
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
