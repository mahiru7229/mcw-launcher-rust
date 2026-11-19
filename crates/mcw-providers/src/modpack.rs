use crate::curseforge::CurseForgeClient;
use crate::{ProviderError, Result};
use mcw_models::instance::ModLoader;
use mcw_net::{DownloadClient, DownloadRequest};
use mcw_storage::{InstanceStore, LauncherPaths};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use zip::ZipArchive;

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
struct MrpackIndex {
    name: String,
    dependencies: HashMap<String, String>,
    #[serde(default)]
    files: Vec<MrpackFile>,
}

#[derive(Debug, Deserialize)]
struct MrpackFile {
    path: String,
    #[serde(default)]
    hashes: HashMap<String, String>,
    #[serde(default)]
    downloads: Vec<String>,
    #[serde(rename = "fileSize", default)]
    file_size: u64,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct CurseForgeManifest {
    name: String,
    minecraft: CfMinecraftSpec,
    #[serde(default)]
    overrides: Option<String>,
    #[serde(default)]
    files: Vec<CfManifestFile>,
}

#[derive(Debug, Deserialize)]
struct CfMinecraftSpec {
    version: String,
    #[serde(rename = "modLoaders", default)]
    mod_loaders: Vec<CfModLoader>,
}

#[derive(Debug, Deserialize)]
struct CfModLoader {
    id: String,
    #[serde(default)]
    primary: bool,
}

#[derive(Debug, Deserialize)]
struct CfManifestFile {
    #[serde(rename = "projectID")]
    #[allow(dead_code)]
    project_id: u64,
    #[serde(rename = "fileID")]
    file_id: u64,
}

pub struct ModpackInstaller;

impl ModpackInstaller {
    /// Auto-detect and install either a Modrinth `.mrpack` or CurseForge `.zip` modpack.
    pub async fn install_from_archive(
        paths: &LauncherPaths,
        archive_path: &Path,
        instance_name_override: Option<&str>,
        downloader: &DownloadClient,
        curseforge: Option<&CurseForgeClient>,
    ) -> Result<ModpackInstallReport> {
        Self::install_from_archive_with_progress(
            paths,
            archive_path,
            instance_name_override,
            downloader,
            curseforge,
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
        let file = File::open(archive_path)?;
        let mut archive = ZipArchive::new(file)
            .map_err(|e| ProviderError::Io(io::Error::other(format!("Invalid archive: {e}"))))?;

        if archive.by_name("modrinth.index.json").is_ok() {
            drop(archive);
            return Self::install_mrpack(
                paths,
                archive_path,
                instance_name_override,
                downloader,
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
                on_progress,
            )
            .await;
        }

        Err(ProviderError::Io(io::Error::other(
            "Unsupported modpack archive: missing modrinth.index.json or manifest.json",
        )))
    }

    async fn install_mrpack(
        paths: &LauncherPaths,
        archive_path: &Path,
        name_override: Option<&str>,
        downloader: &DownloadClient,
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
        let loader = parse_mrpack_loader(&index.dependencies);
        let target_name = name_override
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&index.name);

        let store = InstanceStore::new(paths);
        let (inst, _) = store
            .create_instance(target_name, &game_version, loader.clone())
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
        let mc_dir = paths.minecraft_dir(&inst.instance_id);

        let mut requests = Vec::new();
        for f in &index.files {
            if let Some(ref env) = f.env {
                if env
                    .get("client")
                    .map(|v| v.eq_ignore_ascii_case("unsupported"))
                    .unwrap_or(false)
                {
                    continue;
                }
            }
            let Some(primary_url) = f.downloads.first() else {
                continue;
            };
            let dest = safe_join_under(&mc_dir, &f.path)?;
            let mut req = DownloadRequest::new(primary_url, dest)
                .with_sha1(f.hashes.get("sha1").cloned())
                .with_size(f.file_size);
            if f.downloads.len() > 1 {
                req.mirrors = f.downloads[1..].to_vec();
            }
            requests.push(req);
        }

        let downloaded_files = requests.len();
        let prog_cb = on_progress.clone();
        downloader
            .download_batch(requests, DOWNLOAD_CONCURRENCY, move |done, tot| {
                if let Some(ref cb) = prog_cb {
                    cb(done, tot, format!("Downloading mods {done}/{tot}..."));
                }
            })
            .await?;

        if let Some(ref cb) = on_progress {
            cb(1, 1, "Extracting overrides...".to_string());
        }
        let extracted_overrides = extract_overrides(
            archive_path,
            &["overrides/", "client-overrides/"],
            &mc_dir,
        )?;

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

        let cf_client = curseforge.ok_or(ProviderError::MissingApiKey)?;
        let game_version = manifest.minecraft.version.clone();
        let loader = parse_cf_loader(&manifest.minecraft.mod_loaders);
        let target_name = name_override
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&manifest.name);

        let store = InstanceStore::new(paths);
        let (inst, _) = store
            .create_instance(target_name, &game_version, loader.clone())
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
        let mc_dir = paths.minecraft_dir(&inst.instance_id);
        let mods_dir = mc_dir.join("mods");
        fs::create_dir_all(&mods_dir)?;

        if let Some(ref cb) = on_progress {
            cb(
                0,
                manifest.files.len() as u64,
                format!("Resolving {} CurseForge mod files...", manifest.files.len()),
            );
        }
        let file_ids: Vec<u64> = manifest.files.iter().map(|f| f.file_id).collect();
        let resolved = cf_client.get_files_batch(&file_ids).await?;

        // Batch-resolve project websiteUrl/slug for any files with third-party distribution disabled
        // so manual download links go directly to CurseForge's `/download/{file_id}` countdown page.
        let opt_out_mod_ids: Vec<u64> = resolved
            .iter()
            .filter(|f| f.download_url.as_deref().map(str::trim).unwrap_or("").is_empty())
            .map(|f| f.mod_id)
            .collect();
        let mut mod_urls: HashMap<u64, String> = HashMap::new();
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

        let mut requests = Vec::new();
        let mut manual_downloads = Vec::new();
        for cf_file in resolved {
            let project_ref = mod_urls.get(&cf_file.mod_id).map(String::as_str);
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
        downloader
            .download_batch(requests, DOWNLOAD_CONCURRENCY, move |done, tot| {
                if let Some(ref cb) = prog_cb {
                    cb(done, tot, format!("Downloading mods {done}/{tot}..."));
                }
            })
            .await?;

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

        // Persist pending manual downloads inside the instance folder so the user can resume later.
        let pending_path = paths.instance_dir(&inst.instance_id).join("pending_manual_downloads.json");
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
            let matched_src = if exact.is_file()
                && exact.metadata().map(|m| m.len() > 0).unwrap_or(false)
            {
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

fn parse_mrpack_loader(deps: &HashMap<String, String>) -> ModLoader {
    if let Some(v) = deps.get("fabric-loader") {
        return ModLoader {
            name: "fabric".to_string(),
            version: v.clone(),
        };
    }
    if let Some(v) = deps.get("quilt-loader") {
        return ModLoader {
            name: "quilt".to_string(),
            version: v.clone(),
        };
    }
    if let Some(v) = deps.get("neoforge") {
        return ModLoader {
            name: "neoforge".to_string(),
            version: v.clone(),
        };
    }
    if let Some(v) = deps.get("forge") {
        return ModLoader {
            name: "forge".to_string(),
            version: v.clone(),
        };
    }
    ModLoader::vanilla()
}

fn parse_cf_loader(loaders: &[CfModLoader]) -> ModLoader {
    let chosen = loaders
        .iter()
        .find(|l| l.primary)
        .or_else(|| loaders.first());
    let Some(entry) = chosen else {
        return ModLoader::vanilla();
    };
    if let Some((kind, ver)) = entry.id.split_once('-') {
        ModLoader {
            name: kind.to_ascii_lowercase(),
            version: ver.to_string(),
        }
    } else {
        ModLoader::vanilla()
    }
}

fn extract_overrides(archive_path: &Path, prefixes: &[&str], mc_dir: &Path) -> Result<usize> {
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

fn safe_join_under(base: &Path, relative: &str) -> Result<PathBuf> {
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

        let (resolved, remaining) = ModpackInstaller::scan_and_import_manual_downloads(
            &paths, "rlcraft", &items, &dl_dir,
        )
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
            &paths,
            "rlcraft",
            &remaining,
            &dl_dir,
        )
        .unwrap();
        assert_eq!(resolved2, 1);
        assert!(remaining2.is_empty());
        assert!(ModpackInstaller::load_pending_manual_downloads(&paths, "rlcraft").is_empty());
    }
}
