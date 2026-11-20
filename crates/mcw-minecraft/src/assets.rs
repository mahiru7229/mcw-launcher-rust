use crate::{MinecraftError, Result};
use mcw_models::minecraft::{AssetIndex, VersionMetadata};
use mcw_models::progress::{ProgressEvent, ProgressSink, ProgressStage};
use mcw_models::MOJANG_RESOURCES_URL;
use mcw_net::{DownloadClient, DownloadRequest};
use mcw_storage::LauncherPaths;
use std::fs;
use std::sync::Arc;

const DEFAULT_CONCURRENCY: usize = 20;

pub struct AssetManager<'a> {
    paths: &'a LauncherPaths,
    client: &'a DownloadClient,
}

impl<'a> AssetManager<'a> {
    pub fn new(paths: &'a LauncherPaths, client: &'a DownloadClient) -> Self {
        Self { paths, client }
    }

    pub async fn ensure_assets(
        &self,
        version: &VersionMetadata,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<String> {
        let index_info = version.asset_index.as_ref().ok_or_else(|| {
            MinecraftError::InvalidMetadata(format!(
                "Version '{}' is missing assetIndex metadata",
                version.id
            ))
        })?;

        let index_path = self.paths.asset_index_path(&index_info.id);
        let index_req = DownloadRequest::new(&index_info.url, &index_path)
            .with_sha1(Some(index_info.sha1.clone()))
            .with_size(index_info.size);
        self.client.download_file(&index_req).await?;

        let raw = fs::read_to_string(&index_path)?;
        let index: AssetIndex = serde_json::from_str(&raw)?;

        // Deduplicate by SHA-1 hash because multiple logical asset names can share the same object.
        // Skip objects already present on disk with matching file size (mirroring Python VerificationCache fast path).
        let mut seen = std::collections::HashSet::with_capacity(index.objects.len());
        let mut requests = Vec::new();
        for obj in index.objects.values() {
            let hash = obj.hash.trim().to_ascii_lowercase();
            if hash.len() < 2 || !seen.insert(hash.clone()) {
                continue;
            }
            let dest = self.paths.asset_object_path(&hash)?;
            if obj.size > 0 {
                if let Ok(meta) = fs::metadata(&dest) {
                    if meta.is_file() && meta.len() == obj.size {
                        continue;
                    }
                }
            }
            let url = format!("{MOJANG_RESOURCES_URL}/{}/{hash}", &hash[..2]);
            requests.push(
                DownloadRequest::new(url, dest)
                    .with_sha1(Some(hash))
                    .with_size(obj.size),
            );
        }

        let total = requests.len() as u64;
        progress.on_progress(ProgressEvent {
            stage: ProgressStage::DownloadingAssets,
            message: format!("Preparing {total} assets..."),
            current: 0,
            total,
        });

        let prog_dl = Arc::clone(&progress);
        self.client
            .download_batch(requests, DEFAULT_CONCURRENCY, move |done, tot| {
                prog_dl.on_progress(ProgressEvent {
                    stage: ProgressStage::DownloadingAssets,
                    message: format!("Assets {done}/{tot}"),
                    current: done,
                    total: tot,
                });
            })
            .await?;

        Ok(index_info.id.clone())
    }
}
