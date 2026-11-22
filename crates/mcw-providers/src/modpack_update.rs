use crate::atlauncher::{is_atlauncher_loader_entry, parse_atlauncher_loader, AtLauncherClient};
use crate::curseforge::CurseForgeClient;
use crate::ftb::FtbClient;
use crate::modpack::{
    extract_overrides_filtered, normalize_modloader, parse_cf_loader, parse_mrpack_loader,
    safe_join_under, CurseForgeManifest, ModpackProgressCallback, MrpackIndex,
};
use crate::modrinth::ModrinthClient;
use crate::pack_registry::{
    safe_relative_posix, AtLauncherManagedFile, AtLauncherPackRecord, AtLauncherPackRegistry,
    CurseForgePackRecord, CurseForgePackRegistry, FtbManagedFile, FtbPackRecord, FtbPackRegistry,
    ModrinthPackRecord, ModrinthPackRegistry,
};
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
const FTB_API_BASE: &str = "https://api.feed-the-beast.com/v1/modpacks/public";
const ATLAUNCHER_CDN_BASE: &str = "https://download.nodecdn.net/containers/atl/";

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
    #[serde(default)]
    pub provider: String,
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
    pub provider: String,
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

fn prune_obsolete_managed_files(
    mc_dir: &Path,
    old_managed: &[String],
    new_managed: &[String],
) -> usize {
    let new_set: HashSet<String> = new_managed.iter().map(|s| s.to_ascii_lowercase()).collect();
    let mut removed = 0usize;
    for old_rel in old_managed {
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
        if let Ok(target) = safe_join_under(mc_dir, &clean) {
            if target.is_file() && fs::remove_file(&target).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

/// In-place safe modpack update manager supporting Modrinth, CurseForge, FTB, and ATLauncher
/// while preserving player saves, options, servers, screenshots, and user configs.
pub struct ModpackUpdateService;

impl ModpackUpdateService {
    #[allow(clippy::too_many_arguments)]
    pub async fn check_updates(
        instance_id: &str,
        instance_dir: &Path,
        modrinth: &ModrinthClient,
        curseforge: Option<&CurseForgeClient>,
        ftb: Option<&FtbClient>,
        atlauncher: Option<&AtLauncherClient>,
        include_beta: bool,
        include_alpha: bool,
    ) -> Result<ModpackUpdateCheck> {
        if let Some(rec) = ModrinthPackRegistry::load(instance_dir) {
            if !rec.project_id.trim().is_empty() || !rec.project_slug.trim().is_empty() {
                return Self::check_modrinth_updates(
                    instance_id,
                    instance_dir,
                    modrinth,
                    include_beta,
                    include_alpha,
                )
                .await;
            }
        }
        if let Some(rec) = CurseForgePackRegistry::load(instance_dir) {
            if rec.mod_id > 0 {
                let cf = curseforge.ok_or(ProviderError::MissingApiKey)?;
                let files = cf.get_mod_files(rec.mod_id, None, None).await?;
                let versions: Vec<ModpackUpdateCandidate> = files
                    .into_iter()
                    .filter(|f| match f.release_type {
                        3 => include_alpha,
                        2 => include_beta,
                        _ => true,
                    })
                    .map(|f| {
                        let vt = match f.release_type {
                            3 => "alpha",
                            2 => "beta",
                            _ => "release",
                        };
                        let mut gvs = Vec::new();
                        let mut loaders = Vec::new();
                        for token in &f.game_versions {
                            let low = token.to_ascii_lowercase();
                            if matches!(low.as_str(), "forge" | "neoforge" | "fabric" | "quilt") {
                                if !loaders.contains(&low) {
                                    loaders.push(low);
                                }
                            } else if token
                                .chars()
                                .next()
                                .map(|c| c.is_ascii_digit())
                                .unwrap_or(false)
                            {
                                gvs.push(token.clone());
                            }
                        }
                        let disp = if f.display_name.trim().is_empty() {
                            f.file_name.clone()
                        } else {
                            f.display_name.clone()
                        };
                        ModpackUpdateCandidate {
                            provider: "curseforge".to_string(),
                            version_id: f.id.to_string(),
                            version_number: disp.clone(),
                            name: disp,
                            version_type: vt.to_string(),
                            game_versions: gvs,
                            loaders,
                            download_url: f.download_url.clone().unwrap_or_default(),
                            file_name: f.file_name.clone(),
                            file_size: f.file_length,
                            sha1: f.sha1(),
                        }
                    })
                    .collect();
                return Ok(ModpackUpdateCheck {
                    instance_id: instance_id.to_string(),
                    provider: "curseforge".to_string(),
                    pack_name: rec.pack_name,
                    project_slug_or_id: rec.mod_id.to_string(),
                    current_version_id: if rec.file_id > 0 {
                        rec.file_id.to_string()
                    } else {
                        String::new()
                    },
                    current_version_number: rec.file_name,
                    versions,
                });
            }
        }
        if let Some(rec) = FtbPackRegistry::load(instance_dir) {
            let owned_ftb;
            let client = if let Some(c) = ftb {
                c
            } else {
                owned_ftb = FtbClient::new(None)?;
                &owned_ftb
            };
            let list = client.get_pack_versions(rec.pack_id).await?;
            let versions = list
                .into_iter()
                .filter(|v| {
                    let low = v.release_type.trim().to_ascii_lowercase();
                    if low == "alpha" {
                        include_alpha
                    } else if low == "beta" {
                        include_beta
                    } else {
                        true
                    }
                })
                .map(|v| ModpackUpdateCandidate {
                    provider: "ftb".to_string(),
                    version_id: v.id.to_string(),
                    version_number: v.name.clone(),
                    name: v.name,
                    version_type: v.release_type.to_ascii_lowercase(),
                    game_versions: if v.mc_version.is_empty() {
                        Vec::new()
                    } else {
                        vec![v.mc_version]
                    },
                    loaders: if v.loader.is_empty() {
                        Vec::new()
                    } else {
                        vec![v.loader]
                    },
                    download_url: String::new(),
                    file_name: String::new(),
                    file_size: 0,
                    sha1: None,
                })
                .collect();
            return Ok(ModpackUpdateCheck {
                instance_id: instance_id.to_string(),
                provider: "ftb".to_string(),
                pack_name: rec.pack_name,
                project_slug_or_id: rec.pack_id.to_string(),
                current_version_id: rec.version_id.to_string(),
                current_version_number: rec.version_name,
                versions,
            });
        }
        if let Some(rec) = AtLauncherPackRegistry::load(instance_dir) {
            let owned_atl;
            let client = if let Some(c) = atlauncher {
                c
            } else {
                owned_atl = AtLauncherClient::new(None)?;
                &owned_atl
            };
            let packs = client.list_packs().await?;
            let found = packs
                .into_iter()
                .find(|p| {
                    (!rec.pack_id.is_empty() && p.id.to_string() == rec.pack_id)
                        || (!rec.safe_name.is_empty()
                            && p.effective_safe_name().eq_ignore_ascii_case(&rec.safe_name))
                })
                .ok_or_else(|| {
                    ProviderError::Io(io::Error::other(
                        "ATLauncher pack could not be found in remote catalog.",
                    ))
                })?;
            let versions = found
                .versions
                .into_iter()
                .filter(|v| {
                    if v.is_dev {
                        include_alpha
                    } else if !v.is_recommended {
                        include_beta
                    } else {
                        true
                    }
                })
                .map(|v| {
                    let vt = if v.is_dev {
                        "alpha"
                    } else if !v.is_recommended {
                        "beta"
                    } else {
                        "release"
                    };
                    ModpackUpdateCandidate {
                        provider: "atlauncher".to_string(),
                        version_id: v.version.clone(),
                        version_number: v.version.clone(),
                        name: v.version.clone(),
                        version_type: vt.to_string(),
                        game_versions: if v.minecraft.is_empty() {
                            Vec::new()
                        } else {
                            vec![v.minecraft]
                        },
                        loaders: Vec::new(),
                        download_url: String::new(),
                        file_name: String::new(),
                        file_size: 0,
                        sha1: None,
                    }
                })
                .collect();
            return Ok(ModpackUpdateCheck {
                instance_id: instance_id.to_string(),
                provider: "atlauncher".to_string(),
                pack_name: rec.pack_name,
                project_slug_or_id: rec.safe_name,
                current_version_id: rec.version_id,
                current_version_number: rec.version_name,
                versions,
            });
        }

        Err(ProviderError::Io(io::Error::other(
            "This instance does not have an official Modrinth, CurseForge, FTB, or ATLauncher modpack manifest.",
        )))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn apply_update(
        paths: &LauncherPaths,
        instance_id: &str,
        target: &ModpackUpdateCandidate,
        downloader: &DownloadClient,
        curseforge: Option<&CurseForgeClient>,
        ftb: Option<&FtbClient>,
        atlauncher: Option<&AtLauncherClient>,
        on_progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackUpdateReport> {
        let inst_dir = paths.instance_dir(instance_id);
        match target.provider.as_str() {
            "curseforge" => {
                let cf = curseforge.ok_or(ProviderError::MissingApiKey)?;
                Self::apply_curseforge_update(
                    paths,
                    instance_id,
                    target,
                    downloader,
                    cf,
                    on_progress,
                )
                .await
            }
            "ftb" => {
                let owned_ftb;
                let client = if let Some(c) = ftb {
                    c
                } else {
                    owned_ftb = FtbClient::new(None)?;
                    &owned_ftb
                };
                Self::apply_ftb_update(paths, instance_id, target, client, downloader, on_progress)
                    .await
            }
            "atlauncher" => {
                let owned_atl;
                let client = if let Some(c) = atlauncher {
                    c
                } else {
                    owned_atl = AtLauncherClient::new(None)?;
                    &owned_atl
                };
                Self::apply_atlauncher_update(
                    paths,
                    instance_id,
                    target,
                    client,
                    downloader,
                    on_progress,
                )
                .await
            }
            _ => {
                if ModrinthPackRegistry::load(&inst_dir).is_some() {
                    Self::apply_modrinth_update(paths, instance_id, target, downloader, on_progress)
                        .await
                } else {
                    Err(ProviderError::Io(io::Error::other(
                        "Unsupported modpack update provider for this instance.",
                    )))
                }
            }
        }
    }

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
                    provider: "modrinth".to_string(),
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
            provider: "modrinth".to_string(),
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

    async fn apply_curseforge_update(
        paths: &LauncherPaths,
        instance_id: &str,
        target: &ModpackUpdateCandidate,
        downloader: &DownloadClient,
        curseforge: &CurseForgeClient,
        on_progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackUpdateReport> {
        let inst_dir = paths.instance_dir(instance_id);
        let mc_dir = paths.minecraft_dir(instance_id);
        let old_rec = CurseForgePackRegistry::load(&inst_dir).unwrap_or_default();

        if target.download_url.trim().is_empty() {
            let file_id = target.version_id.parse::<u64>().unwrap_or(0);
            let url = format!(
                "https://www.curseforge.com/minecraft/modpacks/{}/download/{}",
                old_rec.slug, file_id
            );
            return Err(ProviderError::ManualDownloadRequired {
                mod_id: old_rec.mod_id,
                file_id,
                file_name: target.file_name.clone(),
                website_url: url,
            });
        }

        let pack_dir = paths.cache_root.join("modpacks");
        let _ = fs::create_dir_all(&pack_dir);
        let safe_name = safe_relative_posix(&target.file_name, &format!("{instance_id}.zip"));
        let file_only = Path::new(&safe_name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("update.zip");
        let archive_path = pack_dir.join(file_only);

        if let Some(ref cb) = on_progress {
            cb(0, 1, format!("Downloading {file_only}..."));
        }
        let req = DownloadRequest::new(&target.download_url, &archive_path)
            .with_sha1(target.sha1.clone())
            .with_size(target.file_size);
        downloader.download_file(&req).await?;

        let manifest: CurseForgeManifest = {
            let file = File::open(&archive_path)?;
            let mut archive = ZipArchive::new(file)
                .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
            let entry = archive
                .by_name("manifest.json")
                .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;
            serde_json::from_reader(entry)?
        };

        let store = InstanceStore::new(paths);
        let game_version = manifest.minecraft.version.clone();
        let mut cf_loaders = manifest.minecraft.mod_loaders.clone();
        if cf_loaders.is_empty() {
            if let Some(single) = manifest.minecraft.mod_loader.clone() {
                cf_loaders.push(single);
            }
        }
        let loader = parse_cf_loader(&cf_loaders, &game_version);
        let mods_dir = mc_dir.join("mods");
        let _ = fs::create_dir_all(&mods_dir);

        let file_ids: Vec<u64> = manifest
            .files
            .iter()
            .filter(|f| f.required)
            .map(|f| f.file_id)
            .collect();
        let resolved = if file_ids.is_empty() {
            Vec::new()
        } else {
            curseforge.get_files_batch(&file_ids).await?
        };
        let mut requests = Vec::new();
        let mut new_managed = Vec::new();
        for cf_file in resolved {
            let rel =
                safe_relative_posix(&format!("mods/{}", cf_file.file_name), &cf_file.file_name);
            new_managed.push(rel);
            if let Ok(dl_url) = cf_file.require_download_url(None) {
                let dest = mods_dir.join(&cf_file.file_name);
                requests.push(
                    DownloadRequest::new(dl_url, dest)
                        .with_sha1(cf_file.sha1())
                        .with_size(cf_file.file_length),
                );
            }
        }

        let removed_files =
            prune_obsolete_managed_files(&mc_dir, &old_rec.managed_files, &new_managed);
        let downloaded_files = requests.len();
        let prog_cb = on_progress.clone();
        downloader
            .download_batch(requests, DOWNLOAD_CONCURRENCY, move |done, tot| {
                if let Some(ref cb) = prog_cb {
                    cb(
                        done,
                        tot,
                        format!("Updating CurseForge mods {done}/{tot}..."),
                    );
                }
            })
            .await?;

        let override_prefix = format!(
            "{}/",
            manifest
                .overrides
                .as_deref()
                .unwrap_or("overrides")
                .trim_matches('/')
        );
        let extracted_overrides = extract_overrides_filtered(
            &archive_path,
            &[&override_prefix],
            &mc_dir,
            PRESERVED_OVERRIDE_PREFIXES,
        )?;
        store
            .set_runtime_profile(instance_id, &game_version, loader.clone())
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;

        let new_file_id = target.version_id.parse::<u64>().unwrap_or(old_rec.file_id);
        CurseForgePackRegistry::save(
            &inst_dir,
            &CurseForgePackRecord {
                source: "curseforge".to_string(),
                mod_id: old_rec.mod_id,
                slug: old_rec.slug,
                pack_name: if manifest.name.trim().is_empty() {
                    old_rec.pack_name
                } else {
                    manifest.name
                },
                file_id: new_file_id,
                file_name: target.file_name.clone(),
                managed_files: new_managed,
                updated_at: String::new(),
            },
        )?;

        Ok(ModpackUpdateReport {
            instance_id: instance_id.to_string(),
            new_version_id: target.version_id.clone(),
            new_version_number: target.version_number.clone(),
            game_version,
            loader,
            removed_files,
            downloaded_files,
            extracted_overrides,
        })
    }

    async fn apply_ftb_update(
        paths: &LauncherPaths,
        instance_id: &str,
        target: &ModpackUpdateCandidate,
        ftb: &FtbClient,
        downloader: &DownloadClient,
        on_progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackUpdateReport> {
        let inst_dir = paths.instance_dir(instance_id);
        let mc_dir = paths.minecraft_dir(instance_id);
        let old_rec = FtbPackRegistry::load(&inst_dir).ok_or_else(|| {
            ProviderError::Io(io::Error::other("Missing .mcw/ftb-pack.json metadata."))
        })?;
        let target_ver_id = target
            .version_id
            .parse::<u64>()
            .map_err(|_| ProviderError::Io(io::Error::other("Invalid FTB target version ID.")))?;

        if let Some(ref cb) = on_progress {
            cb(0, 1, format!("Fetching FTB manifest v{target_ver_id}..."));
        }
        let ver_raw = ftb
            .cached_get(&format!(
                "{FTB_API_BASE}/modpack/{}/{target_ver_id}",
                old_rec.pack_id
            ))
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

        let mut requests = Vec::new();
        let mut new_managed_entries = Vec::new();
        let mut new_managed_paths = Vec::new();
        if let Some(files) = ver_json.get("files").and_then(|v| v.as_array()) {
            for f in files {
                if f.get("serveronly")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    continue;
                }
                let url = f.get("url").and_then(|v| v.as_str()).unwrap_or("");
                let name = f.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if url.is_empty() || name.is_empty() {
                    continue;
                }
                let rel_dir = f
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("./")
                    .trim_start_matches("./");
                let rel_raw = if rel_dir.is_empty() {
                    name.to_string()
                } else {
                    format!("{}/{name}", rel_dir.trim_end_matches('/'))
                };
                let rel = safe_relative_posix(&rel_raw, name);
                let rel_lower = rel.to_ascii_lowercase();
                if PRESERVED_OVERRIDE_PREFIXES
                    .iter()
                    .any(|p| rel_lower == *p || rel_lower.starts_with(p))
                {
                    continue;
                }
                if let Ok(dest) = safe_join_under(&mc_dir, &rel) {
                    let sha1 = f
                        .get("sha1")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(String::from);
                    let size = f.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                    new_managed_paths.push(rel.clone());
                    new_managed_entries.push(FtbManagedFile {
                        file_id: f.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
                        file_name: name.to_string(),
                        path: rel,
                        sha1: sha1.clone().unwrap_or_default(),
                        size,
                        urls: vec![url.to_string()],
                        optional: f.get("optional").and_then(|v| v.as_bool()).unwrap_or(false),
                        client_only: f
                            .get("clientonly")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                        file_type: f
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
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

        let old_paths: Vec<String> = old_rec
            .managed_files
            .iter()
            .map(|m| m.path.clone())
            .collect();
        let removed_files = prune_obsolete_managed_files(&mc_dir, &old_paths, &new_managed_paths);
        let downloaded_files = requests.len();
        let prog_cb = on_progress.clone();
        downloader
            .download_batch(requests, DOWNLOAD_CONCURRENCY, move |done, tot| {
                if let Some(ref cb) = prog_cb {
                    cb(
                        done,
                        tot,
                        format!("Updating FTB pack files ({done}/{tot})..."),
                    );
                }
            })
            .await?;

        let store = InstanceStore::new(paths);
        store
            .set_runtime_profile(instance_id, &game_version, loader.clone())
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;

        FtbPackRegistry::save(
            &inst_dir,
            &FtbPackRecord {
                schema_version: 2,
                source: "ftb".to_string(),
                pack_id: old_rec.pack_id,
                pack_name: old_rec.pack_name,
                version_id: target_ver_id,
                version_name: target.version_number.clone(),
                managed_files: new_managed_entries,
                updated_at: String::new(),
            },
        )?;

        Ok(ModpackUpdateReport {
            instance_id: instance_id.to_string(),
            new_version_id: target.version_id.clone(),
            new_version_number: target.version_number.clone(),
            game_version,
            loader,
            removed_files,
            downloaded_files,
            extracted_overrides: 0,
        })
    }

    async fn apply_atlauncher_update(
        paths: &LauncherPaths,
        instance_id: &str,
        target: &ModpackUpdateCandidate,
        atl: &AtLauncherClient,
        downloader: &DownloadClient,
        on_progress: Option<ModpackProgressCallback>,
    ) -> Result<ModpackUpdateReport> {
        let inst_dir = paths.instance_dir(instance_id);
        let mc_dir = paths.minecraft_dir(instance_id);
        let old_rec = AtLauncherPackRegistry::load(&inst_dir).ok_or_else(|| {
            ProviderError::Io(io::Error::other(
                "Missing .mcw/atlauncher-pack.json metadata.",
            ))
        })?;
        let safe_name = if !old_rec.safe_name.is_empty() {
            old_rec.safe_name.clone()
        } else {
            old_rec.pack_name.replace(' ', "")
        };
        if let Some(ref cb) = on_progress {
            cb(
                0,
                1,
                format!("Fetching ATLauncher Configs.json v{}...", target.version_id),
            );
        }
        let cfg_url = format!(
            "{ATLAUNCHER_CDN_BASE}packs/{safe_name}/versions/{}/Configs.json",
            target.version_id
        );
        let resp = atl.http.get(&cfg_url).send().await?;
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
            .unwrap_or("1.20.1")
            .to_string();
        let loader = parse_atlauncher_loader(&cfg, &game_version);
        let mods_dir = mc_dir.join("mods");
        let _ = fs::create_dir_all(&mods_dir);

        let mut requests = Vec::new();
        let mut new_managed_entries = Vec::new();
        let mut new_managed_paths = Vec::new();
        if let Some(mods) = cfg.get("mods").and_then(|v| v.as_array()) {
            for m in mods {
                if !m.get("client").and_then(|v| v.as_bool()).unwrap_or(true) {
                    continue;
                }
                let mod_type = m.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let mod_name = m.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if is_atlauncher_loader_entry(mod_type, mod_name) {
                    continue;
                }
                let is_optional = m.get("optional").and_then(|v| v.as_bool()).unwrap_or(false);
                if is_optional && !m.get("selected").and_then(|v| v.as_bool()).unwrap_or(false) {
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
                let rel = safe_relative_posix(&format!("mods/{file}"), file);
                new_managed_paths.push(rel.clone());
                let dest = mods_dir.join(file);
                let sha1 = m
                    .get("sha1")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(String::from);
                let size = m.get("filesize").and_then(|v| v.as_u64()).unwrap_or(0);
                new_managed_entries.push(AtLauncherManagedFile {
                    file_id: file.to_string(),
                    file_name: file.to_string(),
                    name: if mod_name.is_empty() {
                        file.to_string()
                    } else {
                        mod_name.to_string()
                    },
                    path: rel,
                    sha1: sha1.clone().unwrap_or_default(),
                    size,
                    urls: vec![full_url.clone()],
                    optional: is_optional,
                    client_only: true,
                    provider: "atlauncher".to_string(),
                });
                requests.push(
                    DownloadRequest::new(full_url, dest)
                        .with_sha1(sha1)
                        .with_size(size),
                );
            }
        }

        let old_paths: Vec<String> = old_rec
            .managed_files
            .iter()
            .map(|m| m.path.clone())
            .collect();
        let removed_files = prune_obsolete_managed_files(&mc_dir, &old_paths, &new_managed_paths);
        let downloaded_files = requests.len();
        let prog_cb = on_progress.clone();
        downloader
            .download_batch(requests, DOWNLOAD_CONCURRENCY, move |done, tot| {
                if let Some(ref cb) = prog_cb {
                    cb(
                        done,
                        tot,
                        format!("Updating ATLauncher mods ({done}/{tot})..."),
                    );
                }
            })
            .await?;

        let store = InstanceStore::new(paths);
        store
            .set_runtime_profile(instance_id, &game_version, loader.clone())
            .map_err(|e| ProviderError::Io(io::Error::other(e.to_string())))?;

        AtLauncherPackRegistry::save(
            &inst_dir,
            &AtLauncherPackRecord {
                schema_version: 1,
                source: "atlauncher".to_string(),
                pack_id: old_rec.pack_id,
                pack_name: old_rec.pack_name,
                safe_name,
                version_id: target.version_id.clone(),
                version_name: target.version_number.clone(),
                managed_files: new_managed_entries,
                updated_at: String::new(),
            },
        )?;

        Ok(ModpackUpdateReport {
            instance_id: instance_id.to_string(),
            new_version_id: target.version_id.clone(),
            new_version_number: target.version_number.clone(),
            game_version,
            loader,
            removed_files,
            downloaded_files,
            extracted_overrides: 0,
        })
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
        let removed_files =
            prune_obsolete_managed_files(&mc_dir, &old_record.managed_files, &new_managed);

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
