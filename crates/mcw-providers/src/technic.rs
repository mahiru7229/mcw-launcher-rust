use crate::cache::ApiCache;
use crate::modpack::{
    normalize_modloader, safe_join_under, suggest_short_instance_name, ModpackInstallReport,
    ModpackProgressCallback,
};
use crate::pack_registry::{
    safe_relative_posix, TechnicManagedFile, TechnicPackRecord, TechnicPackRegistry,
};
use crate::{ProviderError, Result};
use mcw_models::instance::ModLoader;
use mcw_net::{DownloadClient, DownloadRequest};
use mcw_storage::{InstanceStore, LauncherPaths};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;
use zip::ZipArchive;

static STAGING_SEQ: AtomicU64 = AtomicU64::new(1);

pub(crate) struct StagingDirGuard(PathBuf);

impl StagingDirGuard {
    pub(crate) fn new(prefix: &str) -> Result<Self> {
        let seq = STAGING_SEQ.fetch_add(1, Ordering::Relaxed);
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("mcw-{prefix}-{}-{ts}-{seq}", std::process::id()));
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for StagingDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const TECHNIC_API_BASE: &str = "https://api.technicpack.net";
const TECHNIC_TTL_SECS: u64 = 300;
const TECHNIC_BUILD_TAG: &str = "mcw";
const TECHNIC_CONCURRENCY: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicModpackSummary {
    pub slug: String,
    pub display_name: String,
    pub url: String,
    pub platform_url: String,
    pub icon_url: Option<String>,
    pub minecraft: String,
    pub version: String,
    pub solder_url: String,
    pub description: String,
    pub author: String,
    pub downloads: u64,
    pub runs: u64,
    pub ratings: u64,
}

impl TechnicModpackSummary {
    pub fn website_url(&self) -> String {
        if !self.platform_url.trim().is_empty() {
            self.platform_url.clone()
        } else {
            format!("https://www.technicpack.net/modpack/{}", self.slug)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicModpackDetail {
    pub slug: String,
    pub display_name: String,
    pub url: String,
    pub platform_url: String,
    pub icon_url: Option<String>,
    pub minecraft: String,
    pub version: String,
    pub solder_url: String,
    pub description: String,
    pub author: String,
    pub downloads: u64,
    pub runs: u64,
    pub ratings: u64,
}

impl From<TechnicModpackDetail> for TechnicModpackSummary {
    fn from(d: TechnicModpackDetail) -> Self {
        Self {
            slug: d.slug,
            display_name: d.display_name,
            url: d.url,
            platform_url: d.platform_url,
            icon_url: d.icon_url,
            minecraft: d.minecraft,
            version: d.version,
            solder_url: d.solder_url,
            description: d.description,
            author: d.author,
            downloads: d.downloads,
            runs: d.runs,
            ratings: d.ratings,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolderPackInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub recommended: Option<String>,
    #[serde(default)]
    pub latest: Option<String>,
    #[serde(default)]
    pub builds: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolderModEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub md5: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub filesize: u64,
}

#[derive(Debug, Clone)]
pub struct SolderBuildInfo {
    pub minecraft: String,
    pub forge: Option<String>,
    pub java: Option<String>,
    pub memory_mb: Option<u32>,
    pub mods: Vec<SolderModEntry>,
}

#[derive(Clone)]
pub struct TechnicClient {
    http: reqwest::Client,
    cache: Option<Arc<ApiCache>>,
}

impl TechnicClient {
    pub fn new(cache: Option<Arc<ApiCache>>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(mcw_models::user_agent())
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self { http, cache })
    }

    /// Extract a Technic modpack slug from either a raw slug or pasted Technic Platform/Website URL.
    pub fn extract_slug_from_input(input: &str) -> Option<String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Ok(parsed) = Url::parse(trimmed) {
            let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
            if host.ends_with("technicpack.net") {
                let mut segs = parsed.path_segments()?;
                while let Some(seg) = segs.next() {
                    if seg.eq_ignore_ascii_case("modpack") {
                        let raw_slug = segs.next()?.trim();
                        if raw_slug.is_empty() {
                            return None;
                        }
                        if host.starts_with("api.") {
                            return Some(raw_slug.to_string());
                        }
                        // Website URLs look like `/modpack/<slug>.<numeric_id>`
                        if let Some((slug_part, suffix)) = raw_slug.rsplit_once('.') {
                            if !slug_part.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
                                return Some(slug_part.to_string());
                            }
                        }
                        return Some(raw_slug.to_string());
                    }
                }
            }
        }
        None
    }

    pub async fn trending(&self) -> Result<Vec<TechnicModpackSummary>> {
        let url = format!("{TECHNIC_API_BASE}/trending?build={TECHNIC_BUILD_TAG}");
        let raw = self.cached_get(&url).await?;
        parse_technic_modpack_list(&raw)
    }

    pub async fn search(&self, query: &str) -> Result<Vec<TechnicModpackSummary>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return self.trending().await;
        }
        if let Some(slug) = Self::extract_slug_from_input(trimmed) {
            if let Ok(detail) = self.get_modpack(&slug).await {
                return Ok(vec![detail.into()]);
            }
        }
        let mut url = Url::parse(&format!("{TECHNIC_API_BASE}/search"))
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
        url.query_pairs_mut()
            .append_pair("q", trimmed)
            .append_pair("build", TECHNIC_BUILD_TAG);
        let raw = self.cached_get(url.as_str()).await?;
        let mut list = parse_technic_modpack_list(&raw)?;
        // Also try exact slug resolution when text query has no spaces and returned empty.
        if list.is_empty() && !trimmed.contains(' ') {
            if let Ok(detail) = self.get_modpack(&trimmed.to_ascii_lowercase()).await {
                list.push(detail.into());
            }
        }
        Ok(list)
    }

    pub async fn get_modpack(&self, slug: &str) -> Result<TechnicModpackDetail> {
        let clean = slug.trim();
        let url = format!("{TECHNIC_API_BASE}/modpack/{clean}?build={TECHNIC_BUILD_TAG}");
        let raw = self.cached_get(&url).await?;
        parse_technic_modpack_detail(&raw, clean)
    }

    pub async fn get_solder_pack(&self, solder_url: &str, slug: &str) -> Result<SolderPackInfo> {
        let endpoint = format!(
            "{}/modpack/{}",
            normalize_solder_base(solder_url),
            slug.trim()
        );
        let raw = self.cached_get(&endpoint).await?;
        parse_solder_pack_info(&raw)
    }

    pub async fn get_solder_build(
        &self,
        solder_url: &str,
        slug: &str,
        build: &str,
    ) -> Result<SolderBuildInfo> {
        let endpoint = format!(
            "{}/modpack/{}/{}",
            normalize_solder_base(solder_url),
            slug.trim(),
            build.trim()
        );
        let raw = self.cached_get(&endpoint).await?;
        parse_solder_build_info(&raw)
    }

    /// Download and install a Technic modpack (via Solder build or direct `.zip`) into a new instance.
    #[allow(clippy::too_many_arguments)]
    pub async fn install_pack_version_with_progress(
        &self,
        slug: &str,
        build_override: Option<&str>,
        instance_name_override: Option<&str>,
        paths: &LauncherPaths,
        downloader: &DownloadClient,
        progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackInstallReport> {
        let emit = |cur: u64, tot: u64, msg: String| {
            if let Some(ref cb) = progress {
                cb(cur, tot, msg);
            }
        };

        emit(0, 0, format!("Fetching Technic pack '{slug}' metadata..."));
        let detail = self.get_modpack(slug).await?;
        let display_name = if detail.display_name.trim().is_empty() {
            slug.to_string()
        } else {
            detail.display_name.clone()
        };

        let store = InstanceStore::new(paths);
        let raw_name = instance_name_override
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&display_name);
        let short_name = suggest_short_instance_name(raw_name, 36);
        let candidate_name = store
            .resolve_unique_instance_name(&short_name)
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;

        let has_solder = !detail.solder_url.trim().is_empty();
        if has_solder {
            let solder_info = self.get_solder_pack(&detail.solder_url, slug).await?;
            let chosen_build = build_override
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .or_else(|| solder_info.recommended.clone())
                .or_else(|| solder_info.latest.clone())
                .or_else(|| solder_info.builds.last().cloned())
                .unwrap_or_else(|| detail.version.clone());
            if chosen_build.trim().is_empty() {
                return Err(ProviderError::Io(io::Error::other(format!(
                    "Technic Solder pack '{slug}' returned no builds."
                ))));
            }

            emit(
                0,
                0,
                format!("Fetching Solder build '{chosen_build}' for '{slug}'..."),
            );
            let build = self
                .get_solder_build(&detail.solder_url, slug, &chosen_build)
                .await?;
            let game_version = if !build.minecraft.trim().is_empty() {
                build.minecraft.clone()
            } else if !detail.minecraft.trim().is_empty() {
                detail.minecraft.clone()
            } else {
                "1.12.2".to_string()
            };
            let initial_loader = match build.forge.as_deref().map(str::trim) {
                Some(f) if !f.is_empty() => normalize_modloader("forge", f, &game_version),
                _ => ModLoader::vanilla(),
            };

            let (inst, mut inst_settings) = store
                .create_instance(&candidate_name, &game_version, initial_loader.clone())
                .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
            if let Some(mem) = build.memory_mb {
                if mem >= 512 {
                    inst_settings.max_memory = mem;
                }
            }
            inst_settings.disable_mod_repair_and_updates = Some(true);
            inst_settings.disable_launch_checking = Some(true);
            let _ = store.save_settings(&inst.instance_id, &inst_settings);

            let mc_dir = paths.minecraft_dir(&inst.instance_id);
            fs::create_dir_all(&mc_dir)?;

            let (managed_files, extracted_count) =
                install_solder_mods(&build.mods, &mc_dir, downloader, progress.clone(), &[])
                    .await?;

            let mut final_loader = initial_loader;
            if final_loader.is_vanilla() {
                if let Some(detected) = detect_loader_from_modpack_jar(&mc_dir, &game_version) {
                    final_loader = detected.clone();
                    let _ = store.set_runtime_profile(&inst.instance_id, &game_version, detected);
                }
            }

            let _ = TechnicPackRegistry::save(
                &paths.instance_dir(&inst.instance_id),
                &TechnicPackRecord {
                    schema_version: 1,
                    source: "technic".to_string(),
                    slug: detail.slug.clone(),
                    pack_name: display_name,
                    solder_url: detail.solder_url.clone(),
                    version: chosen_build,
                    minecraft: game_version.clone(),
                    managed_files,
                    updated_at: String::new(),
                },
            );

            Ok(ModpackInstallReport {
                instance_id: inst.instance_id,
                game_version,
                loader: final_loader,
                downloaded_files: build.mods.len(),
                extracted_overrides: extracted_count,
                manual_downloads: Vec::new(),
            })
        } else {
            let download_url = detail.url.trim().to_string();
            if download_url.is_empty() {
                return Err(ProviderError::Io(io::Error::other(format!(
                    "Technic modpack '{slug}' has neither a Solder URL nor a direct download URL."
                ))));
            }
            let game_version = if detail.minecraft.trim().is_empty() {
                "1.12.2".to_string()
            } else {
                detail.minecraft.clone()
            };
            let version_label = build_override
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(&detail.version)
                .to_string();

            let (inst, mut inst_settings) = store
                .create_instance(&candidate_name, &game_version, ModLoader::vanilla())
                .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
            inst_settings.disable_mod_repair_and_updates = Some(true);
            inst_settings.disable_launch_checking = Some(true);
            let _ = store.save_settings(&inst.instance_id, &inst_settings);

            let mc_dir = paths.minecraft_dir(&inst.instance_id);
            fs::create_dir_all(&mc_dir)?;

            let staging = StagingDirGuard::new("technic-dl")?;
            let archive_path = staging.path().join(format!("{slug}.zip"));
            let prog_dl = progress.clone();
            let req = DownloadRequest::new(&download_url, &archive_path);
            downloader
                .download_file_with_progress(&req, move |done, tot| {
                    if let Some(ref cb) = prog_dl {
                        let mb_done = done as f64 / 1_048_576.0;
                        let msg = if tot > 0 {
                            format!(
                                "Downloading Technic archive ({mb_done:.1} / {:.1} MB)",
                                tot as f64 / 1_048_576.0
                            )
                        } else {
                            format!("Downloading Technic archive ({mb_done:.1} MB)")
                        };
                        cb(done, tot.max(done), msg);
                    }
                })
                .await?;

            emit(
                0,
                1,
                format!("Extracting Technic archive for '{display_name}'..."),
            );
            let extracted_paths = extract_zip_safe(&archive_path, &mc_dir, &[])?;
            let final_loader = detect_loader_from_modpack_jar(&mc_dir, &game_version)
                .unwrap_or_else(ModLoader::vanilla);
            if !final_loader.is_vanilla() {
                let _ = store.set_runtime_profile(
                    &inst.instance_id,
                    &game_version,
                    final_loader.clone(),
                );
            }

            let managed_files: Vec<TechnicManagedFile> = extracted_paths
                .into_iter()
                .map(|rel| TechnicManagedFile {
                    name: Path::new(&rel)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("file")
                        .to_string(),
                    version: version_label.clone(),
                    path: safe_relative_posix(&rel, "file"),
                    md5: String::new(),
                    url: String::new(),
                    provider: "technic".to_string(),
                })
                .collect();

            let _ = TechnicPackRegistry::save(
                &paths.instance_dir(&inst.instance_id),
                &TechnicPackRecord {
                    schema_version: 1,
                    source: "technic".to_string(),
                    slug: detail.slug,
                    pack_name: display_name,
                    solder_url: String::new(),
                    version: version_label,
                    minecraft: game_version.clone(),
                    managed_files,
                    updated_at: String::new(),
                },
            );

            Ok(ModpackInstallReport {
                instance_id: inst.instance_id,
                game_version,
                loader: final_loader,
                downloaded_files: 1,
                extracted_overrides: 1,
                manual_downloads: Vec::new(),
            })
        }
    }

    pub(crate) async fn cached_get(&self, url: &str) -> Result<String> {
        let key = format!("technic:{url}");
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
            let _ = cache.set(&key, &text, TECHNIC_TTL_SECS);
        }
        Ok(text)
    }
}

/// Download Solder mod zips, verify optional MD5, extract safely into `mc_dir`, and track managed paths.
pub(crate) async fn install_solder_mods(
    mods: &[SolderModEntry],
    mc_dir: &Path,
    downloader: &DownloadClient,
    progress: Option<ModpackProgressCallback>,
    skip_prefixes: &[&str],
) -> Result<(Vec<TechnicManagedFile>, usize)> {
    let staging = StagingDirGuard::new("technic-solder")?;
    let mut requests = Vec::new();
    let mut staged: Vec<(&SolderModEntry, PathBuf)> = Vec::new();

    for (idx, m) in mods.iter().enumerate() {
        let url = m.url.trim();
        if url.is_empty() {
            continue;
        }
        let safe_name = format!(
            "{idx:04}-{}.zip",
            m.name
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                })
                .collect::<String>()
        );
        let dest = staging.path().join(safe_name);
        requests.push(DownloadRequest::new(url, &dest));
        staged.push((m, dest));
    }

    let total = requests.len() as u64;
    if let Some(ref cb) = progress {
        cb(
            0,
            total,
            format!("Downloading {total} Technic mod packages..."),
        );
    }
    let prog_cb = progress.clone();
    downloader
        .download_batch(requests, TECHNIC_CONCURRENCY, move |done, tot| {
            if let Some(ref cb) = prog_cb {
                cb(
                    done,
                    tot,
                    format!("Downloading Technic packages ({done}/{tot})..."),
                );
            }
        })
        .await?;

    let mut seen_paths = HashSet::new();
    let mut managed_files = Vec::new();
    let mut extracted_count = 0usize;

    for (idx, (m, zip_path)) in staged.iter().enumerate() {
        if let Some(ref cb) = progress {
            cb(
                idx as u64 + 1,
                staged.len() as u64,
                format!("Verifying & extracting {}...", m.name),
            );
        }
        let expected_md5 = m.md5.trim().to_ascii_lowercase();
        if !expected_md5.is_empty() {
            let actual = compute_file_md5(zip_path)?;
            if actual != expected_md5 {
                return Err(ProviderError::Io(io::Error::other(format!(
                    "MD5 checksum mismatch for Technic mod '{}': expected {expected_md5}, got {actual}",
                    m.name
                ))));
            }
        }
        let rels = extract_zip_safe(zip_path, mc_dir, skip_prefixes)?;
        extracted_count += rels.len();
        for rel in rels {
            let norm = safe_relative_posix(&rel, "mod.bin");
            if seen_paths.insert(norm.to_ascii_lowercase()) {
                managed_files.push(TechnicManagedFile {
                    name: m.name.clone(),
                    version: m.version.clone(),
                    path: norm,
                    md5: expected_md5.clone(),
                    url: m.url.clone(),
                    provider: "technic".to_string(),
                });
            }
        }
    }

    Ok((managed_files, extracted_count))
}

pub(crate) fn compute_file_md5(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Md5::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    Ok(digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>())
}

/// Safely extract a `.zip` archive into `dest_root`, rejecting path traversal and preserving `skip_prefixes`.
pub(crate) fn extract_zip_safe(
    archive_path: &Path,
    dest_root: &Path,
    skip_prefixes: &[&str],
) -> Result<Vec<String>> {
    let file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(file)
        .map_err(|e| ProviderError::Io(io::Error::other(format!("Invalid ZIP archive: {e}"))))?;
    let mut extracted = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
        let raw_name = entry.name().replace('\\', "/");
        let clean_rel = raw_name.trim_start_matches('/').to_string();
        if clean_rel.is_empty() || entry.is_dir() || clean_rel.ends_with('/') {
            continue;
        }
        let lower_rel = clean_rel.to_ascii_lowercase();
        if skip_prefixes
            .iter()
            .any(|pre| lower_rel == pre.trim_end_matches('/') || lower_rel.starts_with(pre))
        {
            continue;
        }
        let dest = safe_join_under(dest_root, &clean_rel)?;
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&dest)?;
        io::copy(&mut entry, &mut out)?;
        extracted.push(clean_rel);
    }
    Ok(extracted)
}

/// Inspect `.minecraft/bin/modpack.jar` (used by legacy/modern Technic packs) to detect Forge/Fabric loader version.
pub fn detect_loader_from_modpack_jar(mc_dir: &Path, fallback_mc: &str) -> Option<ModLoader> {
    let modpack_jar = mc_dir.join("bin").join("modpack.jar");
    if !modpack_jar.is_file() {
        return None;
    }
    let file = File::open(&modpack_jar).ok()?;
    let mut archive = ZipArchive::new(file).ok()?;

    // 1. Check modern Forge/NeoForge/Fabric `version.json` embedded inside `modpack.jar`
    if let Ok(mut entry) = archive.by_name("version.json") {
        let mut raw = String::new();
        if entry.read_to_string(&mut raw).is_ok() {
            if let Ok(json) = serde_json::from_str::<Value>(&raw) {
                if let Some(id) = json.get("id").and_then(|v| v.as_str()) {
                    let low = id.to_ascii_lowercase();
                    if low.contains("neoforge") {
                        let ver = id.split('-').next_back().unwrap_or("").trim().to_string();
                        if !ver.is_empty() {
                            return Some(normalize_modloader("neoforge", &ver, fallback_mc));
                        }
                    } else if low.contains("forge") {
                        let ver = id
                            .split("forge-")
                            .nth(1)
                            .unwrap_or_else(|| id.split('-').next_back().unwrap_or(""))
                            .trim()
                            .to_string();
                        if !ver.is_empty() {
                            return Some(normalize_modloader("forge", &ver, fallback_mc));
                        }
                    } else if low.contains("fabric") {
                        let ver = id.split('-').nth(2).unwrap_or("").trim().to_string();
                        if !ver.is_empty() {
                            return Some(normalize_modloader("fabric", &ver, fallback_mc));
                        }
                    }
                }
                if let Some(libs) = json.get("libraries").and_then(|v| v.as_array()) {
                    for lib in libs {
                        let name = lib.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        if let Some(rest) = name.strip_prefix("net.minecraftforge:forge:") {
                            let ver = rest.split('-').nth(1).unwrap_or(rest).trim();
                            if !ver.is_empty() {
                                return Some(normalize_modloader("forge", ver, fallback_mc));
                            }
                        } else if let Some(rest) = name.strip_prefix("net.neoforged:neoforge:") {
                            return Some(normalize_modloader("neoforge", rest.trim(), fallback_mc));
                        } else if let Some(rest) = name.strip_prefix("net.fabricmc:fabric-loader:")
                        {
                            return Some(normalize_modloader("fabric", rest.trim(), fallback_mc));
                        }
                    }
                }
            }
        }
    }

    // 2. Check Forge FML properties (`fmlversion.properties`) common in 1.7.10 - 1.12.2 modpack.jar
    if let Ok(mut entry) = archive.by_name("fmlversion.properties") {
        let mut props = String::new();
        if entry.read_to_string(&mut props).is_ok() {
            let mut major = None;
            let mut minor = None;
            let mut rev = None;
            let mut build = None;
            for line in props.lines() {
                let Some((k, v)) = line.split_once('=') else {
                    continue;
                };
                match k.trim() {
                    "fmlbuild.major.number" => major = Some(v.trim().to_string()),
                    "fmlbuild.minor.number" => minor = Some(v.trim().to_string()),
                    "fmlbuild.revision.number" => rev = Some(v.trim().to_string()),
                    "fmlbuild.build.number" => build = Some(v.trim().to_string()),
                    _ => {}
                }
            }
            if let (Some(maj), Some(min), Some(r), Some(b)) = (major, minor, rev, build) {
                let forge_ver = format!("{maj}.{min}.{r}.{b}");
                return Some(normalize_modloader("forge", &forge_ver, fallback_mc));
            }
        }
    }

    None
}

fn normalize_solder_base(solder_url: &str) -> String {
    let trimmed = solder_url.trim().trim_end_matches('/');
    if trimmed.ends_with("/api") || trimmed.contains("/api/") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/api")
    }
}

fn val_str(v: &Value, keys: &[&str]) -> String {
    for k in keys {
        if let Some(s) = v.get(*k).and_then(|x| x.as_str()) {
            if !s.trim().is_empty() {
                return s.trim().to_string();
            }
        }
    }
    String::new()
}

fn val_u64(v: &Value, keys: &[&str]) -> u64 {
    for k in keys {
        if let Some(x) = v.get(*k) {
            if let Some(n) = x.as_u64() {
                return n;
            }
            if let Some(s) = x.as_str() {
                if let Ok(n) = s.trim().parse::<u64>() {
                    return n;
                }
            }
        }
    }
    0
}

fn extract_icon_url(v: &Value) -> Option<String> {
    for key in ["iconUrl", "logoUrl"] {
        if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
            if !s.trim().is_empty() {
                return Some(s.trim().to_string());
            }
        }
    }
    for obj_key in ["icon", "logo", "background"] {
        if let Some(obj) = v.get(obj_key) {
            if let Some(s) = obj.as_str() {
                if !s.trim().is_empty() {
                    return Some(s.trim().to_string());
                }
            }
            if let Some(s) = obj.get("url").and_then(|u| u.as_str()) {
                if !s.trim().is_empty() {
                    return Some(s.trim().to_string());
                }
            }
        }
    }
    None
}

fn check_technic_error(v: &Value) -> Result<()> {
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        if !err.trim().is_empty() {
            return Err(ProviderError::Io(io::Error::other(format!(
                "Technic API error: {err}"
            ))));
        }
    }
    Ok(())
}

pub(crate) fn parse_technic_modpack_list(raw: &str) -> Result<Vec<TechnicModpackSummary>> {
    let val: Value = serde_json::from_str(raw)?;
    check_technic_error(&val)?;
    let arr = val
        .get("modpacks")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let slug = val_str(&item, &["slug", "name"]);
        if slug.is_empty() {
            continue;
        }
        let display_name = {
            let d = val_str(&item, &["displayName", "name", "slug"]);
            if d.is_empty() {
                slug.clone()
            } else {
                d
            }
        };
        out.push(TechnicModpackSummary {
            slug,
            display_name,
            url: val_str(&item, &["url"]),
            platform_url: val_str(&item, &["platformUrl"]),
            icon_url: extract_icon_url(&item),
            minecraft: val_str(&item, &["minecraft"]),
            version: val_str(&item, &["version"]),
            solder_url: val_str(&item, &["solder"]),
            description: val_str(&item, &["description"]),
            author: val_str(&item, &["user", "author"]),
            downloads: val_u64(&item, &["downloads"]),
            runs: val_u64(&item, &["runs"]),
            ratings: val_u64(&item, &["ratings", "rating"]),
        });
    }
    Ok(out)
}

pub(crate) fn parse_technic_modpack_detail(
    raw: &str,
    fallback_slug: &str,
) -> Result<TechnicModpackDetail> {
    let val: Value = serde_json::from_str(raw)?;
    check_technic_error(&val)?;
    let slug = {
        let s = val_str(&val, &["name", "slug"]);
        if s.is_empty() {
            fallback_slug.to_string()
        } else {
            s
        }
    };
    let display_name = {
        let d = val_str(&val, &["displayName", "name", "slug"]);
        if d.is_empty() {
            slug.clone()
        } else {
            d
        }
    };
    Ok(TechnicModpackDetail {
        slug,
        display_name,
        url: val_str(&val, &["url"]),
        platform_url: val_str(&val, &["platformUrl"]),
        icon_url: extract_icon_url(&val),
        minecraft: val_str(&val, &["minecraft"]),
        version: val_str(&val, &["version"]),
        solder_url: val_str(&val, &["solder"]),
        description: val_str(&val, &["description"]),
        author: val_str(&val, &["user", "author"]),
        downloads: val_u64(&val, &["downloads"]),
        runs: val_u64(&val, &["runs"]),
        ratings: val_u64(&val, &["ratings", "rating"]),
    })
}

pub(crate) fn parse_solder_pack_info(raw: &str) -> Result<SolderPackInfo> {
    let val: Value = serde_json::from_str(raw)?;
    check_technic_error(&val)?;
    Ok(serde_json::from_value(val)?)
}

pub(crate) fn parse_solder_build_info(raw: &str) -> Result<SolderBuildInfo> {
    let val: Value = serde_json::from_str(raw)?;
    check_technic_error(&val)?;
    let minecraft = val_str(&val, &["minecraft"]);
    let forge = val
        .get("forge")
        .and_then(|f| f.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let java = val
        .get("java")
        .and_then(|j| j.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let memory_mb = val.get("memory").and_then(|m| {
        m.as_u64()
            .map(|n| n as u32)
            .or_else(|| m.as_str().and_then(|s| s.trim().parse::<u32>().ok()))
    });
    let mods = val
        .get("mods")
        .and_then(|v| serde_json::from_value::<Vec<SolderModEntry>>(v.clone()).ok())
        .unwrap_or_default();
    Ok(SolderBuildInfo {
        minecraft,
        forge,
        java,
        memory_mb,
        mods,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    #[test]
    fn extracts_slug_from_technic_urls() {
        assert_eq!(
            TechnicClient::extract_slug_from_input(
                "https://api.technicpack.net/modpack/hexxit-ii?build=mcw"
            ),
            Some("hexxit-ii".to_string())
        );
        assert_eq!(
            TechnicClient::extract_slug_from_input(
                "https://www.technicpack.net/modpack/tekkit-classic.552"
            ),
            Some("tekkit-classic".to_string())
        );
        assert_eq!(TechnicClient::extract_slug_from_input("hexxit"), None);
    }

    #[test]
    fn parses_technic_platform_and_solder_json() {
        let search_raw = r#"{
            "modpacks": [
                {
                    "slug": "tekkitmain",
                    "name": "Tekkit Main",
                    "iconUrl": "https://cdn.technicpack.net/icon.png",
                    "runs": 500000,
                    "downloads": 900000
                }
            ]
        }"#;
        let list = parse_technic_modpack_list(search_raw).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].slug, "tekkitmain");
        assert_eq!(list[0].display_name, "Tekkit Main");
        assert_eq!(list[0].downloads, 900_000);

        let detail_raw = r#"{
            "name": "tekkitmain",
            "displayName": "Tekkit",
            "solder": "https://solder.technicpack.net/api/",
            "minecraft": "1.6.4",
            "version": "1.2.9g",
            "icon": { "url": "https://cdn.technicpack.net/icon2.png" }
        }"#;
        let detail = parse_technic_modpack_detail(detail_raw, "tekkitmain").unwrap();
        assert_eq!(detail.display_name, "Tekkit");
        assert_eq!(detail.solder_url, "https://solder.technicpack.net/api/");
        assert_eq!(
            detail.icon_url.as_deref(),
            Some("https://cdn.technicpack.net/icon2.png")
        );

        let build_raw = r#"{
            "minecraft": "1.12.2",
            "forge": "14.23.5.2855",
            "memory": "4096",
            "mods": [
                {
                    "name": "jei",
                    "version": "4.16.1",
                    "md5": "d41d8cd98f00b204e9800998ecf8427e",
                    "url": "https://repo.example/jei.zip"
                }
            ]
        }"#;
        let build = parse_solder_build_info(build_raw).unwrap();
        assert_eq!(build.minecraft, "1.12.2");
        assert_eq!(build.forge.as_deref(), Some("14.23.5.2855"));
        assert_eq!(build.memory_mb, Some(4096));
        assert_eq!(build.mods.len(), 1);
        assert_eq!(build.mods[0].name, "jei");
    }

    #[test]
    fn detects_loader_from_modpack_jar_and_verifies_md5_and_zip_slip() {
        let dir = tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let jar_path = bin_dir.join("modpack.jar");
        {
            let mut zip = ZipWriter::new(File::create(&jar_path).unwrap());
            zip.start_file("version.json", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(br#"{"id":"1.12.2-forge-14.23.5.2860"}"#)
                .unwrap();
            zip.finish().unwrap();
        }
        let detected = detect_loader_from_modpack_jar(dir.path(), "1.12.2").unwrap();
        assert_eq!(detected.name, "forge");
        assert_eq!(detected.version, "14.23.5.2860");

        let empty_file = dir.path().join("empty.bin");
        fs::write(&empty_file, b"").unwrap();
        assert_eq!(
            compute_file_md5(&empty_file).unwrap(),
            "d41d8cd98f00b204e9800998ecf8427e"
        );

        let bad_zip = dir.path().join("evil.zip");
        {
            let mut zip = ZipWriter::new(File::create(&bad_zip).unwrap());
            zip.start_file("../outside.txt", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"evil").unwrap();
            zip.finish().unwrap();
        }
        assert!(extract_zip_safe(&bad_zip, &dir.path().join("out"), &[]).is_err());
    }
}
