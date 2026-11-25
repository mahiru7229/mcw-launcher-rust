/// Direct port of MCW Launcher Python's:
/// - `ModProvenanceRegistry` (`src/core/mod/mod_provenance_registry.py` & `modrinth_registry.py`)
/// - `ModrinthModUpdateManager` (`src/core/modrinth/modrinth_mod_update_manager.py`)
/// - `LanHostingManager` (`src/core/lan/lan_hosting_manager.py` — `manual` vs `e4mc` tunnel provider)
use crate::curseforge::CurseForgeClient;
use crate::modrinth::ModrinthClient;
use crate::{ProviderError, Result};
use mcw_net::{DownloadClient, DownloadRequest};
use mcw_storage::atomic_write_json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

const PROVENANCE_REL_PATH: &str = ".mcw/mod-provenance.json";
pub const LAN_HOSTING_MANAGED_BY: &str = "mcw_lan_hosting";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrackedModEntry {
    pub project_id: String,
    /// `"modrinth"` or `"curseforge"`.
    pub provider: String,
    pub title: String,
    pub file_name: String,
    pub version_id: String,
    pub version_number: String,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub managed_by: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModProvenanceFile {
    #[serde(default)]
    pub mods: HashMap<String, TrackedModEntry>,
}

pub struct ModProvenanceRegistry;

impl ModProvenanceRegistry {
    pub fn load(instance_dir: &Path) -> ModProvenanceFile {
        let path = instance_dir.join(PROVENANCE_REL_PATH);
        let Ok(raw) = fs::read_to_string(&path) else {
            return ModProvenanceFile::default();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    pub fn save(instance_dir: &Path, data: &ModProvenanceFile) -> Result<()> {
        let path = instance_dir.join(PROVENANCE_REL_PATH);
        atomic_write_json(&path, data)
            .map_err(|e| ProviderError::Io(std::io::Error::other(e.to_string())))
    }

    pub fn record_install(instance_dir: &Path, entry: TrackedModEntry) -> Result<()> {
        let mut data = Self::load(instance_dir);
        let key = format!("{}:{}", entry.provider, entry.project_id);
        // Preserve existing `locked` status if re-recording
        let was_locked = data.mods.get(&key).map(|m| m.locked).unwrap_or(false);
        let mut updated = entry;
        updated.locked = updated.locked || was_locked;
        data.mods.insert(key, updated);
        Self::save(instance_dir, &data)
    }

    pub fn set_locked_by_filename(
        instance_dir: &Path,
        file_name: &str,
        locked: bool,
    ) -> Result<bool> {
        let clean = file_name.trim_end_matches(".disabled");
        let mut data = Self::load(instance_dir);
        let mut found = false;
        for entry in data.mods.values_mut() {
            if entry.file_name.trim_end_matches(".disabled") == clean {
                entry.locked = locked;
                found = true;
            }
        }
        if found {
            Self::save(instance_dir, &data)?;
        }
        Ok(found)
    }

    pub fn is_locked_by_filename(instance_dir: &Path, file_name: &str) -> bool {
        let clean = file_name.trim_end_matches(".disabled");
        let data = Self::load(instance_dir);
        data.mods
            .values()
            .any(|e| e.file_name.trim_end_matches(".disabled") == clean && e.locked)
    }
}

#[derive(Debug, Clone)]
pub struct ModUpdateCandidate {
    pub key: String,
    pub project_id: String,
    pub provider: String,
    pub title: String,
    pub current_file_name: String,
    pub current_version_id: String,
    pub current_version_number: String,
    pub latest_version_id: String,
    pub latest_version_number: String,
    pub latest_file_name: String,
    pub download_url: String,
    pub file_size: u64,
    pub sha1: Option<String>,
    pub locked: bool,
    pub update_available: bool,
}

pub struct ModUpdateManager;

impl ModUpdateManager {
    pub async fn check_updates(
        instance_dir: &Path,
        mc_dir: &Path,
        game_version: &str,
        loader: &str,
        modrinth: &ModrinthClient,
        curseforge: Option<&CurseForgeClient>,
    ) -> Vec<ModUpdateCandidate> {
        let reg = ModProvenanceRegistry::load(instance_dir);
        let mods_dir = mc_dir.join("mods");
        let mut candidates = Vec::new();

        for (key, entry) in reg.mods {
            let enabled_path = mods_dir.join(&entry.file_name);
            let disabled_path = mods_dir.join(format!("{}.disabled", entry.file_name));
            if !enabled_path.is_file() && !disabled_path.is_file() {
                continue;
            }

            if entry.provider == "modrinth" {
                if let Ok(versions) = modrinth
                    .list_versions(&entry.project_id, Some(loader), Some(game_version))
                    .await
                {
                    if let Some(latest) = versions.first() {
                        if let Some(primary) = latest.primary_file() {
                            let update_available = latest.id != entry.version_id
                                && latest.version_number != entry.version_number;
                            candidates.push(ModUpdateCandidate {
                                key,
                                project_id: entry.project_id,
                                provider: entry.provider,
                                title: entry.title,
                                current_file_name: entry.file_name,
                                current_version_id: entry.version_id,
                                current_version_number: entry.version_number,
                                latest_version_id: latest.id.clone(),
                                latest_version_number: latest.version_number.clone(),
                                latest_file_name: primary.filename.clone(),
                                download_url: primary.url.clone(),
                                file_size: primary.size,
                                sha1: primary.hashes.get("sha1").cloned(),
                                locked: entry.locked,
                                update_available,
                            });
                        }
                    }
                }
            } else if entry.provider == "curseforge" {
                if let (Some(cf), Ok(mod_id)) = (curseforge, entry.project_id.parse::<u64>()) {
                    if let Ok(files) = cf
                        .get_mod_files(mod_id, Some(game_version), Some(loader))
                        .await
                    {
                        if let Some(latest) = files.first() {
                            if let Ok(dl_url) = latest.require_download_url(None) {
                                let latest_id_str = latest.id.to_string();
                                let update_available = latest_id_str != entry.version_id
                                    && latest.file_name != entry.file_name;
                                candidates.push(ModUpdateCandidate {
                                    key,
                                    project_id: entry.project_id,
                                    provider: entry.provider,
                                    title: entry.title,
                                    current_file_name: entry.file_name,
                                    current_version_id: entry.version_id,
                                    current_version_number: entry.version_number,
                                    latest_version_id: latest_id_str,
                                    latest_version_number: if latest.display_name.is_empty() {
                                        latest.file_name.clone()
                                    } else {
                                        latest.display_name.clone()
                                    },
                                    latest_file_name: latest.file_name.clone(),
                                    download_url: dl_url,
                                    file_size: latest.file_length,
                                    sha1: latest.sha1(),
                                    locked: entry.locked,
                                    update_available,
                                });
                            }
                        }
                    }
                }
            }
        }

        candidates.sort_by(|a, b| {
            b.update_available
                .cmp(&a.update_available)
                .then_with(|| a.locked.cmp(&b.locked))
                .then_with(|| {
                    a.title
                        .to_ascii_lowercase()
                        .cmp(&b.title.to_ascii_lowercase())
                })
        });
        candidates
    }

    /// Apply updates for all unlocked `update_available` candidates.
    pub async fn apply_updates(
        instance_dir: &Path,
        mc_dir: &Path,
        candidates: &[ModUpdateCandidate],
        downloader: &DownloadClient,
    ) -> Result<usize> {
        let mods_dir = mc_dir.join("mods");
        fs::create_dir_all(&mods_dir)?;
        let mut updated_count = 0usize;

        for cand in candidates {
            if !cand.update_available || cand.locked {
                continue;
            }
            let was_disabled = mods_dir
                .join(format!("{}.disabled", cand.current_file_name))
                .is_file()
                && !mods_dir.join(&cand.current_file_name).is_file();
            let dest_name = if was_disabled {
                format!("{}.disabled", cand.latest_file_name)
            } else {
                cand.latest_file_name.clone()
            };
            let dest_path = mods_dir.join(&dest_name);
            let req = DownloadRequest::new(&cand.download_url, &dest_path)
                .with_sha1(cand.sha1.clone())
                .with_size(cand.file_size);
            downloader.download_file(&req).await?;

            // Remove old file if filename changed
            if cand.current_file_name != cand.latest_file_name {
                let _ = fs::remove_file(mods_dir.join(&cand.current_file_name));
                let _ =
                    fs::remove_file(mods_dir.join(format!("{}.disabled", cand.current_file_name)));
            }

            let _ = ModProvenanceRegistry::record_install(
                instance_dir,
                TrackedModEntry {
                    project_id: cand.project_id.clone(),
                    provider: cand.provider.clone(),
                    title: cand.title.clone(),
                    file_name: cand.latest_file_name.clone(),
                    version_id: cand.latest_version_id.clone(),
                    version_number: cand.latest_version_number.clone(),
                    locked: false,
                    managed_by: None,
                },
            );
            updated_count += 1;
        }

        Ok(updated_count)
    }

    pub async fn check_content_pack_updates(
        mc_dir: &Path,
        kind_folder: &str,
        loaders: &[&str],
        mc_version: &str,
        modrinth: &ModrinthClient,
    ) -> Vec<ContentPackUpdateCandidate> {
        let dir = mc_dir.join(kind_folder);
        let Ok(entries) = fs::read_dir(&dir) else {
            return Vec::new();
        };

        let mut file_hashes: Vec<(String, String)> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let low = name.to_ascii_lowercase();
            if !(low.ends_with(".zip") || low.ends_with(".zip.disabled")) {
                continue;
            }
            if let Ok(sha1) = mcw_net::compute_sha1(&path) {
                file_hashes.push((name, sha1.to_ascii_lowercase()));
            }
            if file_hashes.len() >= 200 {
                break;
            }
        }
        if file_hashes.is_empty() {
            return Vec::new();
        }

        let mut updates: HashMap<String, crate::modrinth::ModrinthVersion> = HashMap::new();
        for chunk in file_hashes.chunks(50) {
            let hashes: Vec<String> = chunk.iter().map(|(_, h)| h.clone()).collect();
            if let Ok(map) = modrinth
                .check_version_files_update(&hashes, loaders, mc_version)
                .await
            {
                updates.extend(map);
            }
        }

        build_content_pack_update_candidates(&file_hashes, &updates)
    }

    pub async fn apply_content_pack_updates(
        mc_dir: &Path,
        kind_folder: &str,
        candidates: &[ContentPackUpdateCandidate],
        downloader: &DownloadClient,
    ) -> Result<usize> {
        let dir = mc_dir.join(kind_folder);
        fs::create_dir_all(&dir)?;
        let mut updated = 0usize;

        for cand in candidates {
            if !cand.update_available || cand.download_url.trim().is_empty() {
                continue;
            }
            let was_disabled = cand.file_name.ends_with(".disabled");
            let target_name = if was_disabled {
                format!("{}.disabled", cand.latest_file_name)
            } else {
                cand.latest_file_name.clone()
            };
            let dest_path = dir.join(&target_name);
            let req = DownloadRequest::new(&cand.download_url, &dest_path)
                .with_sha1(cand.sha1.clone())
                .with_size(cand.file_size);
            downloader.download_file(&req).await?;

            if cand.file_name != target_name {
                let _ = fs::remove_file(dir.join(&cand.file_name));
            }
            updated += 1;
        }

        Ok(updated)
    }
}

#[derive(Debug, Clone)]
pub struct ContentPackUpdateCandidate {
    pub file_name: String,
    pub project_id: String,
    pub current_version_number: String,
    pub latest_version_number: String,
    pub latest_file_name: String,
    pub download_url: String,
    pub sha1: Option<String>,
    pub file_size: u64,
    pub update_available: bool,
}

pub fn build_content_pack_update_candidates(
    file_hashes: &[(String, String)],
    updates: &HashMap<String, crate::modrinth::ModrinthVersion>,
) -> Vec<ContentPackUpdateCandidate> {
    let mut candidates = Vec::new();
    for (file_name, local_sha1) in file_hashes {
        let local_low = local_sha1.trim().to_ascii_lowercase();
        let Some(latest) = updates.get(&local_low) else {
            continue;
        };
        let Some(primary) = latest.primary_file() else {
            continue;
        };
        let remote_sha1 = primary
            .hashes
            .get("sha1")
            .map(|s| s.trim().to_ascii_lowercase());
        let update_available = remote_sha1.as_deref() != Some(local_low.as_str());
        candidates.push(ContentPackUpdateCandidate {
            file_name: file_name.clone(),
            project_id: latest.project_id.clone(),
            current_version_number: String::new(),
            latest_version_number: latest.version_number.clone(),
            latest_file_name: primary.filename.clone(),
            download_url: primary.url.clone(),
            sha1: remote_sha1,
            file_size: primary.size,
            update_available,
        });
    }
    candidates
}

pub struct LanHostingManager;

impl LanHostingManager {
    pub const CONNECTION_MANUAL: &'static str = "manual";
    pub const CONNECTION_E4MC: &'static str = "e4mc";

    pub fn normalize_connection_provider(raw: &str) -> &'static str {
        if raw.trim().eq_ignore_ascii_case("e4mc") {
            Self::CONNECTION_E4MC
        } else {
            Self::CONNECTION_MANUAL
        }
    }

    /// Prepare LAN tunnel provider (`manual` vs `e4mc`) before launching or saving settings
    /// (1:1 port of Python `LanHostingManager.prepare`).
    pub async fn prepare(
        instance_dir: &Path,
        mc_dir: &Path,
        game_version: &str,
        loader: &str,
        connection_provider: &str,
        modrinth: &ModrinthClient,
        downloader: &DownloadClient,
    ) -> Result<String> {
        let provider = Self::normalize_connection_provider(connection_provider);
        let mods_dir = mc_dir.join("mods");
        fs::create_dir_all(&mods_dir)?;

        // Always disable legacy `mcwifipnp` JARs if present
        Self::set_mod_prefix_enabled(&mods_dir, "mcwifipnp", false);

        if provider == Self::CONNECTION_MANUAL {
            Self::set_mod_prefix_enabled(&mods_dir, "e4mc", false);
            return Ok(
                "LAN connection set to Manual (disabled managed e4mc tunnel mod if present)."
                    .to_string(),
            );
        }

        let clean_loader = loader.trim().to_ascii_lowercase();
        if !matches!(
            clean_loader.as_str(),
            "fabric" | "quilt" | "forge" | "neoforge"
        ) {
            return Err(ProviderError::Io(std::io::Error::other(
                "e4mc tunnel provider requires a Fabric, Quilt, Forge, or NeoForge instance.",
            )));
        }

        // Re-enable existing e4mc JAR if disabled, or download from Modrinth
        if Self::set_mod_prefix_enabled(&mods_dir, "e4mc", true) {
            return Ok("Re-enabled installed e4mc tunnel mod.".to_string());
        }

        let installed = modrinth
            .install_with_dependencies("e4mc", game_version, &clean_loader, &mods_dir, downloader)
            .await?;
        if let Some(first) = installed.first() {
            let fname = first
                .file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("e4mc.jar")
                .to_string();
            let _ = ModProvenanceRegistry::record_install(
                instance_dir,
                TrackedModEntry {
                    project_id: first.project_id.clone(),
                    provider: "modrinth".to_string(),
                    title: "e4mc".to_string(),
                    file_name: fname.clone(),
                    version_id: first.version_number.clone(),
                    version_number: first.version_number.clone(),
                    locked: false,
                    managed_by: Some(LAN_HOSTING_MANAGED_BY.to_string()),
                },
            );
            Ok(format!(
                "Installed e4mc tunnel mod ({fname}) from Modrinth."
            ))
        } else {
            Ok("Prepared e4mc tunnel mod.".to_string())
        }
    }

    fn set_mod_prefix_enabled(mods_dir: &Path, prefix: &str, enable: bool) -> bool {
        let Ok(entries) = fs::read_dir(mods_dir) else {
            return false;
        };
        let mut matched = false;
        let p_low = prefix.to_ascii_lowercase();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let low = name.to_ascii_lowercase();
            if !low.starts_with(&p_low) {
                continue;
            }
            if enable {
                if let Some(stripped) = name.strip_suffix(".disabled") {
                    let target = mods_dir.join(stripped);
                    let _ = fs::rename(&path, &target);
                    matched = true;
                } else if low.ends_with(".jar") {
                    matched = true;
                }
            } else if low.ends_with(".jar") {
                let target = mods_dir.join(format!("{name}.disabled"));
                let _ = fs::rename(&path, &target);
                matched = true;
            }
        }
        matched
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn provenance_registry_records_and_toggles_version_lock() {
        let dir = tempdir().unwrap();
        ModProvenanceRegistry::record_install(
            dir.path(),
            TrackedModEntry {
                project_id: "AANobbMI".to_string(),
                provider: "modrinth".to_string(),
                title: "Sodium".to_string(),
                file_name: "sodium-fabric-0.6.0.jar".to_string(),
                version_id: "v1".to_string(),
                version_number: "0.6.0".to_string(),
                locked: false,
                managed_by: None,
            },
        )
        .unwrap();

        assert!(!ModProvenanceRegistry::is_locked_by_filename(
            dir.path(),
            "sodium-fabric-0.6.0.jar"
        ));
        ModProvenanceRegistry::set_locked_by_filename(dir.path(), "sodium-fabric-0.6.0.jar", true)
            .unwrap();
        assert!(ModProvenanceRegistry::is_locked_by_filename(
            dir.path(),
            "sodium-fabric-0.6.0.jar.disabled"
        ));
    }

    #[test]
    fn detects_content_pack_update_from_modrinth_hash_response() {
        use crate::modrinth::{ModrinthFile, ModrinthVersion};

        let local_hash_old = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();
        let local_hash_cur = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string();
        let file_hashes = vec![
            ("Complementary.zip".to_string(), local_hash_old.clone()),
            ("Faithful.zip".to_string(), local_hash_cur.clone()),
        ];

        let mut updates = HashMap::new();
        let mut h_new = HashMap::new();
        h_new.insert(
            "sha1".to_string(),
            "cccccccccccccccccccccccccccccccccccccccc".to_string(),
        );
        updates.insert(
            local_hash_old,
            ModrinthVersion {
                id: "v2".to_string(),
                project_id: "HVnmMxH1".to_string(),
                name: "r5.3".to_string(),
                version_number: "r5.3".to_string(),
                version_type: "release".to_string(),
                game_versions: vec!["1.21.4".to_string()],
                loaders: vec!["iris".to_string()],
                files: vec![ModrinthFile {
                    url:
                        "https://cdn.modrinth.com/data/HVnmMxH1/versions/v2/Complementary_r5.3.zip"
                            .to_string(),
                    filename: "Complementary_r5.3.zip".to_string(),
                    primary: true,
                    size: 12345,
                    hashes: h_new,
                }],
                dependencies: Vec::new(),
            },
        );
        let mut h_same = HashMap::new();
        h_same.insert("sha1".to_string(), local_hash_cur.clone());
        updates.insert(
            local_hash_cur,
            ModrinthVersion {
                id: "v1".to_string(),
                project_id: "FAITHFUL".to_string(),
                name: "1.21".to_string(),
                version_number: "1.21".to_string(),
                version_type: "release".to_string(),
                game_versions: vec!["1.21.4".to_string()],
                loaders: vec!["minecraft".to_string()],
                files: vec![ModrinthFile {
                    url: "https://cdn.modrinth.com/data/FAITHFUL/versions/v1/Faithful.zip"
                        .to_string(),
                    filename: "Faithful.zip".to_string(),
                    primary: true,
                    size: 456,
                    hashes: h_same,
                }],
                dependencies: Vec::new(),
            },
        );

        let result = build_content_pack_update_candidates(&file_hashes, &updates);
        assert_eq!(result.len(), 2);
        let comp = result
            .iter()
            .find(|c| c.file_name == "Complementary.zip")
            .unwrap();
        assert!(comp.update_available);
        assert_eq!(comp.latest_version_number, "r5.3");
        let faith = result
            .iter()
            .find(|c| c.file_name == "Faithful.zip")
            .unwrap();
        assert!(!faith.update_available);
    }
}
