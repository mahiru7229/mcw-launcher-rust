use crate::assets::AssetManager;
use crate::libraries::LibraryManager;
use crate::manifest::VersionManifestService;
use crate::modloader::{LoaderKind, ModLoaderService};
use crate::{MinecraftError, Result};
use mcw_models::instance::Instance;
use mcw_models::progress::{ProgressEvent, ProgressSink, ProgressStage};
use mcw_net::{verify_file, DownloadClient, DownloadRequest};
use mcw_storage::{InstanceRunLock, LauncherPaths};
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::Arc;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepairReport {
    pub instance_id: String,
    pub version_id: String,
    pub client_repaired: bool,
    pub libraries_verified: bool,
    pub assets_verified: bool,
    pub natives_reextracted: bool,
}

pub struct RepairService<'a> {
    paths: &'a LauncherPaths,
    client: &'a DownloadClient,
}

impl<'a> RepairService<'a> {
    pub fn new(paths: &'a LauncherPaths, client: &'a DownloadClient) -> Self {
        Self { paths, client }
    }

    /// Force-verify and repair an instance's client JAR, libraries, natives, and assets.
    pub async fn repair_instance(
        &self,
        instance: &Instance,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<RepairReport> {
        if InstanceRunLock::is_running(self.paths, &instance.instance_id) {
            return Err(MinecraftError::Storage(
                mcw_storage::StorageError::InstanceAlreadyRunning(instance.instance_id.clone()),
            ));
        }

        progress.on_progress(ProgressEvent {
            stage: ProgressStage::FetchingManifest,
            message: format!("Refreshing metadata for {}...", instance.version_id),
            current: 0,
            total: 1,
        });

        let manifest_svc = VersionManifestService::new(self.paths, self.client);
        let entry = manifest_svc
            .find_version(&instance.version_id, true)
            .await?;
        let base_meta = manifest_svc.load_version_metadata(&entry).await?;
        let base_id = base_meta.id.clone();

        let metadata = if let Some(kind) = LoaderKind::parse(&instance.mod_loader.name) {
            let loader_svc = ModLoaderService::new(self.paths, self.client);
            loader_svc
                .install_and_merge(kind, &base_meta, &instance.mod_loader.version)
                .await?
        } else {
            base_meta
        };

        // 1. Verify / repair client JAR
        let client_artifact = metadata
            .downloads
            .as_ref()
            .and_then(|d| d.client.as_ref())
            .ok_or_else(|| {
                MinecraftError::InvalidMetadata(format!(
                    "Version '{base_id}' is missing client download metadata"
                ))
            })?;
        let client_jar = self.paths.client_jar(&base_id);
        let client_was_valid = client_jar.is_file()
            && verify_file(
                &client_jar,
                client_artifact.sha1.as_deref(),
                client_artifact.size,
            )
            .is_ok();
        if !client_was_valid {
            let _ = fs::remove_file(&client_jar);
            let req = DownloadRequest::new(&client_artifact.url, &client_jar)
                .with_sha1(client_artifact.sha1.clone())
                .with_size(client_artifact.size);
            self.client.download_file(&req).await?;
        }

        // 2. Clear extracted native markers to force clean re-extraction
        let natives_dir = self.paths.natives_dir(&metadata.id);
        let marker_dir = natives_dir.join(".extracted");
        if marker_dir.is_dir() {
            let _ = fs::remove_dir_all(&marker_dir);
        }

        // 3. Verify / redownload libraries & re-extract natives
        let lib_mgr = LibraryManager::new(self.paths, self.client);
        lib_mgr
            .ensure_libraries(&metadata, Arc::clone(&progress))
            .await?;

        // 4. Verify / redownload assets
        let asset_mgr = AssetManager::new(self.paths, self.client);
        asset_mgr
            .ensure_assets(&metadata, Arc::clone(&progress))
            .await?;

        progress.on_progress(ProgressEvent {
            stage: ProgressStage::Completed,
            message: "Repair completed".to_string(),
            current: 1,
            total: 1,
        });

        Ok(RepairReport {
            instance_id: instance.instance_id.clone(),
            version_id: metadata.id,
            client_repaired: !client_was_valid,
            libraries_verified: true,
            assets_verified: true,
            natives_reextracted: true,
        })
    }
}
