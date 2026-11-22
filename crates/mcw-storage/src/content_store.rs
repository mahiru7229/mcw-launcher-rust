use crate::{LauncherPaths, Result, StorageError};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const HASH_CHUNK_SIZE: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct MaterializationResult {
    pub path: PathBuf,
    pub canonical_path: PathBuf,
    pub sha256: String,
    pub size_bytes: u64,
    pub hardlinked: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ContentStoreStats {
    pub blob_count: usize,
    pub total_blob_bytes: u64,
    pub deduplicated_files: usize,
    pub estimated_saved_bytes: u64,
}

/// Content-addressed store (`cache/content_store/`) for immutable mods, resource packs,
/// shader packs, and provider downloads. Mirrors Python `ContentStore`.
pub struct ContentStore;

impl ContentStore {
    /// Streamingly computes lowercase hex SHA-256 of a file using a 1 MiB buffer.
    pub fn sha256_file(path: &Path) -> Result<String> {
        let file = File::open(path).map_err(|source| StorageError::Io {
            path: path.display().to_string(),
            source,
        })?;
        let mut reader = BufReader::with_capacity(HASH_CHUNK_SIZE, file);
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf).map_err(|source| StorageError::Io {
                path: path.display().to_string(),
                source,
            })?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        let digest = hasher.finalize();
        Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
    }

    /// Publishes `source` into the shared content store (`cache/content_store/<p2>/<sha256>`)
    /// and replaces `source` with a hardlink to the canonical blob when supported by the filesystem.
    pub fn adopt(paths: &LauncherPaths, source: &Path) -> Result<MaterializationResult> {
        if !source.is_file() {
            return Err(StorageError::Io {
                path: source.display().to_string(),
                source: io::Error::new(io::ErrorKind::NotFound, "Source file not found"),
            });
        }
        let size_bytes = fs::metadata(source)
            .map_err(|source_err| StorageError::Io {
                path: source.display().to_string(),
                source: source_err,
            })?
            .len();
        let digest = Self::sha256_file(source)?;
        let target = paths.content_store_blob(&digest)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|err| StorageError::Io {
                path: parent.display().to_string(),
                source: err,
            })?;
        }

        if target.is_file() {
            Self::verify_blob(&target, &digest, size_bytes)?;
        } else {
            Self::publish_new_blob(source, &target, &digest, size_bytes)?;
        }

        let hardlinked = Self::replace_with_hardlink(source, &target, &digest, size_bytes);
        Ok(MaterializationResult {
            path: source.to_path_buf(),
            canonical_path: target,
            sha256: digest,
            size_bytes,
            hardlinked,
        })
    }

    /// Materializes immutable content at `destination` via hardlink (when enabled & supported)
    /// or atomic verified copy fallback.
    pub fn materialize(
        paths: &LauncherPaths,
        source: &Path,
        destination: &Path,
        adopt_source: bool,
        prefer_hardlink: bool,
    ) -> Result<MaterializationResult> {
        let (canonical, digest, size_bytes) = if adopt_source {
            let published = Self::adopt(paths, source)?;
            (
                published.canonical_path,
                published.sha256,
                published.size_bytes,
            )
        } else {
            if !source.is_file() {
                return Err(StorageError::Io {
                    path: source.display().to_string(),
                    source: io::Error::new(io::ErrorKind::NotFound, "Source file not found"),
                });
            }
            let size = fs::metadata(source)
                .map_err(|err| StorageError::Io {
                    path: source.display().to_string(),
                    source: err,
                })?
                .len();
            let sha = Self::sha256_file(source)?;
            (source.to_path_buf(), sha, size)
        };

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|err| StorageError::Io {
                path: parent.display().to_string(),
                source: err,
            })?;
        }

        let temp = destination.with_file_name(format!(
            ".{}.{}.materializing",
            destination
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("blob"),
            Uuid::new_v4().simple()
        ));
        let _ = fs::remove_file(&temp);

        let mut hardlinked = false;
        if prefer_hardlink && fs::hard_link(&canonical, &temp).is_ok() {
            hardlinked = true;
        }
        if !hardlinked {
            if let Err(err) = fs::copy(&canonical, &temp) {
                let _ = fs::remove_file(&temp);
                return Err(StorageError::Io {
                    path: destination.display().to_string(),
                    source: err,
                });
            }
        }

        if let Err(err) = Self::verify_blob(&temp, &digest, size_bytes) {
            let _ = fs::remove_file(&temp);
            return Err(err);
        }

        if destination.exists() {
            let _ = fs::remove_file(destination);
        }
        if let Err(err) = fs::rename(&temp, destination) {
            let _ = fs::remove_file(&temp);
            return Err(StorageError::Io {
                path: destination.display().to_string(),
                source: err,
            });
        }

        Ok(MaterializationResult {
            path: destination.to_path_buf(),
            canonical_path: canonical,
            sha256: digest,
            size_bytes,
            hardlinked,
        })
    }

    /// Attempts to satisfy `destination` directly from `cache/content_store/` via hardlink (or
    /// verified copy fallback) when `sha256` already exists in the shared content store.
    pub fn try_hardlink_from_store(
        paths: &LauncherPaths,
        sha256: &str,
        destination: &Path,
    ) -> Option<PathBuf> {
        let clean = sha256.trim().to_ascii_lowercase();
        if clean.len() != 64 || !clean.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let blob = paths.content_store_blob(&clean).ok()?;
        if !blob.is_file() {
            return None;
        }
        let res = Self::materialize(paths, &blob, destination, false, true).ok()?;
        Some(res.canonical_path)
    }

    /// Scans `cache/content_store/` and returns current blob count and total stored bytes.
    pub fn stats(paths: &LauncherPaths) -> ContentStoreStats {
        let dir = paths.content_store_dir();
        if !dir.is_dir() {
            return ContentStoreStats::default();
        }
        let mut blob_count = 0usize;
        let mut total_blob_bytes = 0u64;
        for entry in walkdir::WalkDir::new(&dir).into_iter().flatten() {
            if entry.file_type().is_file() {
                if let Ok(meta) = entry.metadata() {
                    blob_count += 1;
                    total_blob_bytes = total_blob_bytes.saturating_add(meta.len());
                }
            }
        }
        ContentStoreStats {
            blob_count,
            total_blob_bytes,
            deduplicated_files: 0,
            estimated_saved_bytes: 0,
        }
    }

    /// Scans `instances/*/.minecraft/{mods,resourcepacks,shaderpacks}` for duplicate immutable
    /// `.jar`/`.zip` files and hardlinks duplicates through `cache/content_store/`.
    /// Never touches mutable directories such as `saves/`, `config/`, or `options.txt`.
    pub fn deduplicate_instances(paths: &LauncherPaths) -> Result<ContentStoreStats> {
        let mut candidates: Vec<(PathBuf, u64)> = Vec::new();
        let Ok(inst_entries) = fs::read_dir(&paths.instances_root) else {
            return Ok(Self::stats(paths));
        };
        for inst_entry in inst_entries.flatten() {
            let inst_dir = inst_entry.path();
            if !inst_dir.is_dir() || inst_dir.file_name().is_some_and(|n| n == ".runtime") {
                continue;
            }
            let mc_dir = inst_dir.join(".minecraft");
            for sub in ["mods", "resourcepacks", "shaderpacks"] {
                let target_dir = mc_dir.join(sub);
                if !target_dir.is_dir() {
                    continue;
                }
                let Ok(files) = fs::read_dir(&target_dir) else {
                    continue;
                };
                for file in files.flatten() {
                    let p = file.path();
                    if !p.is_file() {
                        continue;
                    }
                    let ext = p
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|s| s.to_ascii_lowercase())
                        .unwrap_or_default();
                    if ext != "jar" && ext != "zip" {
                        continue;
                    }
                    if let Ok(meta) = fs::metadata(&p) {
                        if meta.len() > 0 {
                            candidates.push((p, meta.len()));
                        }
                    }
                }
            }
        }

        // First bucket by file size so we only SHA-256 files that share a size or already exist in store.
        let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
        for (p, size) in candidates {
            by_size.entry(size).or_default().push(p);
        }

        let mut by_hash: HashMap<String, Vec<(PathBuf, u64)>> = HashMap::new();
        for (size, group) in by_size {
            if group.len() < 2 {
                continue;
            }
            for p in group {
                if let Ok(hash) = Self::sha256_file(&p) {
                    by_hash.entry(hash).or_default().push((p, size));
                }
            }
        }

        let mut deduplicated_files = 0usize;
        let mut estimated_saved_bytes = 0u64;

        for (_hash, entries) in by_hash {
            if entries.len() < 2 {
                continue;
            }
            let mut linked_copies = 0usize;
            let mut file_size = 0u64;
            for (p, size) in entries {
                file_size = size;
                if let Ok(res) = Self::adopt(paths, &p) {
                    if res.hardlinked {
                        linked_copies += 1;
                    }
                }
            }
            if linked_copies >= 2 {
                deduplicated_files += linked_copies - 1;
                estimated_saved_bytes = estimated_saved_bytes
                    .saturating_add(file_size.saturating_mul((linked_copies - 1) as u64));
            }
        }

        let mut final_stats = Self::stats(paths);
        final_stats.deduplicated_files = deduplicated_files;
        final_stats.estimated_saved_bytes = estimated_saved_bytes;
        Ok(final_stats)
    }

    fn publish_new_blob(source: &Path, target: &Path, digest: &str, size: u64) -> Result<()> {
        if fs::hard_link(source, target).is_ok() {
            return Self::verify_blob(target, digest, size);
        }
        if target.is_file() {
            return Self::verify_blob(target, digest, size);
        }
        let temp = target.with_file_name(format!(
            ".{}.{}.publishing",
            target
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("blob"),
            Uuid::new_v4().simple()
        ));
        let _ = fs::remove_file(&temp);
        fs::copy(source, &temp).map_err(|err| StorageError::Io {
            path: target.display().to_string(),
            source: err,
        })?;
        if let Err(err) = Self::verify_blob(&temp, digest, size) {
            let _ = fs::remove_file(&temp);
            return Err(err);
        }
        if target.is_file() {
            let _ = fs::remove_file(&temp);
            return Self::verify_blob(target, digest, size);
        }
        fs::rename(&temp, target).map_err(|err| {
            let _ = fs::remove_file(&temp);
            StorageError::Io {
                path: target.display().to_string(),
                source: err,
            }
        })?;
        Self::verify_blob(target, digest, size)
    }

    fn replace_with_hardlink(source: &Path, target: &Path, digest: &str, size: u64) -> bool {
        let temp = source.with_file_name(format!(
            ".{}.{}.hardlink",
            source.file_name().and_then(|n| n.to_str()).unwrap_or("src"),
            Uuid::new_v4().simple()
        ));
        let _ = fs::remove_file(&temp);
        if fs::hard_link(target, &temp).is_err() {
            return false;
        }
        if Self::verify_blob(&temp, digest, size).is_err() {
            let _ = fs::remove_file(&temp);
            return false;
        }
        // On Windows, replacing an existing file via rename may require remove first.
        #[cfg(windows)]
        {
            if fs::rename(&temp, source).is_ok() {
                return true;
            }
            // Keep safe backup copy path? Only if hardlink target already exists & verified!
            let _ = fs::remove_file(source);
            if fs::rename(&temp, source).is_ok() {
                return true;
            }
            let _ = fs::remove_file(&temp);
            false
        }
        #[cfg(not(windows))]
        {
            if fs::rename(&temp, source).is_ok() {
                return true;
            }
            let _ = fs::remove_file(&temp);
            false
        }
    }

    fn verify_blob(path: &Path, expected_sha256: &str, expected_size: u64) -> Result<()> {
        let meta = fs::metadata(path).map_err(|err| StorageError::Io {
            path: path.display().to_string(),
            source: err,
        })?;
        if meta.len() != expected_size {
            return Err(StorageError::Io {
                path: path.display().to_string(),
                source: io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "Content store blob size mismatch (expected {expected_size}, got {})",
                        meta.len()
                    ),
                ),
            });
        }
        let actual = Self::sha256_file(path)?;
        if actual != expected_sha256 {
            return Err(StorageError::Io {
                path: path.display().to_string(),
                source: io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Content store blob SHA-256 verification failed",
                ),
            });
        }
        Ok(())
    }
}

/// Port of Python `SharedFileMaterializer` (`src/core/storage/shared_file_materializer.py`).
pub struct SharedFileMaterializer;

impl SharedFileMaterializer {
    pub fn link_or_copy(source: &Path, destination: &Path) -> Result<bool> {
        if !source.is_file() {
            return Err(StorageError::Io {
                path: source.display().to_string(),
                source: io::Error::new(io::ErrorKind::NotFound, "Source file not found"),
            });
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|err| StorageError::Io {
                path: parent.display().to_string(),
                source: err,
            })?;
        }
        if destination.is_file() {
            if Self::same_content(source, destination) {
                return Ok(true);
            }
            let _ = fs::remove_file(destination);
        }
        if fs::hard_link(source, destination).is_ok() {
            return Ok(true);
        }
        fs::copy(source, destination).map_err(|err| StorageError::Io {
            path: destination.display().to_string(),
            source: err,
        })?;
        Ok(false)
    }

    pub fn same_content(first: &Path, second: &Path) -> bool {
        let (Ok(m1), Ok(m2)) = (fs::metadata(first), fs::metadata(second)) else {
            return false;
        };
        if m1.len() != m2.len() {
            return false;
        }
        match (
            ContentStore::sha256_file(first),
            ContentStore::sha256_file(second),
        ) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InstanceStore;
    use mcw_models::instance::ModLoader;
    use tempfile::tempdir;

    #[test]
    fn adopts_and_deduplicates_identical_mods_across_instances() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();

        let store = InstanceStore::new(&paths);
        store
            .create_instance("pack-a", "1.21.4", ModLoader::vanilla())
            .unwrap();
        store
            .create_instance("pack-b", "1.21.4", ModLoader::vanilla())
            .unwrap();

        let jar_bytes = b"shared-mod-archive-payload-for-dedup-test";
        let mod_a = paths
            .minecraft_dir("pack-a")
            .join("mods")
            .join("sodium.jar");
        let mod_b = paths
            .minecraft_dir("pack-b")
            .join("mods")
            .join("sodium-copy.jar");
        fs::create_dir_all(mod_a.parent().unwrap()).unwrap();
        fs::create_dir_all(mod_b.parent().unwrap()).unwrap();
        fs::write(&mod_a, jar_bytes).unwrap();
        fs::write(&mod_b, jar_bytes).unwrap();

        assert!(SharedFileMaterializer::same_content(&mod_a, &mod_b));

        let stats = ContentStore::deduplicate_instances(&paths).unwrap();
        assert_eq!(stats.blob_count, 1);
        assert_eq!(stats.total_blob_bytes, jar_bytes.len() as u64);
        assert_eq!(stats.deduplicated_files, 1);
        assert_eq!(stats.estimated_saved_bytes, jar_bytes.len() as u64);

        let dest = dir.path().join("extra").join("materialized.jar");
        let mat = ContentStore::materialize(&paths, &mod_a, &dest, true, true).unwrap();
        assert!(dest.is_file());
        assert_eq!(mat.size_bytes, jar_bytes.len() as u64);
        assert!(SharedFileMaterializer::same_content(&mod_a, &dest));
    }
}
