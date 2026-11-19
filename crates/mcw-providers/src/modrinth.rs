use crate::cache::ApiCache;
use crate::{ProviderError, Result};
use mcw_net::{DownloadClient, DownloadRequest};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

const MODRINTH_API_BASE: &str = "https://api.modrinth.com/v2";
const SEARCH_TTL_SECS: u64 = 120;
const VERSIONS_TTL_SECS: u64 = 300;

#[derive(Debug, Clone)]
pub struct ModrinthSearchParams {
    pub query: String,
    pub project_type: String,
    pub loader: Option<String>,
    pub game_version: Option<String>,
    pub index: String,
    pub limit: u32,
    pub offset: u32,
}

impl Default for ModrinthSearchParams {
    fn default() -> Self {
        Self {
            query: String::new(),
            project_type: "mod".to_string(),
            loader: None,
            game_version: None,
            index: "relevance".to_string(),
            limit: 20,
            offset: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModrinthSearchResult {
    pub hits: Vec<ModrinthSearchHit>,
    pub offset: u32,
    pub limit: u32,
    pub total_hits: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModrinthSearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub project_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModrinthVersion {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub files: Vec<ModrinthFile>,
    #[serde(default)]
    pub dependencies: Vec<ModrinthDependency>,
}

impl ModrinthVersion {
    pub fn primary_file(&self) -> Option<&ModrinthFile> {
        self.files
            .iter()
            .find(|f| f.primary)
            .or_else(|| self.files.first())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModrinthFile {
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub primary: bool,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub hashes: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModrinthDependency {
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    pub dependency_type: String,
}

#[derive(Debug, Clone)]
pub struct InstalledModFile {
    pub project_id: String,
    pub version_number: String,
    pub file_path: PathBuf,
    pub is_dependency: bool,
}

pub struct ModrinthClient {
    http: reqwest::Client,
    cache: Option<Arc<ApiCache>>,
}

impl ModrinthClient {
    pub fn new(cache: Option<Arc<ApiCache>>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(mcw_models::user_agent())
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self { http, cache })
    }

    pub async fn search(&self, params: &ModrinthSearchParams) -> Result<ModrinthSearchResult> {
        let mut facets: Vec<Vec<String>> = vec![vec![format!(
            "project_type:{}",
            params.project_type
        )]];
        if let Some(ref loader) = params.loader {
            if !loader.trim().is_empty() && !loader.eq_ignore_ascii_case("vanilla") {
                facets.push(vec![format!("categories:{}", loader.to_ascii_lowercase())]);
            }
        }
        if let Some(ref gv) = params.game_version {
            if !gv.trim().is_empty() {
                facets.push(vec![format!("versions:{gv}")]);
            }
        }

        let facets_json = serde_json::to_string(&facets)?;
        let mut url = Url::parse(&format!("{MODRINTH_API_BASE}/search")).unwrap();
        url.query_pairs_mut()
            .append_pair("query", &params.query)
            .append_pair("facets", &facets_json)
            .append_pair("index", &params.index)
            .append_pair("limit", &params.limit.to_string())
            .append_pair("offset", &params.offset.to_string());

        let raw = self
            .cached_get(url.as_str(), SEARCH_TTL_SECS)
            .await?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub async fn list_versions(
        &self,
        slug_or_id: &str,
        loader: Option<&str>,
        game_version: Option<&str>,
    ) -> Result<Vec<ModrinthVersion>> {
        let mut url =
            Url::parse(&format!("{MODRINTH_API_BASE}/project/{slug_or_id}/version")).unwrap();
        {
            let mut pairs = url.query_pairs_mut();
            if let Some(l) = loader {
                if !l.trim().is_empty() && !l.eq_ignore_ascii_case("vanilla") {
                    let loaders_json = serde_json::to_string(&[l.to_ascii_lowercase()])?;
                    pairs.append_pair("loaders", &loaders_json);
                }
            }
            if let Some(gv) = game_version {
                if !gv.trim().is_empty() {
                    let gvs_json = serde_json::to_string(&[gv])?;
                    pairs.append_pair("game_versions", &gvs_json);
                }
            }
        }

        let raw = self
            .cached_get(url.as_str(), VERSIONS_TTL_SECS)
            .await?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub async fn get_version(&self, version_id: &str) -> Result<ModrinthVersion> {
        let url = format!("{MODRINTH_API_BASE}/version/{version_id}");
        let raw = self.cached_get(&url, VERSIONS_TTL_SECS).await?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Install a Modrinth project and recursively resolve & download all `required` dependencies.
    pub async fn install_with_dependencies(
        &self,
        slug_or_id: &str,
        game_version: &str,
        loader: &str,
        mods_dir: &Path,
        downloader: &DownloadClient,
    ) -> Result<Vec<InstalledModFile>> {
        let mut queue: VecDeque<(String, Option<String>, bool)> = VecDeque::new();
        queue.push_back((slug_or_id.to_string(), None, false));
        let mut visited_projects: HashSet<String> = HashSet::new();
        let mut installed = Vec::new();

        while let Some((proj, pinned_version_id, is_dep)) = queue.pop_front() {
            let version = if let Some(vid) = pinned_version_id {
                self.get_version(&vid).await?
            } else {
                let versions = self
                    .list_versions(&proj, Some(loader), Some(game_version))
                    .await?;
                versions.into_iter().next().ok_or_else(|| {
                    ProviderError::NoCompatibleVersion {
                        project: proj.clone(),
                        game_version: game_version.to_string(),
                        loader: loader.to_string(),
                    }
                })?
            };

            if !visited_projects.insert(version.project_id.clone()) {
                continue;
            }

            let Some(file) = version.primary_file() else {
                continue;
            };
            let dest = mods_dir.join(&file.filename);
            let sha1 = file.hashes.get("sha1").cloned();
            let req = DownloadRequest::new(&file.url, &dest)
                .with_sha1(sha1)
                .with_size(file.size);
            downloader.download_file(&req).await?;

            installed.push(InstalledModFile {
                project_id: version.project_id.clone(),
                version_number: version.version_number.clone(),
                file_path: dest,
                is_dependency: is_dep,
            });

            for dep in &version.dependencies {
                if dep.dependency_type.eq_ignore_ascii_case("required") {
                    if let Some(ref dep_proj) = dep.project_id {
                        if !visited_projects.contains(dep_proj) {
                            queue.push_back((dep_proj.clone(), dep.version_id.clone(), true));
                        }
                    } else if let Some(ref dep_vid) = dep.version_id {
                        queue.push_back((String::new(), Some(dep_vid.clone()), true));
                    }
                }
            }
        }

        Ok(installed)
    }

    async fn cached_get(&self, url: &str, ttl_secs: u64) -> Result<String> {
        let cache_key = format!("modrinth:{url}");
        if let Some(ref cache) = self.cache {
            if let Some(hit) = cache.get(&cache_key) {
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
            let _ = cache.set(&cache_key, &text, ttl_secs);
        }
        Ok(text)
    }
}
