use crate::assets::AssetManager;
use crate::forge::{ForgeFamily, ForgeLoaderService};
use crate::libraries::LibraryManager;
use crate::manifest::VersionManifestService;
use crate::modloader::{LoaderKind, ModLoaderService};
use crate::optifine::OptiFineManager;
use crate::{MinecraftError, Result};
use mcw_models::instance::Instance;
use mcw_models::progress::{ProgressEvent, ProgressSink, ProgressStage};
use mcw_net::{verify_file, DownloadClient, DownloadRequest};
use mcw_runtime::{detect_mods_required_java, JavaMajorPolicy, JavaProvisioner, JavaResolver};
use mcw_storage::{atomic_write_text, InstanceRunLock, InstanceStore, LauncherPaths};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepairReport {
    pub instance_id: String,
    pub version_id: String,
    #[serde(default)]
    pub mod_loader: String,
    #[serde(default)]
    pub version_json_repaired: bool,
    pub client_repaired: bool,
    pub libraries_verified: bool,
    pub assets_verified: bool,
    pub natives_reextracted: bool,
    #[serde(default)]
    pub crash_flag_cleared: bool,
}

pub struct RepairService<'a> {
    paths: &'a LauncherPaths,
    client: &'a DownloadClient,
}

impl<'a> RepairService<'a> {
    pub fn new(paths: &'a LauncherPaths, client: &'a DownloadClient) -> Self {
        Self { paths, client }
    }

    /// Force-verify and transactionally repair an instance's `version.json`, client JAR,
    /// mod-loader profile (Vanilla, Fabric, Quilt, Forge, NeoForge), libraries, natives,
    /// and missing assets — with full backup + rollback on failure.
    ///
    /// Strictly preserves `saves/`, `mods/`, `resourcepacks/`, `shaderpacks/`, and `options.txt`,
    /// and does NOT alter mod enabled/disabled state.
    pub async fn repair_instance(
        &self,
        instance: &Instance,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<RepairReport> {
        let _run_lock = InstanceRunLock::acquire(self.paths, &instance.instance_id)?;

        progress.on_progress(ProgressEvent {
            stage: ProgressStage::FetchingManifest,
            message: format!("Refreshing Mojang manifest for {}...", instance.version_id),
            current: 0,
            total: 1,
        });

        let manifest_svc = VersionManifestService::new(self.paths, self.client);
        let entry = manifest_svc
            .find_version(&instance.version_id, true)
            .await?;

        Self::repair_from_manifest_entry(self.paths, self.client, instance, &entry, progress).await
    }

    /// Transactional repair implementation separated from manifest lookup for deterministic testing.
    pub async fn repair_from_manifest_entry(
        paths: &LauncherPaths,
        client: &DownloadClient,
        instance: &Instance,
        entry: &mcw_models::minecraft::VersionManifestEntry,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<RepairReport> {
        let base_id = entry.id.clone();
        let ver_json_path = paths.version_json(&base_id);
        let client_jar = paths.client_jar(&base_id);
        let vanilla_natives = paths.natives_dir(&base_id);

        // Snapshot backup of core metadata/binaries before touching anything.
        let backup_root = paths
            .cache_root
            .join("security")
            .join(format!("repair_backup_{}", instance.instance_id));
        if backup_root.exists() {
            let _ = fs::remove_dir_all(&backup_root);
        }
        fs::create_dir_all(&backup_root)?;

        let backup_json = backup_root.join("version.json.bak");
        let had_json = ver_json_path.is_file();
        if had_json {
            fs::copy(&ver_json_path, &backup_json)?;
        }
        let backup_jar = backup_root.join("client.jar.bak");
        let had_jar = client_jar.is_file();
        if had_jar {
            fs::copy(&client_jar, &backup_jar)?;
        }
        let backup_natives = backup_root.join("natives_base");
        let had_base_natives = vanilla_natives.is_dir();
        if had_base_natives {
            copy_dir_recursive(&vanilla_natives, &backup_natives)?;
        }

        let staging_json = backup_root.join("candidate.version.json");
        let res: Result<RepairReport> = async {
            // 1. Download & SHA-1 verify candidate Mojang version.json into staging
            progress.on_progress(ProgressEvent {
                stage: ProgressStage::DownloadingMetadata,
                message: format!("Verifying metadata for {}...", base_id),
                current: 0,
                total: 1,
            });
            let meta_req = DownloadRequest::new(&entry.url, &staging_json)
                .with_sha1(entry.sha1.clone())
                .with_size(entry.size.unwrap_or(0));
            client.download_file(&meta_req).await?;

            let new_raw = fs::read_to_string(&staging_json)?;
            let base_meta: mcw_models::minecraft::VersionMetadata = serde_json::from_str(&new_raw)?;
            let old_raw = fs::read_to_string(&ver_json_path).ok();
            let version_json_repaired = old_raw.as_deref() != Some(new_raw.as_str());
            if let Some(parent) = ver_json_path.parent() {
                fs::create_dir_all(parent)?;
            }
            atomic_write_text(&ver_json_path, &new_raw)?;

            // 2. Verify / download client JAR safely (via staged temp file if missing/corrupt)
            let client_artifact = base_meta
                .downloads
                .as_ref()
                .and_then(|d| d.client.as_ref())
                .ok_or_else(|| {
                    MinecraftError::InvalidMetadata(format!(
                        "Version '{base_id}' is missing client download metadata"
                    ))
                })?;
            let client_was_valid = client_jar.is_file()
                && verify_file(
                    &client_jar,
                    client_artifact.sha1.as_deref(),
                    client_artifact.size,
                )
                .is_ok();
            if !client_was_valid {
                progress.on_progress(ProgressEvent {
                    stage: ProgressStage::DownloadingClient,
                    message: format!("Repairing client JAR ({base_id})..."),
                    current: 0,
                    total: 1,
                });
                let staged_jar = backup_root.join(format!("{base_id}.jar.part"));
                let jar_req = DownloadRequest::new(&client_artifact.url, &staged_jar)
                    .with_sha1(client_artifact.sha1.clone())
                    .with_size(client_artifact.size);
                client.download_file(&jar_req).await?;
                if let Some(parent) = client_jar.parent() {
                    fs::create_dir_all(parent)?;
                }
                if client_jar.exists() {
                    let _ = fs::remove_file(&client_jar);
                }
                fs::rename(&staged_jar, &client_jar)?;
            }

            // 3. Reconcile mod-loader metadata cleanly across ALL loaders (Fabric, Quilt, Forge, NeoForge)
            let loader_name = instance.mod_loader.name.trim().to_ascii_lowercase();
            let clean_loader_ver = instance
                .mod_loader
                .version
                .trim()
                .trim_start_matches(&format!("{loader_name}-"))
                .trim_start_matches(&format!("{base_id}-"))
                .to_string();

            let metadata = if let Some(kind) = LoaderKind::parse(&instance.mod_loader.name) {
                progress.on_progress(ProgressEvent {
                    stage: ProgressStage::InstallingModLoader,
                    message: format!(
                        "Verifying {} loader ({})...",
                        kind.as_str(),
                        clean_loader_ver
                    ),
                    current: 0,
                    total: 1,
                });
                let loader_svc = ModLoaderService::new(paths, client);
                loader_svc
                    .install_and_merge(kind, &base_meta, &clean_loader_ver)
                    .await?
            } else if let Some(family) = ForgeFamily::parse(&instance.mod_loader.name) {
                progress.on_progress(ProgressEvent {
                    stage: ProgressStage::InstallingModLoader,
                    message: format!("Verifying {} ({})...", family.as_str(), clean_loader_ver),
                    current: 0,
                    total: 1,
                });
                let meta_java_major = base_meta.java_version.as_ref().map(|j| j.major_version);
                let req_java = JavaMajorPolicy::required_for_minecraft(&base_id, meta_java_major)?;
                let mods_dir = paths.minecraft_dir(&instance.instance_id).join("mods");
                let _ = mods_dir
                    .is_dir()
                    .then(|| detect_mods_required_java(&mods_dir));
                let java = match JavaResolver::resolve(
                    req_java,
                    None,
                    Some(&paths.runtimes_root),
                    false,
                ) {
                    Ok(j) => j,
                    Err(_) => {
                        JavaProvisioner::ensure_managed(&paths.runtimes_root, req_java, client)
                            .await?
                    }
                };
                let forge_svc = ForgeLoaderService::new(paths, client);
                let mut merged = forge_svc
                    .install_and_merge_with_progress(
                        family,
                        &base_meta,
                        &clean_loader_ver,
                        Some(&java.path),
                        |_, _| {},
                    )
                    .await?;
                if family == ForgeFamily::Forge {
                    merged = OptiFineManager::apply_to_version(paths, instance, merged)?;
                }
                merged
            } else {
                OptiFineManager::apply_to_version(paths, instance, base_meta)?
            };

            // 4. Verify libraries & force clean re-extraction of natives
            let lib_mgr = LibraryManager::new(paths, client);
            let target_natives = paths.natives_dir(&metadata.id);
            let marker_dir = target_natives.join(".extracted");
            if marker_dir.is_dir() {
                let _ = fs::remove_dir_all(&marker_dir);
            }
            lib_mgr
                .ensure_libraries(&metadata, Arc::clone(&progress))
                .await?;

            // 5. Verify assets (zero-rewrite fast path for already valid assets; downloads only missing)
            if metadata.asset_index.is_some() {
                let asset_mgr = AssetManager::new(paths, client);
                asset_mgr
                    .ensure_assets(&metadata, Arc::clone(&progress))
                    .await?;
            }

            // 6. Clear stale crash status once repair succeeds cleanly
            let store = InstanceStore::new(paths);
            let mut crash_flag_cleared = false;
            if let Ok(mut inst_meta) = store.load_instance(&instance.instance_id) {
                if inst_meta.last_launch_crashed || inst_meta.last_launch_state == "crashed" {
                    inst_meta.last_launch_crashed = false;
                    inst_meta.last_launch_state = "ready".to_string();
                    let _ = store.save_instance(&inst_meta);
                    crash_flag_cleared = true;
                }
            }

            progress.on_progress(ProgressEvent {
                stage: ProgressStage::Completed,
                message: "Repair completed safely".to_string(),
                current: 1,
                total: 1,
            });

            Ok(RepairReport {
                instance_id: instance.instance_id.clone(),
                version_id: metadata.id,
                mod_loader: instance.mod_loader.name.clone(),
                version_json_repaired,
                client_repaired: !client_was_valid,
                libraries_verified: true,
                assets_verified: true,
                natives_reextracted: true,
                crash_flag_cleared,
            })
        }
        .await;

        match res {
            Ok(report) => {
                let _ = fs::remove_dir_all(&backup_root);
                Ok(report)
            }
            Err(err) => {
                // Transactional rollback on error: never leave the instance in a worse state.
                if had_json && backup_json.is_file() {
                    let _ = fs::copy(&backup_json, &ver_json_path);
                } else if !had_json {
                    let _ = fs::remove_file(&ver_json_path);
                }
                if had_jar && backup_jar.is_file() {
                    let _ = fs::copy(&backup_jar, &client_jar);
                } else if !had_jar {
                    let _ = fs::remove_file(&client_jar);
                }
                if had_base_natives && backup_natives.is_dir() {
                    let _ = fs::remove_dir_all(&vanilla_natives);
                    let _ = copy_dir_recursive(&backup_natives, &vanilla_natives);
                } else if !had_base_natives && vanilla_natives.is_dir() {
                    let _ = fs::remove_dir_all(&vanilla_natives);
                }
                let _ = fs::remove_dir_all(&backup_root);
                Err(err)
            }
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcw_models::minecraft::VersionManifestEntry;
    use mcw_models::progress::NoopProgressSink;
    use sha1::{Digest, Sha1};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use tempfile::tempdir;

    fn sha1_hex(bytes: &[u8]) -> String {
        let mut h = Sha1::new();
        h.update(bytes);
        let d = h.finalize();
        let mut out = String::with_capacity(40);
        for b in d {
            out.push_str(&format!("{b:02x}"));
        }
        out
    }

    #[tokio::test]
    async fn rollback_restores_original_files_and_preserves_saves_mods_options() {
        let tmp = tempdir().unwrap();
        let paths = LauncherPaths::portable(tmp.path());
        paths.initialize().unwrap();

        let store = InstanceStore::new(&paths);
        let (mut inst, _) = store
            .create_instance(
                "rep_test",
                "1.21.4",
                mcw_models::instance::ModLoader::vanilla(),
            )
            .unwrap();
        inst.last_launch_crashed = true;
        inst.last_launch_state = "crashed".to_string();
        store.save_instance(&inst).unwrap();

        // User data that Repair MUST NEVER touch
        let mc_dir = paths.minecraft_dir("rep_test");
        fs::create_dir_all(mc_dir.join("saves/world1")).unwrap();
        fs::create_dir_all(mc_dir.join("mods")).unwrap();
        fs::write(mc_dir.join("saves/world1/level.dat"), b"world-data").unwrap();
        fs::write(mc_dir.join("mods/sodium.jar.disabled"), b"mod-bytes").unwrap();
        fs::write(mc_dir.join("options.txt"), b"fov:90").unwrap();

        // Pre-existing core files that must survive rollback byte-for-byte
        let ver_json = paths.version_json("1.21.4");
        let client_jar = paths.client_jar("1.21.4");
        fs::create_dir_all(ver_json.parent().unwrap()).unwrap();
        fs::write(&ver_json, br#"{"id":"1.21.4","old":true}"#).unwrap();
        fs::write(&client_jar, b"existing-client").unwrap();

        // Mock HTTP server returning valid metadata JSON whose client JAR endpoint 404s
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let candidate = serde_json::json!({
            "id": "1.21.4",
            "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "downloads": {
                "client": {
                    "sha1": "0000000000000000000000000000000000000000",
                    "size": 100,
                    "url": format!("http://{addr}/broken-client.jar")
                }
            },
            "libraries": []
        })
        .to_string();
        let cand_sha1 = sha1_hex(candidate.as_bytes());
        let cand_len = candidate.len() as u64;

        let handle = std::thread::spawn(move || {
            for _ in 0..8 {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut buf = [0u8; 1024];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                if req.starts_with("GET /meta.json ") {
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        candidate.len(),
                        candidate
                    );
                    let _ = stream.write_all(resp.as_bytes());
                } else {
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found",
                    );
                }
            }
        });

        let client = DownloadClient::new().unwrap();
        let entry = VersionManifestEntry {
            id: "1.21.4".to_string(),
            version_type: "release".to_string(),
            url: format!("http://{addr}/meta.json"),
            release_time: String::new(),
            sha1: Some(cand_sha1),
            size: Some(cand_len),
        };

        let result = RepairService::repair_from_manifest_entry(
            &paths,
            &client,
            &inst,
            &entry,
            Arc::new(NoopProgressSink),
        )
        .await;
        drop(handle);

        assert!(result.is_err());
        assert_eq!(
            fs::read(&ver_json).unwrap(),
            br#"{"id":"1.21.4","old":true}"#
        );
        assert_eq!(fs::read(&client_jar).unwrap(), b"existing-client");
        assert_eq!(
            fs::read(mc_dir.join("saves/world1/level.dat")).unwrap(),
            b"world-data"
        );
        assert_eq!(
            fs::read(mc_dir.join("mods/sodium.jar.disabled")).unwrap(),
            b"mod-bytes"
        );
        assert_eq!(fs::read(mc_dir.join("options.txt")).unwrap(), b"fov:90");
    }
}
