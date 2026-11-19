use crate::cache::ApiCache;
use crate::{ProviderError, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

const CURSEFORGE_API_BASE: &str = "https://api.curseforge.com/v1";
const MINECRAFT_GAME_ID: u32 = 432;
const SEARCH_TTL_SECS: u64 = 120;
const FILES_TTL_SECS: u64 = 300;
const BATCH_CHUNK_SIZE: usize = 50;

pub fn class_id_for(kind: &str) -> u32 {
    match kind.trim().to_ascii_lowercase().as_str() {
        "modpack" => 4471,
        "resourcepack" => 12,
        "shader" => 6552,
        _ => 6, // mod
    }
}

pub fn loader_type_id(loader: &str) -> Option<u32> {
    match loader.trim().to_ascii_lowercase().as_str() {
        "forge" => Some(1),
        "cauldron" => Some(2),
        "liteloader" => Some(3),
        "fabric" => Some(4),
        "quilt" => Some(5),
        "neoforge" => Some(6),
        _ => None,
    }
}

pub fn sort_field_id(sort: &str) -> u32 {
    match sort.trim().to_ascii_lowercase().as_str() {
        "updated" => 3,
        "downloads" => 6,
        "newest" => 11,
        _ => 2, // popularity
    }
}

#[derive(Debug, Clone)]
pub struct CurseForgeSearchParams {
    pub query: String,
    pub kind: String,
    pub loader: Option<String>,
    pub game_version: Option<String>,
    pub sort: String,
    pub page_size: u32,
    pub index: u32,
}

impl Default for CurseForgeSearchParams {
    fn default() -> Self {
        Self {
            query: String::new(),
            kind: "mod".to_string(),
            loader: None,
            game_version: None,
            sort: "popularity".to_string(),
            page_size: 20,
            index: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurseForgeSearchResult {
    pub data: Vec<CurseForgeMod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurseForgeMod {
    pub id: u64,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub summary: String,
    #[serde(rename = "downloadCount", default)]
    pub download_count: u64,
    #[serde(default)]
    pub links: Option<CurseForgeLinks>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurseForgeLinks {
    #[serde(rename = "websiteUrl", default)]
    pub website_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurseForgeFile {
    pub id: u64,
    #[serde(rename = "modId")]
    pub mod_id: u64,
    #[serde(rename = "fileName")]
    pub file_name: String,
    #[serde(rename = "fileLength", default)]
    pub file_length: u64,
    #[serde(rename = "downloadUrl", default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub hashes: Vec<CurseForgeHash>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurseForgeHash {
    pub value: String,
    /// 1 = SHA-1, 2 = MD5
    pub algo: u32,
}

impl CurseForgeFile {
    pub fn sha1(&self) -> Option<String> {
        self.hashes
            .iter()
            .find(|h| h.algo == 1)
            .map(|h| h.value.clone())
    }

    /// Resolve direct download URL, or return a structured `ManualDownloadRequired` error
    /// when the author disabled third-party distribution (`downloadUrl == null`).
    pub fn require_download_url(&self, project_slug: Option<&str>) -> Result<String> {
        if let Some(ref url) = self.download_url {
            if !url.trim().is_empty() {
                return Ok(url.clone());
            }
        }
        let website_url = match project_slug {
            Some(slug) if !slug.is_empty() => format!(
                "https://www.curseforge.com/minecraft/mc-mods/{slug}/files/{}",
                self.id
            ),
            _ => format!("https://www.curseforge.com/projects/{}/files/{}", self.mod_id, self.id),
        };
        Err(ProviderError::ManualDownloadRequired {
            mod_id: self.mod_id,
            file_id: self.id,
            file_name: self.file_name.clone(),
            website_url,
        })
    }
}

pub struct CurseForgeClient {
    http: reqwest::Client,
    api_key: String,
    cache: Option<Arc<ApiCache>>,
}

impl CurseForgeClient {
    pub fn new(api_key: impl Into<String>, cache: Option<Arc<ApiCache>>) -> Result<Self> {
        let key = api_key.into().trim().to_string();
        if key.is_empty() {
            return Err(ProviderError::MissingApiKey);
        }
        let http = reqwest::Client::builder()
            .user_agent(mcw_models::user_agent())
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self {
            http,
            api_key: key,
            cache,
        })
    }

    pub fn from_env(cache: Option<Arc<ApiCache>>) -> Result<Self> {
        let key = std::env::var("CURSEFORGE_API_KEY").unwrap_or_default();
        Self::new(key, cache)
    }

    pub async fn search(&self, params: &CurseForgeSearchParams) -> Result<CurseForgeSearchResult> {
        let mut url = Url::parse(&format!("{CURSEFORGE_API_BASE}/mods/search")).unwrap();
        {
            let mut pairs = url.query_pairs_mut();
            pairs
                .append_pair("gameId", &MINECRAFT_GAME_ID.to_string())
                .append_pair("classId", &class_id_for(&params.kind).to_string())
                .append_pair("sortField", &sort_field_id(&params.sort).to_string())
                .append_pair("sortOrder", "desc")
                .append_pair("pageSize", &params.page_size.min(50).to_string())
                .append_pair("index", &params.index.to_string());
            if !params.query.trim().is_empty() {
                pairs.append_pair("searchFilter", params.query.trim());
            }
            if let Some(ref gv) = params.game_version {
                if !gv.trim().is_empty() {
                    pairs.append_pair("gameVersion", gv.trim());
                }
            }
            if let Some(ref loader) = params.loader {
                if let Some(id) = loader_type_id(loader) {
                    pairs.append_pair("modLoaderType", &id.to_string());
                }
            }
        }

        let raw = self.cached_get(url.as_str(), SEARCH_TTL_SECS).await?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Fetch mod files with explicit `gameVersion` & `modLoaderType` query filters
    /// (critical for mods with >50 files so older/target game versions aren't missed).
    pub async fn get_mod_files(
        &self,
        mod_id: u64,
        game_version: Option<&str>,
        loader: Option<&str>,
    ) -> Result<Vec<CurseForgeFile>> {
        let mut url =
            Url::parse(&format!("{CURSEFORGE_API_BASE}/mods/{mod_id}/files")).unwrap();
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("pageSize", "50");
            if let Some(gv) = game_version {
                if !gv.trim().is_empty() {
                    pairs.append_pair("gameVersion", gv.trim());
                }
            }
            if let Some(l) = loader {
                if let Some(id) = loader_type_id(l) {
                    pairs.append_pair("modLoaderType", &id.to_string());
                }
            }
        }

        let raw = self.cached_get(url.as_str(), FILES_TTL_SECS).await?;
        let val: serde_json::Value = serde_json::from_str(&raw)?;
        let data = val.get("data").cloned().unwrap_or_else(|| json!([]));
        Ok(serde_json::from_value(data)?)
    }

    /// Batch-resolve CurseForge file metadata in chunks of 50 IDs (for large modpacks).
    pub async fn get_files_batch(&self, file_ids: &[u64]) -> Result<Vec<CurseForgeFile>> {
        let mut all = Vec::with_capacity(file_ids.len());
        let url = format!("{CURSEFORGE_API_BASE}/mods/files");

        for chunk in file_ids.chunks(BATCH_CHUNK_SIZE) {
            let resp = self
                .http
                .post(&url)
                .header("x-api-key", &self.api_key)
                .json(&json!({ "fileIds": chunk }))
                .send()
                .await?;

            let status = resp.status();
            let text = resp.text().await?;
            if !status.is_success() {
                return Err(ProviderError::ApiError {
                    status: status.as_u16(),
                    message: text,
                });
            }
            let val: serde_json::Value = serde_json::from_str(&text)?;
            let data = val.get("data").cloned().unwrap_or_else(|| json!([]));
            let mut batch: Vec<CurseForgeFile> = serde_json::from_value(data)?;
            all.append(&mut batch);
        }
        Ok(all)
    }

    async fn cached_get(&self, url: &str, ttl_secs: u64) -> Result<String> {
        let cache_key = format!("curseforge:{url}");
        if let Some(ref cache) = self.cache {
            if let Some(hit) = cache.get(&cache_key) {
                return Ok(hit);
            }
        }

        let resp = self
            .http
            .get(url)
            .header("x-api-key", &self.api_key)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(ProviderError::ApiError {
                status: status.as_u16(),
                message: text,
            });
        }

        if let Some(ref cache) = self.cache {
            let _ = cache.set(&cache_key, &text, ttl_secs);
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_distribution_returns_manual_download_error() {
        let file = CurseForgeFile {
            id: 55555,
            mod_id: 12345,
            file_name: "opt-out-mod.jar".to_string(),
            file_length: 1024,
            download_url: None,
            hashes: vec![],
        };
        let err = file.require_download_url(Some("cool-mod")).unwrap_err();
        assert!(matches!(err, ProviderError::ManualDownloadRequired { .. }));
        assert!(err
            .to_string()
            .contains("https://www.curseforge.com/minecraft/mc-mods/cool-mod/files/55555"));
    }
}
