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

        let (cached_count, requests) = collect_missing_asset_requests(self.paths, &index)?;
        let missing_total = requests.len() as u64;
        let total_unique = cached_count + missing_total;

        if missing_total == 0 {
            progress.on_progress(ProgressEvent {
                stage: ProgressStage::DownloadingAssets,
                message: format!("All {total_unique} assets verified (cached)"),
                current: total_unique,
                total: total_unique,
            });
            return Ok(index_info.id.clone());
        }

        progress.on_progress(ProgressEvent {
            stage: ProgressStage::DownloadingAssets,
            message: format!(
                "Assets: {cached_count} cached, downloading {missing_total} missing..."
            ),
            current: 0,
            total: missing_total,
        });

        let prog_dl = Arc::clone(&progress);
        self.client
            .download_batch(requests, DEFAULT_CONCURRENCY, move |done, tot| {
                prog_dl.on_progress(ProgressEvent {
                    stage: ProgressStage::DownloadingAssets,
                    message: format!("Missing assets {done}/{tot} ({cached_count} cached)"),
                    current: done,
                    total: tot,
                });
            })
            .await?;

        Ok(index_info.id.clone())
    }
}

/// Scans unique asset objects against `<cache_root>/assets/objects` and returns
/// `(cached_count, missing_download_requests)` without touching SSD for valid cached files.
fn collect_missing_asset_requests(
    paths: &LauncherPaths,
    index: &AssetIndex,
) -> Result<(u64, Vec<DownloadRequest>)> {
    let mut seen = std::collections::HashSet::with_capacity(index.objects.len());
    let mut requests = Vec::new();
    let mut cached_count = 0u64;

    for obj in index.objects.values() {
        let hash = obj.hash.trim().to_ascii_lowercase();
        if hash.len() < 2 || !seen.insert(hash.clone()) {
            continue;
        }
        let dest = paths.asset_object_path(&hash)?;
        if let Ok(meta) = fs::metadata(&dest) {
            if meta.is_file() && meta.len() == obj.size {
                cached_count += 1;
                continue;
            }
        }
        let url = format!("{MOJANG_RESOURCES_URL}/{}/{hash}", &hash[..2]);
        requests.push(
            DownloadRequest::new(url, dest)
                .with_sha1(Some(hash))
                .with_size(obj.size),
        );
    }

    Ok((cached_count, requests))
}

#[cfg(test)]
mod tests {
    use super::collect_missing_asset_requests;
    use mcw_models::minecraft::{AssetIndex, AssetObject};
    use mcw_storage::LauncherPaths;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn reuses_cached_assets_including_zero_byte_and_queues_only_missing() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();

        let h_ok = "aa11223344556677889900112233445566778899";
        let h_zero = "bb11223344556677889900112233445566778899";
        let h_bad_size = "cc11223344556677889900112233445566778899";
        let h_missing = "dd11223344556677889900112233445566778899";

        for (hash, bytes) in [
            (h_ok, b"hello".as_slice()),
            (h_zero, b"".as_slice()),
            (h_bad_size, b"short".as_slice()),
        ] {
            let p = paths.asset_object_path(hash).unwrap();
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, bytes).unwrap();
        }

        let mut objects = HashMap::new();
        objects.insert(
            "a.ogg".into(),
            AssetObject {
                hash: h_ok.into(),
                size: 5,
            },
        );
        // Duplicate hash entry should be deduplicated cleanly
        objects.insert(
            "a_dup.ogg".into(),
            AssetObject {
                hash: h_ok.into(),
                size: 5,
            },
        );
        objects.insert(
            "empty.dat".into(),
            AssetObject {
                hash: h_zero.into(),
                size: 0,
            },
        );
        objects.insert(
            "stale.json".into(),
            AssetObject {
                hash: h_bad_size.into(),
                size: 99,
            },
        );
        objects.insert(
            "missing.png".into(),
            AssetObject {
                hash: h_missing.into(),
                size: 42,
            },
        );

        let index = AssetIndex { objects };
        let (cached, missing) = collect_missing_asset_requests(&paths, &index).unwrap();
        assert_eq!(cached, 2);
        assert_eq!(missing.len(), 2);
    }
}
