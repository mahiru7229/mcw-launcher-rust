use crate::modpack::{
    extract_overrides_filtered, parse_mrpack_loader, safe_join_under, ModpackProgressCallback,
    MrpackIndex,
};
use crate::modrinth::ModrinthClient;
use crate::pack_registry::{safe_relative_posix, ModrinthPackRecord, ModrinthPackRegistry};
use crate::{ProviderError, Result};
use mcw_models::instance::ModLoader;
use mcw_net::{DownloadClient, DownloadRequest};
use mcw_storage::{InstanceStore, LauncherPaths};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io;
use std::path::Path;
use zip::ZipArchive;

const DOWNLOAD_CONCURRENCY: usize = 20;

const PRESERVED_USER_PREFIXES: &[&str] = &[
    "saves/",
    "config/",
    "options.txt",
    "servers.dat",
    "screenshots/",
    "resourcepacks/",
    "shaderpacks/",
];

const PRESERVED_OVERRIDE_PREFIXES: &[&str] =
    &["saves/", "options.txt", "servers.dat", "screenshots/"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModpackUpdateCandidate {
    pub version_id: String,
    pub version_number: String,
    pub name: String,
    pub version_type: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub download_url: String,
    pub file_name: String,
    pub file_size: u64,
    pub sha1: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ModpackUpdateCheck {
    pub instance_id: String,
    pub pack_name: String,
    pub project_slug_or_id: String,
    pub current_version_id: String,
    pub current_version_number: String,
    pub versions: Vec<ModpackUpdateCandidate>,
}

#[derive(Debug, Clone)]
pub struct ModpackUpdateReport {
    pub instance_id: String,
    pub new_version_id: String,
    pub new_version_number: String,
    pub game_version: String,
    pub loader: ModLoader,
    pub removed_files: usize,
    pub downloaded_files: usize,
    pub extracted_overrides: usize,
}

/// In-place Modrinth `.mrpack` update check & safe upgrade manager
/// (mirroring Python `modrinth_pack_update_manager.py`).
pub struct ModpackUpdateService;

impl ModpackUpdateService {
    pub async fn check_modrinth_updates(
        instance_id: &str,
        instance_dir: &Path,
        modrinth: &ModrinthClient,
        include_beta: bool,
        include_alpha: bool,
    ) -> Result<ModpackUpdateCheck> {
        let Some(rec) = ModrinthPackRegistry::load(instance_dir) else {
            return Err(ProviderError::Io(io::Error::other(
                "This instance does not have a recorded Modrinth modpack manifest (.mcw/modrinth-pack.json).",
            )));
        };
        let project_ref = if !rec.project_id.trim().is_empty() {
            rec.project_id.trim().to_string()
        } else if !rec.project_slug.trim().is_empty() {
            rec.project_slug.trim().to_string()
        } else {
            return Err(ProviderError::Io(io::Error::other(
                "No Modrinth project ID or slug recorded for this modpack (local .mrpack import cannot check remote updates).",
            )));
        };

        let all_versions = modrinth.list_versions(&project_ref, None, None).await?;
        let versions: Vec<ModpackUpdateCandidate> = all_versions
            .into_iter()
            .filter(|v| {
                let vt = v.version_type.trim().to_ascii_lowercase();
                if vt == "alpha" {
                    include_alpha
                } else if vt == "beta" {
                    include_beta
                } else {
                    true
                }
            })
            .filter_map(|v| {
                let file = v.primary_file()?.clone();
                Some(ModpackUpdateCandidate {
                    version_id: v.id,
                    version_number: v.version_number,
                    name: v.name,
                    version_type: v.version_type,
                    game_versions: v.game_versions,
                    loaders: v.loaders,
                    download_url: file.url,
                    file_name: file.filename,
                    file_size: file.size,
                    sha1: file.hashes.get("sha1").cloned(),
                })
            })
            .collect();

        Ok(ModpackUpdateCheck {
            instance_id: instance_id.to_string(),
            pack_name: rec.pack_name,
            project_slug_or_id: project_ref,
            current_version_id: rec.version_id,
            current_version_number: rec.version_number,
            versions,
        })
    }

    pub async fn apply_modrinth_update(
        paths: &LauncherPaths,
        instance_id: &str,
        target: &ModpackUpdateCandidate,
        downloader: &DownloadClient,
        on_progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackUpdateReport> {
        let mrpack_dir = paths.cache_root.join("modpacks");
        let _ = fs::create_dir_all(&mrpack_dir);
        let safe_name = safe_relative_posix(&target.file_name, &format!("{instance_id}.mrpack"));
        let file_only = Path::new(&safe_name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("update.mrpack");
        let archive_path = mrpack_dir.join(file_only);

        if let Some(ref cb) = on_progress {
            cb(0, 1, format!("Downloading {file_only}..."));
        }
        let req = DownloadRequest::new(&target.download_url, &archive_path)
            .with_sha1(target.sha1.clone())
            .with_size(target.file_size);
        downloader.download_file(&req).await?;

        Self::apply_mrpack_archive_in_place(
            paths,
            instance_id,
            &archive_path,
            Some(&target.version_id),
            Some(&target.version_number),
            downloader,
            on_progress,
        )
        .await
    }

    pub async fn apply_mrpack_archive_in_place(
        paths: &LauncherPaths,
        instance_id: &str,
        archive_path: &Path,
        version_id_override: Option<&str>,
        version_number_override: Option<&str>,
        downloader: &DownloadClient,
        on_progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackUpdateReport> {
        let index: MrpackIndex = {
            let file = File::open(archive_path)?;
            let mut archive = ZipArchive::new(file)
                .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
            let entry = archive
                .by_name("modrinth.index.json")
                .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
            serde_json::from_reader(entry)?
        };

        let store = InstanceStore::new(paths);
        let inst = store
            .load_instance(instance_id)
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
        let inst_dir = paths.instance_dir(instance_id);
        let mc_dir = paths.minecraft_dir(instance_id);
        let old_record = ModrinthPackRegistry::load(&inst_dir).unwrap_or_default();

        let game_version = index
            .dependencies
            .get("minecraft")
            .cloned()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| inst.version_id.clone());
        let loader = parse_mrpack_loader(&index.dependencies, &game_version);

        let mut requests = Vec::new();
        let mut new_managed: Vec<String> = Vec::new();
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
            let rel_norm = safe_relative_posix(&f.path, &f.path);
            new_managed.push(rel_norm.clone());
            let Some(primary_url) = f.downloads.first() else {
                continue;
            };
            let dest = safe_join_under(&mc_dir, &rel_norm)?;
            let mut req = DownloadRequest::new(primary_url, dest)
                .with_sha1(f.hashes.get("sha1").cloned())
                .with_size(f.file_size);
            if f.downloads.len() > 1 {
                req.mirrors = f.downloads[1..].to_vec();
            }
            requests.push(req);
        }

        // Remove ONLY obsolete managed files from the old manifest while protecting user data.
        let new_set: HashSet<String> = new_managed.iter().map(|s| s.to_ascii_lowercase()).collect();
        let mut removed_files = 0usize;
        for old_rel in &old_record.managed_files {
            let clean = safe_relative_posix(old_rel, old_rel);
            let lower = clean.to_ascii_lowercase();
            if new_set.contains(&lower) {
                continue;
            }
            if PRESERVED_USER_PREFIXES
                .iter()
                .any(|p| lower == *p || lower.starts_with(p))
            {
                continue;
            }
            if let Ok(target) = safe_join_under(&mc_dir, &clean) {
                if target.is_file() && fs::remove_file(&target).is_ok() {
                    removed_files += 1;
                }
            }
        }

        let downloaded_files = requests.len();
        let prog_cb = on_progress.clone();
        downloader
            .download_batch(requests, DOWNLOAD_CONCURRENCY, move |done, tot| {
                if let Some(ref cb) = prog_cb {
                    cb(done, tot, format!("Updating pack files {done}/{tot}..."));
                }
            })
            .await?;

        if let Some(ref cb) = on_progress {
            cb(
                1,
                1,
                "Applying updated overrides (preserving saves & options)...".to_string(),
            );
        }
        let extracted_overrides = extract_overrides_filtered(
            archive_path,
            &["overrides/", "client-overrides/"],
            &mc_dir,
            PRESERVED_OVERRIDE_PREFIXES,
        )?;

        store
            .set_runtime_profile(instance_id, &game_version, loader.clone())
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;

        let new_ver_id = version_id_override
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&old_record.version_id)
            .to_string();
        let new_ver_num = version_number_override
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&index.version_id)
            .to_string();

        ModrinthPackRegistry::save(
            &inst_dir,
            &ModrinthPackRecord {
                source: "modrinth".to_string(),
                project_id: old_record.project_id,
                project_slug: old_record.project_slug,
                version_id: new_ver_id.clone(),
                version_number: new_ver_num.clone(),
                pack_name: if index.name.trim().is_empty() {
                    old_record.pack_name
                } else {
                    index.name
                },
                managed_files: new_managed,
                updated_at: String::new(),
            },
        )?;

        Ok(ModpackUpdateReport {
            instance_id: instance_id.to_string(),
            new_version_id: new_ver_id,
            new_version_number: new_ver_num,
            game_version,
            loader,
            removed_files,
            downloaded_files,
            extracted_overrides,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    #[tokio::test]
    async fn updates_mrpack_in_place_preserving_saves_and_pruning_stale_managed_mods() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();

        let store = InstanceStore::new(&paths);
        store
            .create_instance(
                "pack-inst",
                "1.20.1",
                ModLoader {
                    name: "fabric".to_string(),
                    version: "0.15.0".to_string(),
                },
            )
            .unwrap();
        let inst_dir = paths.instance_dir("pack-inst");
        let mc_dir = paths.minecraft_dir("pack-inst");
        fs::create_dir_all(mc_dir.join("mods")).unwrap();
        fs::create_dir_all(mc_dir.join("saves/MyWorld")).unwrap();
        fs::create_dir_all(mc_dir.join("config")).unwrap();
        fs::write(mc_dir.join("mods/old-mod.jar"), b"old").unwrap();
        fs::write(mc_dir.join("mods/user-custom-mod.jar"), b"keep").unwrap();
        fs::write(mc_dir.join("saves/MyWorld/level.dat"), b"world-data").unwrap();
        fs::write(mc_dir.join("options.txt"), b"fov:90").unwrap();
        fs::write(mc_dir.join("config/user.cfg"), b"keep-config").unwrap();

        ModrinthPackRegistry::save(
            &inst_dir,
            &ModrinthPackRecord {
                source: "modrinth".to_string(),
                project_id: "proj123".to_string(),
                project_slug: "test-pack".to_string(),
                version_id: "v1".to_string(),
                version_number: "1.0.0".to_string(),
                pack_name: "Test Pack".to_string(),
                managed_files: vec!["mods/old-mod.jar".to_string()],
                updated_at: String::new(),
            },
        )
        .unwrap();

        let archive = dir.path().join("update.mrpack");
        {
            let mut zw = ZipWriter::new(File::create(&archive).unwrap());
            zw.start_file("modrinth.index.json", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(
                br#"{
                    "name": "Test Pack",
                    "versionId": "2.0.0",
                    "dependencies": { "minecraft": "1.21.4", "fabric-loader": "0.16.10" },
                    "files": []
                }"#,
            )
            .unwrap();
            zw.start_file(
                "overrides/saves/MyWorld/level.dat",
                SimpleFileOptions::default(),
            )
            .unwrap();
            zw.write_all(b"should-not-overwrite").unwrap();
            zw.start_file("overrides/options.txt", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(b"fov:70").unwrap();
            zw.start_file(
                "overrides/config/pack-new.cfg",
                SimpleFileOptions::default(),
            )
            .unwrap();
            zw.write_all(b"new-pack-config").unwrap();
            zw.finish().unwrap();
        }

        let dl = DownloadClient::new().unwrap();
        let report = ModpackUpdateService::apply_mrpack_archive_in_place(
            &paths,
            "pack-inst",
            &archive,
            Some("v2"),
            Some("2.0.0"),
            &dl,
            None,
        )
        .await
        .unwrap();

        assert_eq!(report.removed_files, 1);
        assert_eq!(report.game_version, "1.21.4");
        assert!(!mc_dir.join("mods/old-mod.jar").exists());
        assert!(mc_dir.join("mods/user-custom-mod.jar").is_file());
        assert_eq!(
            fs::read(mc_dir.join("saves/MyWorld/level.dat")).unwrap(),
            b"world-data"
        );
        assert_eq!(fs::read(mc_dir.join("options.txt")).unwrap(), b"fov:90");
        assert!(mc_dir.join("config/user.cfg").is_file());
        assert!(mc_dir.join("config/pack-new.cfg").is_file());

        let updated_rec = ModrinthPackRegistry::load(&inst_dir).unwrap();
        assert_eq!(updated_rec.version_id, "v2");
        assert_eq!(updated_rec.version_number, "2.0.0");
    }
}
