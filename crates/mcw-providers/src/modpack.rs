use crate::curseforge::CurseForgeClient;
use crate::{ProviderError, Result};
use mcw_models::instance::ModLoader;
use mcw_net::{DownloadClient, DownloadOutcome, DownloadRequest};
use mcw_storage::{atomic_write_json, ContentStore, InstanceStore, LauncherPaths};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use zip::ZipArchive;

fn adopt_downloaded_mods(paths: &LauncherPaths, outcomes: &[DownloadOutcome]) {
    for out in outcomes {
        if !out.skipped
            && fs::metadata(&out.destination)
                .map(|m| m.len() >= 64 * 1024)
                .unwrap_or(false)
        {
            let _ = ContentStore::adopt(paths, &out.destination);
        }
    }
}

pub type ModpackProgressCallback = Arc<dyn Fn(u64, u64, String) + Send + Sync>;

const DOWNLOAD_CONCURRENCY: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualDownloadItem {
    pub mod_id: u64,
    pub file_id: u64,
    pub file_name: String,
    pub website_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModpackInstallReport {
    pub instance_id: String,
    pub game_version: String,
    pub loader: ModLoader,
    pub downloaded_files: usize,
    pub extracted_overrides: usize,
    pub manual_downloads: Vec<ManualDownloadItem>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MrpackIndex {
    pub(crate) name: String,
    #[serde(rename = "versionId", default)]
    pub(crate) version_id: String,
    pub(crate) dependencies: HashMap<String, String>,
    #[serde(default)]
    pub(crate) files: Vec<MrpackFile>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MrpackFile {
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) hashes: HashMap<String, String>,
    #[serde(default)]
    pub(crate) downloads: Vec<String>,
    #[serde(rename = "fileSize", default)]
    pub(crate) file_size: u64,
    #[serde(default)]
    pub(crate) env: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CurseForgeManifest {
    pub(crate) name: String,
    pub(crate) minecraft: CfMinecraftSpec,
    #[serde(default)]
    pub(crate) overrides: Option<String>,
    #[serde(default)]
    pub(crate) files: Vec<CfManifestFile>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CfMinecraftSpec {
    pub(crate) version: String,
    #[serde(rename = "modLoaders", default)]
    pub(crate) mod_loaders: Vec<CfModLoader>,
    #[serde(rename = "modLoader", default)]
    pub(crate) mod_loader: Option<CfModLoader>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CfModLoader {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModpackOptionalFile {
    pub id: String,
    pub name: String,
    pub path: String,
    pub default_selected: bool,
}

#[derive(Debug, Clone)]
pub struct ModpackInstallOptions {
    pub include_optional: bool,
    pub selected_optional_paths: Option<HashSet<String>>,
}

impl Default for ModpackInstallOptions {
    fn default() -> Self {
        Self {
            include_optional: true,
            selected_optional_paths: None,
        }
    }
}

impl ModpackInstallOptions {
    pub fn should_include(&self, key: &str) -> bool {
        if !self.include_optional {
            return false;
        }
        match self.selected_optional_paths {
            Some(ref set) => set.contains(key),
            None => true,
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct CfManifestFile {
    #[serde(rename = "projectID")]
    pub(crate) project_id: u64,
    #[serde(rename = "fileID")]
    pub(crate) file_id: u64,
    #[serde(default = "default_true")]
    pub(crate) required: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModpackExportMode {
    /// Modrinth `.mrpack` format (`modrinth.index.json` + `overrides/`).
    Mrpack,
    /// Lightweight provider profile `.zip` (`manifest.json` + `mcw_profile.json` + `overrides/config`).
    ProviderProfile,
    /// MCW Portable package `.mcwpack` (`mcw_package.json` + instance settings + mods/configs/packs).
    Portable { full: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModpackExportOptions {
    pub mode: ModpackExportMode,
    pub include_saves: bool,
    pub disable_mod_repair_and_updates: bool,
}

impl Default for ModpackExportOptions {
    fn default() -> Self {
        Self {
            mode: ModpackExportMode::Mrpack,
            include_saves: false,
            disable_mod_repair_and_updates: true,
        }
    }
}

pub struct ModpackInstaller;

impl ModpackInstaller {
    /// Inspects a local `.mrpack` or CurseForge `.zip` archive and returns any optional files
    /// that the user can toggle before installation.
    pub fn inspect_optional_files(archive_path: &Path) -> Result<Vec<ModpackOptionalFile>> {
        let file = File::open(archive_path)?;
        let mut archive = ZipArchive::new(file)
            .map_err(|e| ProviderError::Io(io::Error::other(format!("Invalid archive: {e}"))))?;

        if let Ok(entry) = archive.by_name("modrinth.index.json") {
            let index: MrpackIndex = serde_json::from_reader(entry)?;
            let mut out = Vec::new();
            for f in index.files {
                let is_optional = f
                    .env
                    .as_ref()
                    .and_then(|e| e.get("client"))
                    .is_some_and(|v| v.eq_ignore_ascii_case("optional"));
                if is_optional {
                    let name = Path::new(&f.path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&f.path)
                        .to_string();
                    out.push(ModpackOptionalFile {
                        id: f.path.clone(),
                        name,
                        path: f.path,
                        default_selected: true,
                    });
                }
            }
            return Ok(out);
        }

        if let Ok(entry) = archive.by_name("manifest.json") {
            let manifest: CurseForgeManifest = serde_json::from_reader(entry)?;
            let mut out = Vec::new();
            for f in manifest.files {
                if !f.required {
                    let key = format!("{}:{}", f.project_id, f.file_id);
                    out.push(ModpackOptionalFile {
                        id: key.clone(),
                        name: format!("CurseForge Project #{} (File #{})", f.project_id, f.file_id),
                        path: key,
                        default_selected: true,
                    });
                }
            }
            return Ok(out);
        }

        Ok(Vec::new())
    }

    /// Auto-detect and install either a Modrinth `.mrpack` or CurseForge `.zip` modpack.
    pub async fn install_from_archive(
        paths: &LauncherPaths,
        archive_path: &Path,
        instance_name_override: Option<&str>,
        downloader: &DownloadClient,
        curseforge: Option<&CurseForgeClient>,
    ) -> Result<ModpackInstallReport> {
        Self::install_from_archive_with_options(
            paths,
            archive_path,
            instance_name_override,
            downloader,
            curseforge,
            &ModpackInstallOptions::default(),
            None,
        )
        .await
    }

    pub async fn install_from_archive_with_progress(
        paths: &LauncherPaths,
        archive_path: &Path,
        instance_name_override: Option<&str>,
        downloader: &DownloadClient,
        curseforge: Option<&CurseForgeClient>,
        on_progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackInstallReport> {
        Self::install_from_archive_with_options(
            paths,
            archive_path,
            instance_name_override,
            downloader,
            curseforge,
            &ModpackInstallOptions::default(),
            on_progress,
        )
        .await
    }

    pub async fn install_from_archive_with_options(
        paths: &LauncherPaths,
        archive_path: &Path,
        instance_name_override: Option<&str>,
        downloader: &DownloadClient,
        curseforge: Option<&CurseForgeClient>,
        options: &ModpackInstallOptions,
        on_progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackInstallReport> {
        let file = File::open(archive_path)?;
        let mut archive = ZipArchive::new(file)
            .map_err(|e| ProviderError::Io(io::Error::other(format!("Invalid archive: {e}"))))?;

        if archive.by_name("mcw_package.json").is_ok() {
            drop(archive);
            return Self::install_mcwpack(paths, archive_path, instance_name_override, on_progress);
        }
        if archive.by_name("modrinth.index.json").is_ok() {
            drop(archive);
            return Self::install_mrpack(
                paths,
                archive_path,
                instance_name_override,
                downloader,
                options,
                on_progress,
            )
            .await;
        }
        if archive.by_name("manifest.json").is_ok() {
            drop(archive);
            return Self::install_curseforge_zip(
                paths,
                archive_path,
                instance_name_override,
                downloader,
                curseforge,
                options,
                on_progress,
            )
            .await;
        }

        Err(ProviderError::Io(io::Error::other(
            "Unsupported modpack archive: missing mcw_package.json, modrinth.index.json, or manifest.json",
        )))
    }

    fn install_mcwpack(
        paths: &LauncherPaths,
        archive_path: &Path,
        name_override: Option<&str>,
        on_progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackInstallReport> {
        let pkg_val: serde_json::Value = {
            let file = File::open(archive_path)?;
            let mut archive = ZipArchive::new(file)
                .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
            let entry = archive
                .by_name("mcw_package.json")
                .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
            serde_json::from_reader(entry)?
        };

        let inst_meta: mcw_models::instance::Instance = pkg_val
            .get("instance")
            .cloned()
            .ok_or_else(|| {
                ProviderError::Io(io::Error::other("mcw_package.json is missing 'instance'"))
            })
            .and_then(|v| serde_json::from_value(v).map_err(ProviderError::from))?;
        let pkg_settings: Option<mcw_models::instance::InstanceSettings> = pkg_val
            .get("settings")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok());

        let base_name = name_override
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&inst_meta.name);
        let store = InstanceStore::new(paths);
        let target_name = store
            .resolve_unique_instance_name(base_name)
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;

        let disable_flag = pkg_val
            .get("disable_mod_repair_and_updates")
            .and_then(|v| v.as_bool())
            .or_else(|| {
                pkg_settings
                    .as_ref()
                    .and_then(|s| s.disable_mod_repair_and_updates)
            })
            .unwrap_or(true);

        let loader = normalize_modloader(
            &inst_meta.mod_loader.name,
            &inst_meta.mod_loader.version,
            &inst_meta.version_id,
        );
        let (inst, mut inst_settings) = store
            .create_instance(&target_name, &inst_meta.version_id, loader.clone())
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
        if let Some(mut settings) = pkg_settings {
            settings.disable_mod_repair_and_updates = Some(disable_flag);
            let _ = store.save_settings(&inst.instance_id, &settings);
        } else {
            inst_settings.disable_mod_repair_and_updates = Some(disable_flag);
            let _ = store.save_settings(&inst.instance_id, &inst_settings);
        }

        let _ = atomic_write_json(
            &paths
                .instance_dir(&inst.instance_id)
                .join(".mcw")
                .join("mcwpack.json"),
            &serde_json::json!({
                "source": "mcwpack",
                "disable_mod_repair_and_updates": disable_flag,
            }),
        );

        let mc_dir = paths.minecraft_dir(&inst.instance_id);
        if let Some(ref cb) = on_progress {
            cb(0, 1, "Extracting .mcwpack portable package...".to_string());
        }
        let extracted_overrides =
            extract_overrides(archive_path, &["overrides/", ".minecraft/"], &mc_dir)?;
        extract_embedded_instance_icon(archive_path, &store, &inst.instance_id);

        Ok(ModpackInstallReport {
            instance_id: inst.instance_id,
            game_version: inst_meta.version_id,
            loader,
            downloaded_files: 0,
            extracted_overrides,
            manual_downloads: Vec::new(),
        })
    }

    async fn install_mrpack(
        paths: &LauncherPaths,
        archive_path: &Path,
        name_override: Option<&str>,
        downloader: &DownloadClient,
        options: &ModpackInstallOptions,
        on_progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackInstallReport> {
        let index: MrpackIndex = {
            let file = File::open(archive_path)?;
            let mut archive = ZipArchive::new(file)
                .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
            let entry = archive
                .by_name("modrinth.index.json")
                .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
            serde_json::from_reader(entry)?
        };

        let game_version = index
            .dependencies
            .get("minecraft")
            .cloned()
            .unwrap_or_else(|| "1.21.4".to_string());
        let loader = parse_mrpack_loader(&index.dependencies, &game_version);
        let base_name = name_override
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(index.name.trim());

        let store = InstanceStore::new(paths);
        let target_name = store
            .resolve_unique_instance_name(base_name)
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
        let (inst, mut inst_settings) = store
            .create_instance(&target_name, &game_version, loader.clone())
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
        inst_settings.disable_mod_repair_and_updates = Some(true);
        let _ = store.save_settings(&inst.instance_id, &inst_settings);
        let mc_dir = paths.minecraft_dir(&inst.instance_id);

        let mut requests = Vec::new();
        let mut managed_files = Vec::new();
        let mut hardlinked_files = 0usize;
        for f in &index.files {
            if let Some(ref env) = f.env {
                if env
                    .get("client")
                    .map(|v| v.eq_ignore_ascii_case("unsupported"))
                    .unwrap_or(false)
                {
                    continue;
                }
                if env
                    .get("client")
                    .map(|v| v.eq_ignore_ascii_case("optional"))
                    .unwrap_or(false)
                    && !options.should_include(&f.path)
                {
                    continue;
                }
            }
            managed_files.push(f.path.replace('\\', "/"));
            let dest = safe_join_under(&mc_dir, &f.path)?;
            if let Some(sha256) = f.hashes.get("sha256") {
                if ContentStore::try_hardlink_from_store(paths, sha256, &dest).is_some() {
                    hardlinked_files += 1;
                    continue;
                }
            }
            let Some(primary_url) = f.downloads.first() else {
                continue;
            };
            let mut req = DownloadRequest::new(primary_url, dest)
                .with_sha1(f.hashes.get("sha1").cloned())
                .with_size(f.file_size);
            if f.downloads.len() > 1 {
                req.mirrors = f.downloads[1..].to_vec();
            }
            requests.push(req);
        }

        let downloaded_files = requests.len() + hardlinked_files;
        let prog_cb = on_progress.clone();
        let outcomes = downloader
            .download_batch(requests, DOWNLOAD_CONCURRENCY, move |done, tot| {
                if let Some(ref cb) = prog_cb {
                    cb(done, tot, format!("Downloading mods {done}/{tot}..."));
                }
            })
            .await?;
        adopt_downloaded_mods(paths, &outcomes);

        if let Some(ref cb) = on_progress {
            cb(1, 1, "Extracting overrides...".to_string());
        }
        let extracted_overrides =
            extract_overrides(archive_path, &["overrides/", "client-overrides/"], &mc_dir)?;
        extract_embedded_instance_icon(archive_path, &store, &inst.instance_id);
        let _ = crate::pack_registry::ModrinthPackRegistry::save(
            &paths.instance_dir(&inst.instance_id),
            &crate::pack_registry::ModrinthPackRecord {
                source: "modrinth".to_string(),
                project_id: String::new(),
                project_slug: String::new(),
                version_id: String::new(),
                version_number: index.version_id.clone(),
                pack_name: target_name.to_string(),
                managed_files,
                updated_at: String::new(),
            },
        );

        Ok(ModpackInstallReport {
            instance_id: inst.instance_id,
            game_version,
            loader,
            downloaded_files,
            extracted_overrides,
            manual_downloads: Vec::new(),
        })
    }

    async fn install_curseforge_zip(
        paths: &LauncherPaths,
        archive_path: &Path,
        name_override: Option<&str>,
        downloader: &DownloadClient,
        curseforge: Option<&CurseForgeClient>,
        options: &ModpackInstallOptions,
        on_progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackInstallReport> {
        let manifest: CurseForgeManifest = {
            let file = File::open(archive_path)?;
            let mut archive = ZipArchive::new(file)
                .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
            let entry = archive
                .by_name("manifest.json")
                .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
            serde_json::from_reader(entry)?
        };

        let game_version = manifest.minecraft.version.clone();
        let mut cf_loaders = manifest.minecraft.mod_loaders.clone();
        if cf_loaders.is_empty() {
            if let Some(single) = manifest.minecraft.mod_loader.clone() {
                cf_loaders.push(single);
            }
        }
        let loader = parse_cf_loader(&cf_loaders, &game_version);
        let base_name = name_override
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(manifest.name.trim());

        let store = InstanceStore::new(paths);
        let target_name = store
            .resolve_unique_instance_name(base_name)
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
        let (inst, mut inst_settings) = store
            .create_instance(&target_name, &game_version, loader.clone())
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
        inst_settings.disable_mod_repair_and_updates = Some(true);
        let _ = store.save_settings(&inst.instance_id, &inst_settings);
        let mc_dir = paths.minecraft_dir(&inst.instance_id);
        let mods_dir = mc_dir.join("mods");
        fs::create_dir_all(&mods_dir)?;

        let mut resolved = Vec::new();
        let mut mod_urls: HashMap<u64, String> = HashMap::new();
        let selected_cf_files: Vec<&CfManifestFile> = manifest
            .files
            .iter()
            .filter(|f| {
                f.required || options.should_include(&format!("{}:{}", f.project_id, f.file_id))
            })
            .collect();
        if !selected_cf_files.is_empty() {
            let cf_client = curseforge.ok_or(ProviderError::MissingApiKey)?;
            if let Some(ref cb) = on_progress {
                cb(
                    0,
                    selected_cf_files.len() as u64,
                    format!(
                        "Resolving {} CurseForge mod files...",
                        selected_cf_files.len()
                    ),
                );
            }
            let file_ids: Vec<u64> = selected_cf_files.iter().map(|f| f.file_id).collect();
            resolved = cf_client.get_files_batch(&file_ids).await?;

            // Batch-resolve project websiteUrl/slug for any files with third-party distribution disabled
            // so manual download links go directly to CurseForge's `/download/{file_id}` countdown page.
            let opt_out_mod_ids: Vec<u64> = resolved
                .iter()
                .filter(|f| {
                    f.download_url
                        .as_deref()
                        .map(str::trim)
                        .unwrap_or("")
                        .is_empty()
                })
                .map(|f| f.mod_id)
                .collect();
            if !opt_out_mod_ids.is_empty() {
                if let Ok(mods_meta) = cf_client.get_mods_batch(&opt_out_mod_ids).await {
                    for m in mods_meta {
                        let best = m
                            .links
                            .as_ref()
                            .and_then(|l| l.website_url.clone())
                            .filter(|u| !u.trim().is_empty())
                            .unwrap_or_else(|| m.slug.clone());
                        if !best.is_empty() {
                            mod_urls.insert(m.id, best);
                        }
                    }
                }
            }
        }

        let mut requests = Vec::new();
        let mut manual_downloads = Vec::new();
        let mut managed_files = Vec::new();
        for cf_file in resolved {
            let project_ref = mod_urls.get(&cf_file.mod_id).map(String::as_str);
            managed_files.push(format!("mods/{}", cf_file.file_name));
            match cf_file.require_download_url(project_ref) {
                Ok(url) => {
                    let dest = mods_dir.join(&cf_file.file_name);
                    requests.push(
                        DownloadRequest::new(url, dest)
                            .with_sha1(cf_file.sha1())
                            .with_size(cf_file.file_length),
                    );
                }
                Err(ProviderError::ManualDownloadRequired {
                    mod_id,
                    file_id,
                    file_name,
                    website_url,
                }) => {
                    manual_downloads.push(ManualDownloadItem {
                        mod_id,
                        file_id,
                        file_name,
                        website_url,
                    });
                }
                Err(other) => return Err(other),
            }
        }

        let downloaded_files = requests.len();
        let prog_cb = on_progress.clone();
        let outcomes = downloader
            .download_batch(requests, DOWNLOAD_CONCURRENCY, move |done, tot| {
                if let Some(ref cb) = prog_cb {
                    cb(done, tot, format!("Downloading mods {done}/{tot}..."));
                }
            })
            .await?;
        adopt_downloaded_mods(paths, &outcomes);

        if let Some(ref cb) = on_progress {
            cb(1, 1, "Extracting overrides...".to_string());
        }
        let override_prefix = format!(
            "{}/",
            manifest
                .overrides
                .as_deref()
                .unwrap_or("overrides")
                .trim_matches('/')
        );
        let extracted_overrides = extract_overrides(archive_path, &[&override_prefix], &mc_dir)?;
        extract_embedded_instance_icon(archive_path, &store, &inst.instance_id);
        let _ = crate::pack_registry::CurseForgePackRegistry::save(
            &paths.instance_dir(&inst.instance_id),
            &crate::pack_registry::CurseForgePackRecord {
                source: "curseforge".to_string(),
                mod_id: 0,
                slug: String::new(),
                pack_name: target_name,
                file_id: 0,
                file_name: archive_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string(),
                managed_files,
                updated_at: String::new(),
            },
        );

        // Persist pending manual downloads inside the instance folder so the user can resume later.
        let pending_path = paths
            .instance_dir(&inst.instance_id)
            .join("pending_manual_downloads.json");
        if !manual_downloads.is_empty() {
            if let Ok(json) = serde_json::to_string_pretty(&manual_downloads) {
                let _ = mcw_storage::atomic_write_text(&pending_path, &json);
            }
        }

        Ok(ModpackInstallReport {
            instance_id: inst.instance_id,
            game_version,
            loader,
            downloaded_files,
            extracted_overrides,
            manual_downloads,
        })
    }

    /// Load persisted pending manual downloads for an instance, filtering out any files
    /// that are already present in `.minecraft/mods/`.
    pub fn load_pending_manual_downloads(
        paths: &LauncherPaths,
        instance_id: &str,
    ) -> Vec<ManualDownloadItem> {
        let pending_path = paths
            .instance_dir(instance_id)
            .join("pending_manual_downloads.json");
        let Ok(raw) = fs::read_to_string(&pending_path) else {
            return Vec::new();
        };
        let items: Vec<ManualDownloadItem> = serde_json::from_str(&raw).unwrap_or_default();
        let mods_dir = paths.minecraft_dir(instance_id).join("mods");
        let remaining: Vec<ManualDownloadItem> = items
            .into_iter()
            .filter(|item| !mods_dir.join(&item.file_name).is_file())
            .collect();
        if remaining.is_empty() {
            let _ = fs::remove_file(&pending_path);
        }
        remaining
    }

    /// Scan `downloads_dir` (e.g. user's Downloads folder) and the instance's `mods/` folder
    /// for any matching `ManualDownloadItem` filenames and copy them into `instances/<id>/.minecraft/mods/`.
    /// Ignores incomplete browser downloads (`.crdownload`, `.part`, `.tmp`, 0-byte files) and handles
    /// browser duplicate suffixes like `ModName (1).jar` or URL-decoded spaces.
    /// Returns `(resolved_count, remaining)`.
    pub fn scan_and_import_manual_downloads(
        paths: &LauncherPaths,
        instance_id: &str,
        items: &[ManualDownloadItem],
        downloads_dir: &Path,
    ) -> Result<(usize, Vec<ManualDownloadItem>)> {
        let mods_dir = paths.minecraft_dir(instance_id).join("mods");
        fs::create_dir_all(&mods_dir)?;

        // Collect completed files in downloads_dir once for flexible matching.
        let download_entries: Vec<(String, PathBuf)> = fs::read_dir(downloads_dir)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if !path.is_file() {
                    return None;
                }
                let meta = entry.metadata().ok()?;
                if meta.len() == 0 {
                    return None;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                let lower = name.to_ascii_lowercase();
                if lower.ends_with(".crdownload")
                    || lower.ends_with(".part")
                    || lower.ends_with(".tmp")
                {
                    return None;
                }
                Some((name, path))
            })
            .collect();

        let mut resolved = 0usize;
        let mut remaining = Vec::new();

        for item in items {
            let target = mods_dir.join(&item.file_name);
            if target.is_file() && target.metadata().map(|m| m.len() > 0).unwrap_or(false) {
                resolved += 1;
                continue;
            }
            let exact = downloads_dir.join(&item.file_name);
            let matched_src =
                if exact.is_file() && exact.metadata().map(|m| m.len() > 0).unwrap_or(false) {
                    Some(exact)
                } else {
                    find_matching_download(&item.file_name, &download_entries)
                };

            if let Some(src) = matched_src {
                fs::copy(&src, &target)?;
                resolved += 1;
            } else {
                remaining.push(item.clone());
            }
        }

        let pending_path = paths
            .instance_dir(instance_id)
            .join("pending_manual_downloads.json");
        if remaining.is_empty() {
            let _ = fs::remove_file(&pending_path);
        } else if let Ok(json) = serde_json::to_string_pretty(&remaining) {
            let _ = mcw_storage::atomic_write_text(&pending_path, &json);
        }

        Ok((resolved, remaining))
    }

    /// Export an existing instance as a shareable Modrinth `.mrpack` archive containing
    /// `modrinth.index.json` and `overrides/` (`mods/`, `config/`, `resourcepacks/`,
    /// `shaderpacks/`, `options.txt`) — excluding personal `saves/`, `logs/`, and `screenshots/`.
    pub fn export_to_mrpack(
        paths: &LauncherPaths,
        instance_id: &str,
        output_path: &Path,
    ) -> Result<usize> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;

        let store = InstanceStore::new(paths);
        let inst = store
            .load_instance(instance_id)
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;

        let mut deps = serde_json::Map::new();
        deps.insert(
            "minecraft".to_string(),
            serde_json::Value::String(inst.version_id.clone()),
        );
        match inst.mod_loader.name.to_ascii_lowercase().as_str() {
            "fabric" => {
                deps.insert(
                    "fabric-loader".to_string(),
                    serde_json::Value::String(inst.mod_loader.version.clone()),
                );
            }
            "quilt" => {
                deps.insert(
                    "quilt-loader".to_string(),
                    serde_json::Value::String(inst.mod_loader.version.clone()),
                );
            }
            "forge" => {
                deps.insert(
                    "forge".to_string(),
                    serde_json::Value::String(inst.mod_loader.version.clone()),
                );
            }
            "neoforge" => {
                deps.insert(
                    "neoforge".to_string(),
                    serde_json::Value::String(inst.mod_loader.version.clone()),
                );
            }
            _ => {}
        }

        let index_json = serde_json::json!({
            "formatVersion": 1,
            "game": "minecraft",
            "versionId": "1.0.0",
            "name": inst.name,
            "summary": format!("Exported from MCW Launcher: Rust Edition (MC {})", inst.version_id),
            "files": [],
            "dependencies": deps,
        });

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = File::create(output_path)?;
        let mut zip = ZipWriter::new(file);
        let opts =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("modrinth.index.json", opts)
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
        zip.write_all(serde_json::to_string_pretty(&index_json)?.as_bytes())?;

        let mc_dir = paths.minecraft_dir(instance_id);
        let include_dirs = ["mods", "config", "resourcepacks", "shaderpacks"];
        let mut exported_files = 0usize;

        for sub in include_dirs {
            let dir = mc_dir.join(sub);
            if !dir.is_dir() {
                continue;
            }
            for entry in walkdir::WalkDir::new(&dir).into_iter().flatten() {
                if !entry.file_type().is_file() {
                    continue;
                }
                let Ok(rel) = entry.path().strip_prefix(&mc_dir) else {
                    continue;
                };
                let zip_path = format!("overrides/{}", rel.to_string_lossy().replace('\\', "/"));
                zip.start_file(&zip_path, opts)
                    .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
                let mut src = File::open(entry.path())?;
                io::copy(&mut src, &mut zip)?;
                exported_files += 1;
            }
        }

        let options_txt = mc_dir.join("options.txt");
        if options_txt.is_file() {
            zip.start_file("overrides/options.txt", opts)
                .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
            let mut src = File::open(&options_txt)?;
            io::copy(&mut src, &mut zip)?;
            exported_files += 1;
        }

        zip.finish()
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
        Ok(exported_files)
    }

    /// Exports an instance using one of MCW Launcher's 3 standard modes:
    /// 1. `Mrpack` (`.mrpack`): `modrinth.index.json` + `overrides/` (+ optional `saves/`)
    /// 2. `ProviderProfile` (`-MCW-Profile.zip`): CurseForge-compatible `manifest.json` + `mcw_profile.json` + `overrides/config`
    /// 3. `Portable` (`.mcwpack`): `mcw_package.json` + `instance.json` + `settings.json` + `overrides/` (Smart or Full + optional `saves/`)
    pub fn export_instance(
        paths: &LauncherPaths,
        instance_id: &str,
        output_path: &Path,
        options: &ModpackExportOptions,
    ) -> Result<usize> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;

        let store = InstanceStore::new(paths);
        let (inst, settings) = store
            .get_instance(instance_id)
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = File::create(output_path)?;
        let mut zip = ZipWriter::new(file);
        let zopts =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        let mc_dir = paths.minecraft_dir(instance_id);
        let mut exported_files = 0usize;

        let add_dir = |zip: &mut ZipWriter<File>, sub: &str, prefix: &str| -> Result<usize> {
            let dir = mc_dir.join(sub);
            if !dir.is_dir() {
                return Ok(0);
            }
            let mut count = 0usize;
            for entry in walkdir::WalkDir::new(&dir).into_iter().flatten() {
                if !entry.file_type().is_file() {
                    continue;
                }
                let Ok(rel) = entry.path().strip_prefix(&mc_dir) else {
                    continue;
                };
                let zip_path = format!("{prefix}{}", rel.to_string_lossy().replace('\\', "/"));
                zip.start_file(&zip_path, zopts)
                    .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
                let mut src = File::open(entry.path())?;
                io::copy(&mut src, zip)?;
                count += 1;
            }
            Ok(count)
        };

        match options.mode {
            ModpackExportMode::Mrpack => {
                let mut deps = serde_json::Map::new();
                deps.insert(
                    "minecraft".to_string(),
                    serde_json::Value::String(inst.version_id.clone()),
                );
                let loader_key = match inst.mod_loader.name.to_ascii_lowercase().as_str() {
                    "fabric" => Some("fabric-loader"),
                    "quilt" => Some("quilt-loader"),
                    "forge" => Some("forge"),
                    "neoforge" => Some("neoforge"),
                    _ => None,
                };
                if let Some(k) = loader_key {
                    if !inst.mod_loader.version.is_empty() && inst.mod_loader.version != "-1" {
                        deps.insert(
                            k.to_string(),
                            serde_json::Value::String(inst.mod_loader.version.clone()),
                        );
                    }
                }
                let index_json = serde_json::json!({
                    "formatVersion": 1,
                    "game": "minecraft",
                    "versionId": "1.0.0",
                    "name": inst.name,
                    "summary": format!("Exported from MCW Launcher: Rust Edition ({})", inst.version_id),
                    "files": [],
                    "dependencies": deps,
                });
                zip.start_file("modrinth.index.json", zopts)
                    .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
                zip.write_all(serde_json::to_string_pretty(&index_json)?.as_bytes())?;

                for sub in ["mods", "config", "resourcepacks", "shaderpacks"] {
                    exported_files += add_dir(&mut zip, sub, "overrides/")?;
                }
                if options.include_saves {
                    exported_files += add_dir(&mut zip, "saves", "overrides/")?;
                }
                let options_txt = mc_dir.join("options.txt");
                if options_txt.is_file() {
                    zip.start_file("overrides/options.txt", zopts)
                        .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
                    let mut src = File::open(&options_txt)?;
                    io::copy(&mut src, &mut zip)?;
                    exported_files += 1;
                }
            }
            ModpackExportMode::ProviderProfile => {
                let mut mod_loaders = Vec::new();
                if !inst.mod_loader.is_vanilla() && !inst.mod_loader.version.is_empty() {
                    mod_loaders.push(serde_json::json!({
                        "id": format!("{}-{}", inst.mod_loader.name, inst.mod_loader.version),
                        "primary": true,
                    }));
                }
                let manifest = serde_json::json!({
                    "minecraft": {
                        "version": inst.version_id,
                        "modLoaders": mod_loaders,
                    },
                    "manifestType": "minecraftModpack",
                    "manifestVersion": 1,
                    "name": inst.name,
                    "version": "1.0.0",
                    "author": "MCW Launcher",
                    "files": [],
                    "overrides": "overrides",
                });
                zip.start_file("manifest.json", zopts)
                    .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
                zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;

                let profile_meta = serde_json::json!({
                    "format": "mcw-provider-profile",
                    "version": 1,
                    "instance": inst,
                });
                zip.start_file("mcw_profile.json", zopts)
                    .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
                zip.write_all(serde_json::to_string_pretty(&profile_meta)?.as_bytes())?;

                exported_files += add_dir(&mut zip, "config", "overrides/")?;
                let options_txt = mc_dir.join("options.txt");
                if options_txt.is_file() {
                    zip.start_file("overrides/options.txt", zopts)
                        .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
                    let mut src = File::open(&options_txt)?;
                    io::copy(&mut src, &mut zip)?;
                    exported_files += 1;
                }
            }
            ModpackExportMode::Portable { full } => {
                let mut export_settings = settings;
                export_settings.disable_mod_repair_and_updates =
                    Some(options.disable_mod_repair_and_updates);
                let pkg_meta = serde_json::json!({
                    "format": "mcwpack",
                    "version": 2,
                    "portable_mode": if full { "full" } else { "smart" },
                    "include_saves": options.include_saves,
                    "disable_mod_repair_and_updates": options.disable_mod_repair_and_updates,
                    "instance": inst,
                    "settings": export_settings,
                });
                zip.start_file("mcw_package.json", zopts)
                    .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
                zip.write_all(serde_json::to_string_pretty(&pkg_meta)?.as_bytes())?;

                let dirs: &[&str] = if full {
                    &[
                        "mods",
                        "config",
                        "resourcepacks",
                        "shaderpacks",
                        "datapacks",
                        "screenshots",
                    ]
                } else {
                    &["mods", "config", "resourcepacks", "shaderpacks"]
                };
                for sub in dirs {
                    exported_files += add_dir(&mut zip, sub, "overrides/")?;
                }
                if options.include_saves {
                    exported_files += add_dir(&mut zip, "saves", "overrides/")?;
                }
                let options_txt = mc_dir.join("options.txt");
                if options_txt.is_file() {
                    zip.start_file("overrides/options.txt", zopts)
                        .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
                    let mut src = File::open(&options_txt)?;
                    io::copy(&mut src, &mut zip)?;
                    exported_files += 1;
                }
            }
        }

        zip.finish()
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
        Ok(exported_files)
    }
}

fn normalize_mod_filename(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let replaced = lower.replace("%20", " ").replace('+', " ");
    // Strip browser duplicate suffix like " (1)" before extension: "foo (1).jar" -> "foo.jar"
    if let Some((stem, ext)) = replaced.rsplit_once('.') {
        let trimmed_stem = strip_duplicate_suffix(stem);
        format!("{trimmed_stem}.{ext}")
    } else {
        replaced
    }
}

fn strip_duplicate_suffix(stem: &str) -> &str {
    if let Some(without_close) = stem.strip_suffix(')') {
        if let Some((prefix, digits)) = without_close.rsplit_once(" (") {
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                return prefix;
            }
        }
    }
    stem
}

fn find_matching_download(expected: &str, entries: &[(String, PathBuf)]) -> Option<PathBuf> {
    let expected_norm = normalize_mod_filename(expected);
    entries
        .iter()
        .find(|(name, _)| normalize_mod_filename(name) == expected_norm)
        .map(|(_, path)| path.clone())
}

/// Canonicalize a modloader name and defensively strip redundant prefixes from its version string
/// (matching MCW Launcher Python's `ModLoaderManager.resolve` and `LOADER_ALIASES`).
pub fn normalize_modloader(loader_name: &str, raw_version: &str, game_version: &str) -> ModLoader {
    let cleaned_name: String = loader_name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .collect();
    let canonical = match cleaned_name.as_str() {
        "" | "vanilla" | "minecraft" | "none" => return ModLoader::vanilla(),
        "fabric" | "fabricloader" => "fabric",
        "quilt" | "quiltloader" => "quilt",
        "forge" | "minecraftforge" => "forge",
        "neoforge" | "neoforged" | "neo" => "neoforge",
        other => other,
    };

    let mut ver = raw_version.trim().to_string();
    // 1. Strip `<loader_name>-` prefix (e.g. `forge-47.3.0` -> `47.3.0`)
    for prefix in [
        format!("{canonical}-"),
        format!("{canonical}/"),
        format!("{canonical}_"),
    ] {
        if ver.len() > prefix.len() && ver[..prefix.len()].eq_ignore_ascii_case(&prefix) {
            ver = ver[prefix.len()..].trim().to_string();
        }
    }
    // 2. For fabric/quilt, strip redundant `loader-` prefix (e.g. `fabric-loader-0.16.9` -> `0.16.9`)
    if matches!(canonical, "fabric" | "quilt")
        && ver.len() > 7
        && ver[..7].eq_ignore_ascii_case("loader-")
    {
        ver = ver[7..].trim().to_string();
    }
    // 3. Strip `<game_version>-` prefix (e.g. `1.20.1-47.3.0` -> `47.3.0`)
    let gv = game_version.trim();
    if !gv.is_empty() {
        let gv_prefix = format!("{gv}-");
        if ver.len() > gv_prefix.len() && ver[..gv_prefix.len()].eq_ignore_ascii_case(&gv_prefix) {
            ver = ver[gv_prefix.len()..].trim().to_string();
        }
    }

    let ver_lower = ver.to_ascii_lowercase();
    if matches!(
        ver_lower.as_str(),
        "" | "-1" | "auto" | "latest" | "recommended" | "none"
    ) {
        ver = String::new();
    }

    ModLoader {
        name: canonical.to_string(),
        version: ver,
    }
}

pub(crate) fn parse_mrpack_loader(deps: &HashMap<String, String>, game_version: &str) -> ModLoader {
    for (key, loader) in [
        ("fabric-loader", "fabric"),
        ("quilt-loader", "quilt"),
        ("neoforge", "neoforge"),
        ("forge", "forge"),
    ] {
        if let Some(v) = deps.get(key) {
            return normalize_modloader(loader, v, game_version);
        }
    }
    ModLoader::vanilla()
}

pub(crate) fn parse_cf_loader(loaders: &[CfModLoader], game_version: &str) -> ModLoader {
    let chosen = loaders
        .iter()
        .find(|l| l.primary)
        .or_else(|| loaders.first());
    let Some(entry) = chosen else {
        return ModLoader::vanilla();
    };
    parse_cf_loader_id(&entry.id, game_version)
}

/// Parse CurseForge `modLoaders[].id` (e.g. `"fabric-loader-0.16.9"`, `"forge-1.20.1-47.3.0"`,
/// `"neoforge-21.1.72"`, `"quilt-loader-0.27.0"`, `"forge"`) matching Python's `_parse_loader_id`.
pub fn parse_cf_loader_id(raw_id: &str, game_version: &str) -> ModLoader {
    let text = raw_id.trim();
    if text.is_empty() {
        return ModLoader::vanilla();
    }
    let lower = text.to_ascii_lowercase();
    // Ordered longest-first so `fabric-loader` / `neoforged` / `minecraftforge` match before shorter aliases.
    const ALIASES: &[(&str, &str)] = &[
        ("minecraftforge", "forge"),
        ("fabric-loader", "fabric"),
        ("quilt-loader", "quilt"),
        ("neoforged", "neoforge"),
        ("neoforge", "neoforge"),
        ("fabric", "fabric"),
        ("quilt", "quilt"),
        ("forge", "forge"),
        ("neo", "neoforge"),
    ];
    for &(alias, canonical) in ALIASES {
        if lower == alias {
            return normalize_modloader(canonical, "", game_version);
        }
        if lower.starts_with(alias) {
            let rest = &text[alias.len()..];
            if let Some(first_ch) = rest.chars().next() {
                if matches!(first_ch, '-' | '/' | '_' | ' ') {
                    return normalize_modloader(canonical, &rest[1..], game_version);
                }
            }
        }
    }
    if let Some((kind, ver)) = text.split_once('-') {
        normalize_modloader(kind, ver, game_version)
    } else {
        ModLoader::vanilla()
    }
}

/// Extract an embedded instance icon from an imported archive if present
/// (matching Python's `apply_embedded_archive_artwork`).
fn extract_embedded_instance_icon(archive_path: &Path, store: &InstanceStore, instance_id: &str) {
    let Ok(file) = File::open(archive_path) else {
        return;
    };
    let Ok(mut archive) = ZipArchive::new(file) else {
        return;
    };
    const PREFIXES: &[&str] = &[
        ".mcw/instance-icon.",
        "mcw/instance-icon.",
        "overrides/.mcw/instance-icon.",
        "overrides/mcw/instance-icon.",
    ];
    for i in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(i) else {
            continue;
        };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        let is_icon = PREFIXES.iter().any(|p| name.starts_with(p))
            || name == "icon.png"
            || name == "overrides/icon.png";
        if is_icon {
            let mut buf = Vec::new();
            if io::Read::read_to_end(&mut entry, &mut buf).is_ok() && !buf.is_empty() {
                let _ = store.set_instance_icon_bytes_with_origin(instance_id, &buf, "embedded");
                return;
            }
        }
    }
}

pub(crate) fn extract_overrides(
    archive_path: &Path,
    prefixes: &[&str],
    mc_dir: &Path,
) -> Result<usize> {
    extract_overrides_filtered(archive_path, prefixes, mc_dir, &[])
}

pub(crate) fn extract_overrides_filtered(
    archive_path: &Path,
    prefixes: &[&str],
    mc_dir: &Path,
    skip_prefixes: &[&str],
) -> Result<usize> {
    let file = File::open(archive_path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;

    let mut extracted = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
        let name = entry.name().replace('\\', "/");
        let matched_rel = prefixes
            .iter()
            .find_map(|p| name.strip_prefix(p))
            .unwrap_or("");
        if matched_rel.is_empty() {
            continue;
        }
        let rel_lower = matched_rel.to_ascii_lowercase();
        if skip_prefixes
            .iter()
            .any(|sp| rel_lower == *sp || rel_lower.starts_with(sp))
        {
            continue;
        }
        let target = safe_join_under(mc_dir, matched_rel)?;
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&target)?;
        io::copy(&mut entry, &mut out)?;
        extracted += 1;
    }
    Ok(extracted)
}

pub(crate) fn safe_join_under(base: &Path, relative: &str) -> Result<PathBuf> {
    let normalized = relative.replace('\\', "/");
    let mut out = base.to_path_buf();
    for comp in Path::new(&normalized).components() {
        match comp {
            Component::Normal(p) => {
                if p.to_string_lossy().contains(':') {
                    return Err(ProviderError::Io(io::Error::other(format!(
                        "Unsafe modpack path: {relative}"
                    ))));
                }
                out.push(p);
            }
            _ => {
                return Err(ProviderError::Io(io::Error::other(format!(
                    "Unsafe modpack path: {relative}"
                ))));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    #[tokio::test]
    async fn installs_mrpack_with_overrides_and_loader_metadata() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();

        let mrpack = dir.path().join("demo.mrpack");
        {
            let mut zip = ZipWriter::new(File::create(&mrpack).unwrap());
            zip.start_file("modrinth.index.json", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(
                br#"{
                    "name": "Demo Pack",
                    "dependencies": { "minecraft": "1.21.4", "fabric-loader": "0.16.10" },
                    "files": []
                }"#,
            )
            .unwrap();
            zip.start_file("overrides/options.txt", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"fov:0.5\n").unwrap();
            zip.finish().unwrap();
        }

        let downloader = DownloadClient::new().unwrap();
        let report = ModpackInstaller::install_from_archive(
            &paths,
            &mrpack,
            Some("my-pack"),
            &downloader,
            None,
        )
        .await
        .unwrap();

        assert_eq!(report.instance_id, "my-pack");
        assert_eq!(report.game_version, "1.21.4");
        assert_eq!(report.loader.name, "fabric");
        assert_eq!(report.extracted_overrides, 1);
        assert_eq!(
            fs::read_to_string(paths.minecraft_dir("my-pack").join("options.txt")).unwrap(),
            "fov:0.5\n"
        );
    }

    #[test]
    fn scans_and_imports_manual_downloads_with_browser_suffix_and_ignores_partial() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();
        let inst_dir = paths.instance_dir("rlcraft");
        fs::create_dir_all(&inst_dir).unwrap();

        let dl_dir = dir.path().join("Downloads");
        fs::create_dir_all(&dl_dir).unwrap();
        // Browser duplicate suffix match
        fs::write(dl_dir.join("OptOutMod-1.0 (1).jar"), b"fake-jar-bytes").unwrap();
        // Incomplete browser partial download must be ignored
        fs::write(dl_dir.join("SecondMod-2.0.jar.crdownload"), b"partial").unwrap();

        let items = vec![
            ManualDownloadItem {
                mod_id: 100,
                file_id: 200,
                file_name: "OptOutMod-1.0.jar".to_string(),
                website_url: "https://example.com/1".to_string(),
            },
            ManualDownloadItem {
                mod_id: 101,
                file_id: 201,
                file_name: "SecondMod-2.0.jar".to_string(),
                website_url: "https://example.com/2".to_string(),
            },
        ];

        let (resolved, remaining) =
            ModpackInstaller::scan_and_import_manual_downloads(&paths, "rlcraft", &items, &dl_dir)
                .unwrap();
        assert_eq!(resolved, 1);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].file_name, "SecondMod-2.0.jar");
        assert!(paths
            .minecraft_dir("rlcraft")
            .join("mods")
            .join("OptOutMod-1.0.jar")
            .is_file());
        // Still persisted as incomplete
        assert_eq!(
            ModpackInstaller::load_pending_manual_downloads(&paths, "rlcraft").len(),
            1
        );

        // Now complete the second file
        fs::write(dl_dir.join("SecondMod-2.0.jar"), b"jar2").unwrap();
        let (resolved2, remaining2) = ModpackInstaller::scan_and_import_manual_downloads(
            &paths, "rlcraft", &remaining, &dl_dir,
        )
        .unwrap();
        assert_eq!(resolved2, 1);
        assert!(remaining2.is_empty());
        assert!(ModpackInstaller::load_pending_manual_downloads(&paths, "rlcraft").is_empty());
    }

    #[test]
    fn exports_instance_to_valid_mrpack_and_skips_saves() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();

        let store = InstanceStore::new(&paths);
        store
            .create_instance(
                "export-test",
                "1.21.4",
                ModLoader {
                    name: "fabric".to_string(),
                    version: "0.16.10".to_string(),
                },
            )
            .unwrap();
        let mc = paths.minecraft_dir("export-test");
        fs::create_dir_all(mc.join("mods")).unwrap();
        fs::create_dir_all(mc.join("saves/world1")).unwrap();
        fs::write(mc.join("mods/demo.jar"), b"jar-bytes").unwrap();
        fs::write(mc.join("saves/world1/level.dat"), b"save-bytes").unwrap();

        let out = dir.path().join("export-test.mrpack");
        let count = ModpackInstaller::export_to_mrpack(&paths, "export-test", &out).unwrap();
        assert_eq!(count, 1);
        assert!(out.is_file());

        let mut zip = ZipArchive::new(File::open(&out).unwrap()).unwrap();
        assert!(zip.by_name("modrinth.index.json").is_ok());
        assert!(zip.by_name("overrides/mods/demo.jar").is_ok());
        assert!(zip.by_name("overrides/saves/world1/level.dat").is_err());
    }

    #[test]
    fn normalizes_modpack_loaders_matching_python() {
        let l1 = parse_cf_loader_id("fabric-loader-0.16.9", "1.21.4");
        assert_eq!(l1.name, "fabric");
        assert_eq!(l1.version, "0.16.9");

        let l2 = parse_cf_loader_id("forge-1.20.1-47.3.0", "1.20.1");
        assert_eq!(l2.name, "forge");
        assert_eq!(l2.version, "47.3.0");

        let l3 = parse_cf_loader_id("quilt-loader-0.27.0", "1.20.1");
        assert_eq!(l3.name, "quilt");
        assert_eq!(l3.version, "0.27.0");

        let l4 = parse_cf_loader_id("neoforged-21.1.72", "1.21.1");
        assert_eq!(l4.name, "neoforge");
        assert_eq!(l4.version, "21.1.72");

        let l5 = normalize_modloader("FabricLoader", "loader-0.16.10", "1.21.4");
        assert_eq!(l5.name, "fabric");
        assert_eq!(l5.version, "0.16.10");

        let l6 = normalize_modloader("MinecraftForge", "1.20.1-47.2.20", "1.20.1");
        assert_eq!(l6.name, "forge");
        assert_eq!(l6.version, "47.2.20");

        let l7 = parse_cf_loader_id("forge", "1.20.1");
        assert_eq!(l7.name, "forge");
        assert_eq!(l7.version, "");
    }

    #[tokio::test]
    async fn exports_and_imports_mcwpack_portable() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();

        let store = InstanceStore::new(&paths);
        store
            .create_instance(
                "portable-src",
                "1.21.4",
                ModLoader {
                    name: "fabric".to_string(),
                    version: "0.16.10".to_string(),
                },
            )
            .unwrap();
        let mc = paths.minecraft_dir("portable-src");
        fs::create_dir_all(mc.join("mods")).unwrap();
        fs::write(mc.join("mods/sodium.jar"), b"sodium-bytes").unwrap();

        let out = dir.path().join("portable-src.mcwpack");
        ModpackInstaller::export_instance(
            &paths,
            "portable-src",
            &out,
            &ModpackExportOptions {
                mode: ModpackExportMode::Portable { full: false },
                include_saves: false,
                disable_mod_repair_and_updates: true,
            },
        )
        .unwrap();

        let dl = DownloadClient::new().unwrap();
        let rep = ModpackInstaller::install_from_archive(
            &paths,
            &out,
            Some("portable-imported"),
            &dl,
            None,
        )
        .await
        .unwrap();
        assert_eq!(rep.instance_id, "portable-imported");
        assert_eq!(rep.loader.name, "fabric");
        assert!(paths
            .minecraft_dir("portable-imported")
            .join("mods/sodium.jar")
            .is_file());
    }

    #[test]
    fn inspects_optional_files_from_mrpack_and_curseforge_archives() {
        let dir = tempdir().unwrap();
        let mrpack = dir.path().join("opt.mrpack");
        {
            let mut zip = ZipWriter::new(File::create(&mrpack).unwrap());
            zip.start_file("modrinth.index.json", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(
                br#"{
                    "name": "Opt Pack",
                    "dependencies": { "minecraft": "1.21.4", "fabric-loader": "0.16.10" },
                    "files": [
                        { "path": "mods/req.jar", "env": { "client": "required" } },
                        { "path": "mods/shader-opt.jar", "env": { "client": "optional" } }
                    ]
                }"#,
            )
            .unwrap();
            zip.finish().unwrap();
        }
        let mr_opts = ModpackInstaller::inspect_optional_files(&mrpack).unwrap();
        assert_eq!(mr_opts.len(), 1);
        assert_eq!(mr_opts[0].path, "mods/shader-opt.jar");

        let cf_zip = dir.path().join("cf_opt.zip");
        {
            let mut zip = ZipWriter::new(File::create(&cf_zip).unwrap());
            zip.start_file("manifest.json", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(
                br#"{
                    "name": "CF Opt",
                    "minecraft": { "version": "1.20.1", "modLoaders": [] },
                    "files": [
                        { "projectID": 10, "fileID": 100, "required": true },
                        { "projectID": 20, "fileID": 200, "required": false }
                    ]
                }"#,
            )
            .unwrap();
            zip.finish().unwrap();
        }
        let cf_opts = ModpackInstaller::inspect_optional_files(&cf_zip).unwrap();
        assert_eq!(cf_opts.len(), 1);
        assert_eq!(cf_opts[0].id, "20:200");
    }
}
