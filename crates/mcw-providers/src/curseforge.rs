use crate::cache::ApiCache;
use crate::modpack::{ModpackInstallReport, ModpackInstaller, ModpackProgressCallback};
use crate::{ProviderError, Result};
use mcw_net::{DownloadClient, DownloadRequest};
use mcw_storage::{atomic_write_text, LauncherPaths};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

include!(concat!(env!("OUT_DIR"), "/embedded_cf_key.rs"));

const KEYSTREAM_DOMAIN: &[u8] = b"mcw-launcher-cf-keystream-v1";

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
        "datapack" => 6945,
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
        "relevance" | "featured" => 1,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CurseForgePagination {
    pub index: u32,
    pub page_size: u32,
    pub result_count: u32,
    pub total_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurseForgeSearchResult {
    pub data: Vec<CurseForgeMod>,
    #[serde(default)]
    pub pagination: Option<CurseForgePagination>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurseForgeLogo {
    #[serde(rename = "thumbnailUrl", default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurseForgeAuthor {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurseForgeScreenshot {
    #[serde(rename = "thumbnailUrl", default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
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
    pub authors: Vec<CurseForgeAuthor>,
    #[serde(default)]
    pub screenshots: Vec<CurseForgeScreenshot>,
    #[serde(default)]
    pub links: Option<CurseForgeLinks>,
    #[serde(default)]
    pub logo: Option<CurseForgeLogo>,
}

impl CurseForgeMod {
    pub fn icon_url(&self) -> Option<&str> {
        self.logo
            .as_ref()
            .and_then(|l| l.thumbnail_url.as_deref().or(l.url.as_deref()))
            .filter(|u| !u.trim().is_empty())
    }

    pub fn primary_author(&self) -> String {
        let names: Vec<&str> = self
            .authors
            .iter()
            .map(|a| a.name.trim())
            .filter(|s| !s.is_empty())
            .take(2)
            .collect();
        names.join(", ")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurseForgeLinks {
    #[serde(rename = "websiteUrl", default)]
    pub website_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurseForgeDependency {
    #[serde(rename = "modId")]
    pub mod_id: u64,
    /// CurseForge relationType: 1 = EmbeddedLibrary, 2 = OptionalDependency, 3 = RequiredDependency.
    #[serde(rename = "relationType", default)]
    pub relation_type: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurseForgeFile {
    pub id: u64,
    #[serde(rename = "modId")]
    pub mod_id: u64,
    #[serde(rename = "displayName", default)]
    pub display_name: String,
    #[serde(rename = "fileName")]
    pub file_name: String,
    /// CurseForge release channel: 1 = Release, 2 = Beta, 3 = Alpha.
    #[serde(rename = "releaseType", default = "default_cf_release_type")]
    pub release_type: u32,
    #[serde(rename = "fileLength", default)]
    pub file_length: u64,
    #[serde(rename = "downloadUrl", default)]
    pub download_url: Option<String>,
    #[serde(rename = "gameVersions", default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub hashes: Vec<CurseForgeHash>,
    #[serde(default)]
    pub dependencies: Vec<CurseForgeDependency>,
}

fn default_cf_release_type() -> u32 {
    1
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
    /// routing straight to CurseForge's countdown download page (`/download/{file_id}`).
    pub fn require_download_url(&self, project_url_or_slug: Option<&str>) -> Result<String> {
        if let Some(ref url) = self.download_url {
            if !url.trim().is_empty() {
                return Ok(url.clone());
            }
        }
        let website_url = build_curseforge_countdown_url(project_url_or_slug, self.mod_id, self.id);
        Err(ProviderError::ManualDownloadRequired {
            mod_id: self.mod_id,
            file_id: self.id,
            file_name: self.file_name.clone(),
            website_url,
        })
    }
}

/// Build the direct CurseForge countdown download URL (`.../download/{file_id}`),
/// matching `mcw-launcher` commit `1585bdf`. Never emits broken `/projects/<id>/files/<id>` URLs.
pub fn build_curseforge_countdown_url(
    project_url_or_slug: Option<&str>,
    mod_id: u64,
    file_id: u64,
) -> String {
    if let Some(raw) = project_url_or_slug.map(str::trim).filter(|s| !s.is_empty()) {
        if raw.starts_with("https://") || raw.starts_with("http://") {
            let base = raw.trim_end_matches('/');
            if file_id > 0 {
                return format!("{base}/download/{file_id}");
            }
            return base.to_string();
        }
        if !raw.chars().all(|c| c.is_ascii_digit()) {
            if file_id > 0 {
                return format!(
                    "https://www.curseforge.com/minecraft/mc-mods/{raw}/download/{file_id}"
                );
            }
            return format!("https://www.curseforge.com/minecraft/mc-mods/{raw}");
        }
    }
    format!("https://www.curseforge.com/minecraft/search?search={mod_id}")
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
        let key = load_curseforge_api_key(None).unwrap_or_default();
        Self::new(key, cache)
    }

    pub fn from_root(project_root: &Path, cache: Option<Arc<ApiCache>>) -> Result<Self> {
        let key = load_curseforge_api_key(Some(project_root)).unwrap_or_default();
        Self::new(key, cache)
    }

    /// Download and install the latest compatible mod `.jar` for `(mod_id, game_version, loader)`,
    /// automatically resolving required dependencies (`relationType == 3`) recursively.
    pub async fn install_mod(
        &self,
        mod_id: u64,
        slug: Option<&str>,
        game_version: &str,
        loader: &str,
        mods_dir: &Path,
        downloader: &DownloadClient,
    ) -> Result<PathBuf> {
        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back((mod_id, slug.map(String::from)));
        let mut primary_dest: Option<PathBuf> = None;

        fs::create_dir_all(mods_dir)?;

        while let Some((cur_mod_id, cur_slug)) = queue.pop_front() {
            if !visited.insert(cur_mod_id) {
                continue;
            }
            let files = match self
                .get_mod_files(cur_mod_id, Some(game_version), Some(loader))
                .await
            {
                Ok(list) => list,
                Err(err) if primary_dest.is_some() => {
                    tracing::warn!(mod_id = cur_mod_id, error = %err, "Skipping unresolved transitive CurseForge dependency");
                    continue;
                }
                Err(err) => return Err(err),
            };
            let Some(file) = files.into_iter().next() else {
                if primary_dest.is_none() {
                    return Err(ProviderError::NoCompatibleVersion {
                        project: cur_slug.unwrap_or_else(|| format!("mod-{cur_mod_id}")),
                        game_version: game_version.to_string(),
                        loader: loader.to_string(),
                    });
                }
                continue;
            };

            for dep in &file.dependencies {
                if dep.relation_type == 3 && !visited.contains(&dep.mod_id) {
                    queue.push_back((dep.mod_id, None));
                }
            }

            let url = match file.require_download_url(cur_slug.as_deref()) {
                Ok(u) => u,
                Err(err) if primary_dest.is_some() => {
                    tracing::warn!(mod_id = cur_mod_id, error = %err, "Skipping opt-out transitive CurseForge dependency");
                    continue;
                }
                Err(err) => return Err(err),
            };
            let dest = mods_dir.join(&file.file_name);
            let req = DownloadRequest::new(url, &dest)
                .with_sha1(file.sha1())
                .with_size(file.file_length);
            downloader.download_file(&req).await?;
            if primary_dest.is_none() {
                primary_dest = Some(dest);
            }
        }

        primary_dest.ok_or_else(|| ProviderError::NoCompatibleVersion {
            project: slug
                .map(String::from)
                .unwrap_or_else(|| format!("mod-{mod_id}")),
            game_version: game_version.to_string(),
            loader: loader.to_string(),
        })
    }

    /// Resolve and download required CurseForge dependencies (`relationType == 3`) for a specific `file_id`.
    pub async fn install_required_dependencies_for_file(
        &self,
        mod_id: u64,
        file_id: u64,
        game_version: &str,
        loader: &str,
        mods_dir: &Path,
        downloader: &DownloadClient,
    ) -> Result<Vec<PathBuf>> {
        let files = self.get_files_batch(&[file_id]).await?;
        let Some(root_file) = files.into_iter().next() else {
            return Ok(Vec::new());
        };
        let mut visited = std::collections::HashSet::new();
        visited.insert(mod_id);
        let mut installed = Vec::new();
        for dep in &root_file.dependencies {
            if dep.relation_type == 3 && visited.insert(dep.mod_id) {
                if let Ok(p) = self
                    .install_mod(dep.mod_id, None, game_version, loader, mods_dir, downloader)
                    .await
                {
                    installed.push(p);
                }
            }
        }
        Ok(installed)
    }

    /// Install a CurseForge resource pack or shader pack into `target_dir` (prefers matching
    /// `game_version`, falls back to latest file if broad compatibility).
    pub async fn install_content_pack(
        &self,
        mod_id: u64,
        slug: Option<&str>,
        game_version: &str,
        target_dir: &Path,
        downloader: &DownloadClient,
    ) -> Result<PathBuf> {
        fs::create_dir_all(target_dir)?;
        let mut files = self.get_mod_files(mod_id, Some(game_version), None).await?;
        if files.is_empty() {
            files = self.get_mod_files(mod_id, None, None).await?;
        }
        let file = files
            .into_iter()
            .next()
            .ok_or_else(|| ProviderError::NoCompatibleVersion {
                project: slug.unwrap_or("curseforge-pack").to_string(),
                game_version: game_version.to_string(),
                loader: "any".to_string(),
            })?;

        let download_url = file.require_download_url(slug)?;
        let dest = target_dir.join(&file.file_name);
        let sha1 = file
            .hashes
            .iter()
            .find(|h| h.algo == 1)
            .map(|h| h.value.clone());
        let req = DownloadRequest::new(download_url, &dest)
            .with_sha1(sha1)
            .with_size(file.file_length);
        downloader.download_file(&req).await?;
        Ok(dest)
    }

    /// Download the latest CurseForge modpack `.zip` and install it as a new Instance.
    pub async fn install_modpack(
        &self,
        mod_id: u64,
        slug: Option<&str>,
        instance_name_override: Option<&str>,
        paths: &LauncherPaths,
        downloader: &DownloadClient,
    ) -> Result<ModpackInstallReport> {
        self.install_modpack_with_progress(
            mod_id,
            slug,
            instance_name_override,
            paths,
            downloader,
            None,
        )
        .await
    }

    pub async fn install_modpack_with_progress(
        &self,
        mod_id: u64,
        slug: Option<&str>,
        instance_name_override: Option<&str>,
        paths: &LauncherPaths,
        downloader: &DownloadClient,
        on_progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackInstallReport> {
        if let Some(ref cb) = on_progress {
            cb(0, 1, "Fetching modpack archive metadata...".to_string());
        }
        let files = self.get_mod_files(mod_id, None, None).await?;
        let file = files
            .into_iter()
            .next()
            .ok_or_else(|| ProviderError::NoCompatibleVersion {
                project: slug
                    .map(String::from)
                    .unwrap_or_else(|| format!("modpack-{mod_id}")),
                game_version: "any".to_string(),
                loader: "modpack".to_string(),
            })?;

        let url = file.require_download_url(slug)?;
        let pack_dir = paths.cache_root.join("modpacks");
        fs::create_dir_all(&pack_dir)?;
        let archive_path = pack_dir.join(&file.file_name);
        if let Some(ref cb) = on_progress {
            cb(0, 1, format!("Downloading {}...", file.file_name));
        }
        let req = DownloadRequest::new(url, &archive_path)
            .with_sha1(file.sha1())
            .with_size(file.file_length);
        downloader.download_file(&req).await?;

        let rep = ModpackInstaller::install_from_archive_with_progress(
            paths,
            &archive_path,
            instance_name_override,
            downloader,
            Some(self),
            on_progress,
        )
        .await?;
        let _ = crate::pack_registry::CurseForgePackRegistry::set_origin_metadata(
            &paths.instance_dir(&rep.instance_id),
            mod_id,
            slug.unwrap_or(""),
            file.id,
            &file.file_name,
        );
        Ok(rep)
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
        let mut url = Url::parse(&format!("{CURSEFORGE_API_BASE}/mods/{mod_id}/files")).unwrap();
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

    /// Batch-resolve CurseForge mod metadata (including `slug` and `links.websiteUrl`)
    /// in chunks of 50 IDs (`POST /v1/mods`) so manual download links route directly to
    /// `https://www.curseforge.com/minecraft/.../{slug}/download/{file_id}`.
    pub async fn get_mods_batch(&self, mod_ids: &[u64]) -> Result<Vec<CurseForgeMod>> {
        let mut all = Vec::with_capacity(mod_ids.len());
        let url = format!("{CURSEFORGE_API_BASE}/mods");

        for chunk in mod_ids.chunks(BATCH_CHUNK_SIZE) {
            let resp = self
                .http
                .post(&url)
                .header("x-api-key", &self.api_key)
                .json(&json!({ "modIds": chunk }))
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
            let mut batch: Vec<CurseForgeMod> = serde_json::from_value(data)?;
            all.append(&mut batch);
        }
        Ok(all)
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

    /// Fetches a project's long HTML description (`GET /v1/mods/{mod_id}/description`)
    /// and converts it to a concise, plain-text readable preview.
    pub async fn get_mod_description(&self, mod_id: u64) -> Result<String> {
        let url = format!("{CURSEFORGE_API_BASE}/mods/{mod_id}/description");
        let raw = self.cached_get(&url, FILES_TTL_SECS).await?;
        let val: serde_json::Value = serde_json::from_str(&raw)?;
        let html = val.get("data").and_then(|d| d.as_str()).unwrap_or("");
        Ok(strip_html_to_preview(html, 2400))
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

/// Converts CurseForge HTML project descriptions into concise readable plain-text paragraphs.
pub fn strip_html_to_preview(html: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(html.len().min(max_chars.saturating_add(128)));
    let mut chars = html.chars().peekable();
    let mut skip_depth = 0usize;

    while let Some(ch) = chars.next() {
        if ch == '<' {
            let mut tag = String::new();
            for tc in chars.by_ref() {
                if tc == '>' {
                    break;
                }
                tag.push(tc);
            }
            let tag_trim = tag.trim().to_ascii_lowercase();
            if tag_trim.starts_with("script") || tag_trim.starts_with("style") {
                skip_depth += 1;
            } else if tag_trim.starts_with("/script") || tag_trim.starts_with("/style") {
                skip_depth = skip_depth.saturating_sub(1);
            } else if skip_depth == 0 {
                if tag_trim.starts_with("br")
                    || tag_trim.starts_with("/p")
                    || tag_trim.starts_with("/div")
                    || tag_trim.starts_with("/h1")
                    || tag_trim.starts_with("/h2")
                    || tag_trim.starts_with("/h3")
                    || tag_trim.starts_with("/h4")
                    || tag_trim.starts_with("/li")
                {
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                } else if tag_trim == "li" || tag_trim.starts_with("li ") {
                    if !out.ends_with('\n') && !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str("- ");
                }
            }
        } else if skip_depth == 0 {
            out.push(ch);
        }
    }

    let decoded = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&ensp;", " ")
        .replace("&emsp;", " ")
        .replace("&thinsp;", " ")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&hellip;", "...");

    let mut clean_lines = Vec::new();
    let mut prev_blank = true;
    for raw_line in decoded.lines() {
        let trimmed = raw_line.split_whitespace().collect::<Vec<_>>().join(" ");
        if trimmed.is_empty() {
            if !prev_blank {
                clean_lines.push(String::new());
                prev_blank = true;
            }
        } else {
            clean_lines.push(trimmed);
            prev_blank = false;
        }
    }
    let joined = clean_lines.join("\n").trim().to_string();
    if max_chars == 0 || joined.chars().count() <= max_chars {
        return joined;
    }
    let mut clipped: String = joined.chars().take(max_chars.saturating_sub(3)).collect();
    while clipped.ends_with(char::is_whitespace) {
        clipped.pop();
    }
    clipped.push_str("...");
    clipped
}

/// Path to custom CurseForge API key config under `config/curseforge.json`.
pub fn custom_curseforge_key_path(config_root: &Path) -> PathBuf {
    config_root.join("curseforge.json")
}

/// Load a user-saved custom CurseForge API key from `config/curseforge.json`.
pub fn load_custom_curseforge_api_key(config_root: &Path) -> Option<String> {
    let path = custom_curseforge_key_path(config_root);
    let raw = fs::read_to_string(path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&raw).ok()?;
    val.get("api_key")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.contains(['\r', '\n', '\0']))
        .map(String::from)
}

/// Save or clear a custom CurseForge API key in `config/curseforge.json`.
pub fn save_custom_curseforge_api_key(config_root: &Path, api_key: &str) -> Result<()> {
    let clean = api_key.trim();
    if clean.contains(['\r', '\n', '\0']) {
        return Err(ProviderError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid CurseForge API key.",
        )));
    }
    let path = custom_curseforge_key_path(config_root);
    if clean.is_empty() {
        if path.is_file() {
            let _ = fs::remove_file(path);
        }
        return Ok(());
    }
    let payload = serde_json::json!({
        "schema_version": 1,
        "api_key": clean,
    });
    let serialized = serde_json::to_string_pretty(&payload)?;
    atomic_write_text(&path, &serialized).map_err(|e| ProviderError::Io(std::io::Error::other(e)))
}

/// Load `CURSEFORGE_API_KEY` from process environment, `config/curseforge.json`, or `.env` files.
pub fn load_curseforge_api_key(project_root: Option<&Path>) -> Option<String> {
    if let Ok(val) = std::env::var("CURSEFORGE_API_KEY") {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    if let Some(root) = project_root {
        if let Some(custom) = load_custom_curseforge_api_key(&root.join("config")) {
            return Some(custom);
        }
    }

    let mut candidates = Vec::new();
    if let Some(root) = project_root {
        candidates.push(root.join(".env"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(".env"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(".env"));
            // When running `target/debug/mcw-desktop.exe`, also check workspace root 2 levels up.
            if let Some(grandparent) = dir.parent().and_then(|p| p.parent()) {
                candidates.push(grandparent.join(".env"));
            }
        }
    }

    for path in candidates {
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Some(val) = parse_dotenv_key(&raw, "CURSEFORGE_API_KEY") {
                return Some(val);
            }
        }
    }
    decrypt_embedded_key(&EMBEDDED_CF_SALT, EMBEDDED_CF_CIPHERTEXT)
}

/// Decrypts the compile-time encrypted CurseForge API key using a SHA-256 CTR keystream.
/// Zero plaintext bytes of the embedded key are stored in the binary.
fn decrypt_embedded_key(salt: &[u8; 32], ciphertext: &[u8]) -> Option<String> {
    if ciphertext.is_empty() {
        return None;
    }
    let mut plain = Vec::with_capacity(ciphertext.len());
    for (block_idx, chunk) in ciphertext.chunks(32).enumerate() {
        let mut hasher = Sha256::new();
        hasher.update(KEYSTREAM_DOMAIN);
        hasher.update(salt);
        hasher.update((block_idx as u32).to_le_bytes());
        let keystream = hasher.finalize();
        for (b, k) in chunk.iter().zip(keystream.iter()) {
            plain.push(b ^ k);
        }
    }
    let s = String::from_utf8(plain).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Update or append `CURSEFORGE_API_KEY=...` in `<project_root>/.env` atomically.
pub fn save_curseforge_api_key_to_dotenv(project_root: &Path, api_key: &str) -> Result<PathBuf> {
    let env_path = project_root.join(".env");
    let clean_key = api_key.trim();
    let existing = fs::read_to_string(&env_path).unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    let mut replaced = false;

    for line in existing.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("CURSEFORGE_API_KEY=") {
            lines.push(format!("CURSEFORGE_API_KEY={clean_key}"));
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !replaced {
        if !lines.is_empty() && !lines.last().map(|l| l.is_empty()).unwrap_or(true) {
            lines.push(String::new());
        }
        lines.push(format!("CURSEFORGE_API_KEY={clean_key}"));
    }

    let content = format!("{}\n", lines.join("\n"));
    atomic_write_text(&env_path, &content)
        .map_err(|e| ProviderError::Io(std::io::Error::other(e.to_string())))?;
    Ok(env_path)
}

fn parse_dotenv_key(content: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        if let Some(rest) = line.strip_prefix(&prefix) {
            let val = rest.trim().trim_matches('"').trim_matches('\'').trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_key_sha256_ctr_roundtrips_without_plaintext_leak() {
        let secret = "$2a$10$TestCurseForgeKeyExample9876543210";
        let salt = [0x5Au8; 32];
        // Encrypt using identical SHA-256 CTR XOR keystream
        let mut ciphertext = Vec::with_capacity(secret.len());
        for (block_idx, chunk) in secret.as_bytes().chunks(32).enumerate() {
            let mut hasher = Sha256::new();
            hasher.update(KEYSTREAM_DOMAIN);
            hasher.update(salt);
            hasher.update((block_idx as u32).to_le_bytes());
            let ks = hasher.finalize();
            for (b, k) in chunk.iter().zip(ks.iter()) {
                ciphertext.push(b ^ k);
            }
        }
        assert_ne!(ciphertext.as_slice(), secret.as_bytes());
        assert_eq!(
            decrypt_embedded_key(&salt, &ciphertext).as_deref(),
            Some(secret)
        );
        assert_eq!(decrypt_embedded_key(&salt, &[]), None);
    }

    #[test]
    fn parses_dotenv_key_ignoring_comments_and_quotes() {
        let content = "# comment\nCURSEFORGE_API_KEY=\"$2a$10$samplekey123\"\n";
        assert_eq!(
            parse_dotenv_key(content, "CURSEFORGE_API_KEY").as_deref(),
            Some("$2a$10$samplekey123")
        );
    }

    #[test]
    fn disabled_distribution_returns_manual_download_error() {
        let file = CurseForgeFile {
            id: 55555,
            mod_id: 12345,
            display_name: String::new(),
            file_name: "opt-out-mod.jar".to_string(),
            release_type: 1,
            file_length: 1024,
            download_url: None,
            game_versions: vec![],
            hashes: vec![],
            dependencies: vec![],
        };
        let err = file.require_download_url(Some("cool-mod")).unwrap_err();
        assert!(matches!(err, ProviderError::ManualDownloadRequired { .. }));
        assert!(err
            .to_string()
            .contains("https://www.curseforge.com/minecraft/mc-mods/cool-mod/download/55555"));

        let fallback = build_curseforge_countdown_url(None, 12345, 55555);
        assert_eq!(
            fallback,
            "https://www.curseforge.com/minecraft/search?search=12345"
        );
    }

    #[test]
    fn saves_loads_and_clears_custom_curseforge_api_key() {
        let dir = tempfile::tempdir().unwrap();
        let config_root = dir.path().join("config");
        assert!(load_custom_curseforge_api_key(&config_root).is_none());

        save_custom_curseforge_api_key(&config_root, "$2a$10$customKeyValue").unwrap();
        assert_eq!(
            load_custom_curseforge_api_key(&config_root).as_deref(),
            Some("$2a$10$customKeyValue")
        );

        save_custom_curseforge_api_key(&config_root, "").unwrap();
        assert!(load_custom_curseforge_api_key(&config_root).is_none());
    }

    #[test]
    fn deserializes_curseforge_authors_and_strips_html_description() {
        let json = r#"{
            "id": 245755,
            "name": "Waystones",
            "slug": "waystones",
            "summary": "Teleportation stones",
            "downloadCount": 120000000,
            "authors": [
                { "id": 1, "name": "BlayTheNinth", "url": "https://cf.example/author" },
                { "id": 2, "name": "CoAuthor", "url": "" }
            ],
            "screenshots": [
                { "thumbnailUrl": "https://img.example/thumb.png", "url": "https://img.example/full.png", "title": "Main" }
            ]
        }"#;
        let parsed: CurseForgeMod = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.primary_author(), "BlayTheNinth, CoAuthor");
        assert_eq!(parsed.screenshots.len(), 1);

        let preview = strip_html_to_preview(
            "<style>.x{color:red}</style><h1>About &amp; Features</h1><p>Fast <b>waystones</b>.<br/>Works&nbsp;great.</p><ul><li>Item 1</li><li>Item 2</li></ul>",
            500,
        );
        assert!(preview.contains("About & Features"));
        assert!(!preview.contains("color:red"));
        assert!(preview.contains("- Item 1"));
    }
}
