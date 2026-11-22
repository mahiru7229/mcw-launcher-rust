use crate::{MinecraftError, Result};
use mcw_models::minecraft::{VersionManifest, VersionManifestEntry, VersionMetadata};
use mcw_models::MOJANG_MANIFEST_URL;
use mcw_net::{DownloadClient, DownloadRequest};
use mcw_storage::{atomic_write_text, LauncherPaths};
use std::fs;
use tracing::warn;

pub struct VersionManifestService<'a> {
    paths: &'a LauncherPaths,
    client: &'a DownloadClient,
}

impl<'a> VersionManifestService<'a> {
    pub fn new(paths: &'a LauncherPaths, client: &'a DownloadClient) -> Self {
        Self { paths, client }
    }

    pub async fn get_manifest(&self, force_refresh: bool) -> Result<VersionManifest> {
        let cache_path = self.paths.version_manifest();
        if !force_refresh && cache_path.is_file() {
            if let Ok(raw) = fs::read_to_string(&cache_path) {
                if let Ok(parsed) = serde_json::from_str::<VersionManifest>(&raw) {
                    if !parsed.versions.is_empty() {
                        return Ok(parsed);
                    }
                }
            }
        }

        match self
            .client
            .get_text_cached(MOJANG_MANIFEST_URL, &cache_path)
            .await
        {
            Ok(raw) => {
                let parsed: VersionManifest = serde_json::from_str(&raw)?;
                if parsed.versions.is_empty() {
                    return Err(MinecraftError::InvalidMetadata(
                        "Mojang returned an empty version manifest".to_string(),
                    ));
                }
                let _ = atomic_write_text(&cache_path, &raw);
                Ok(parsed)
            }
            Err(net_err) => {
                if cache_path.is_file() {
                    warn!(error = %net_err, "Failed to refresh manifest; falling back to cached version_manifest_v2.json");
                    let raw = fs::read_to_string(&cache_path)?;
                    let parsed: VersionManifest = serde_json::from_str(&raw)?;
                    return Ok(parsed);
                }
                Err(net_err.into())
            }
        }
    }

    pub async fn find_version(
        &self,
        version_id: &str,
        force_refresh: bool,
    ) -> Result<VersionManifestEntry> {
        let manifest = self.get_manifest(force_refresh).await?;
        if let Some(entry) = manifest.versions.into_iter().find(|v| v.id == version_id) {
            return Ok(entry);
        }
        // If not found in cached manifest, retry once with force_refresh.
        if !force_refresh {
            let refreshed = self.get_manifest(true).await?;
            if let Some(entry) = refreshed.versions.into_iter().find(|v| v.id == version_id) {
                return Ok(entry);
            }
        }
        Err(MinecraftError::VersionNotFound(version_id.to_string()))
    }

    pub async fn load_version_metadata(
        &self,
        entry: &VersionManifestEntry,
    ) -> Result<VersionMetadata> {
        let dest = self.paths.version_json(&entry.id);
        let req = DownloadRequest::new(&entry.url, &dest)
            .with_sha1(entry.sha1.clone())
            .with_size(entry.size.unwrap_or(0));
        self.client.download_file(&req).await?;

        let raw = fs::read_to_string(&dest)?;
        let metadata: VersionMetadata = serde_json::from_str(&raw)?;
        Ok(metadata)
    }
}
