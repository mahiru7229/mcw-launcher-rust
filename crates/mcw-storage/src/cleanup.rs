use crate::LauncherPaths;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

#[derive(Debug, Clone, Default)]
pub struct StorageSummary {
    pub cache_bytes: u64,
    pub instances_bytes: u64,
    pub runtimes_bytes: u64,
    pub backups_bytes: u64,
    pub logs_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct UnusedVersionCandidate {
    pub version_id: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub age_days: u32,
}

#[derive(Debug, Clone)]
pub struct CachedVersionInfo {
    pub version_id: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub has_jar: bool,
}

#[derive(Debug, Clone, Default)]
pub struct StorageCleanupReport {
    pub removed_count: usize,
    pub reclaimed_bytes: u64,
    pub errors: Vec<String>,
}

pub struct StorageCleanupService;

impl StorageCleanupService {
    pub fn dir_size(root: &Path) -> u64 {
        if !root.exists() {
            return 0;
        }
        WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum()
    }

    pub fn inspect_usage(paths: &LauncherPaths) -> StorageSummary {
        let cache_bytes = Self::dir_size(&paths.cache_root);
        let instances_bytes = Self::dir_size(&paths.instances_root);
        let runtimes_bytes = Self::dir_size(&paths.runtimes_root);
        let backups_bytes = Self::dir_size(&paths.backups_root);
        let logs_bytes = Self::dir_size(&paths.logs_root);
        let total_bytes =
            cache_bytes + instances_bytes + runtimes_bytes + backups_bytes + logs_bytes;
        StorageSummary {
            cache_bytes,
            instances_bytes,
            runtimes_bytes,
            backups_bytes,
            logs_bytes,
            total_bytes,
        }
    }

    /// Scan `cache/versions/<version_id>` for cached Minecraft version JAR/JSON folders that are
    /// unreferenced by any instance and have not been modified for at least `retention_days`.
    pub fn scan_unused_versions(
        paths: &LauncherPaths,
        retention_days: u32,
        active_versions: &HashSet<String>,
    ) -> Vec<UnusedVersionCandidate> {
        let versions_dir = paths.cache_root.join("versions");
        let Ok(entries) = fs::read_dir(&versions_dir) else {
            return Vec::new();
        };
        let now = SystemTime::now();
        let mut out = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let version_id = entry.file_name().to_string_lossy().to_string();
            if active_versions.contains(&version_id) {
                continue;
            }

            let mut newest = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(UNIX_EPOCH_FALLBACK);
            for sub in WalkDir::new(&path).into_iter().filter_map(|e| e.ok()) {
                if let Ok(meta) = sub.metadata() {
                    if let Ok(mtime) = meta.modified() {
                        if mtime > newest {
                            newest = mtime;
                        }
                    }
                }
            }

            let age_secs = now
                .duration_since(newest)
                .unwrap_or(Duration::ZERO)
                .as_secs();
            let age_days = (age_secs / 86_400) as u32;
            if age_days >= retention_days {
                let size_bytes = Self::dir_size(&path);
                out.push(UnusedVersionCandidate {
                    version_id,
                    path,
                    size_bytes,
                    age_days,
                });
            }
        }

        out.sort_by_key(|a| std::cmp::Reverse(a.size_bytes));
        out
    }

    /// Safely clear only disposable temporary caches (`cache/api`, `cache/thumbnails`,
    /// `cache/updates`, `cache/tmp`) without touching `instances/`, `accounts/`, `cache/assets/`,
    /// `cache/libraries/`, or `cache/versions/`.
    pub fn clear_api_and_temp_cache(paths: &LauncherPaths) -> StorageCleanupReport {
        let mut report = StorageCleanupReport::default();
        for sub in ["api", "thumbnails", "updates", "tmp"] {
            let dir = paths.cache_root.join(sub);
            if !dir.exists() {
                continue;
            }
            let size = Self::dir_size(&dir);
            match fs::remove_dir_all(&dir) {
                Ok(()) => {
                    report.removed_count += 1;
                    report.reclaimed_bytes += size;
                }
                Err(e) => {
                    report
                        .errors
                        .push(format!("Failed to clear {}: {e}", dir.display()));
                }
            }
        }
        report
    }

    pub fn prune_unused_versions(candidates: &[UnusedVersionCandidate]) -> StorageCleanupReport {
        let mut report = StorageCleanupReport::default();
        for cand in candidates {
            if !cand.path.is_dir() {
                continue;
            }
            let size = Self::dir_size(&cand.path);
            match fs::remove_dir_all(&cand.path) {
                Ok(()) => {
                    report.removed_count += 1;
                    report.reclaimed_bytes += size;
                }
                Err(e) => {
                    report.errors.push(format!(
                        "Failed to remove cached version '{}': {e}",
                        cand.version_id
                    ));
                }
            }
        }
        report
    }

    /// List all version folders under `cache/versions/`, sorted by `version_id` descending.
    pub fn list_cached_versions(paths: &LauncherPaths) -> Vec<CachedVersionInfo> {
        let versions_dir = paths.cache_root.join("versions");
        let Ok(entries) = fs::read_dir(&versions_dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let version_id = entry.file_name().to_string_lossy().to_string();
            let has_jar = path.join(format!("{version_id}.jar")).is_file();
            let size_bytes = Self::dir_size(&path);
            out.push(CachedVersionInfo {
                version_id,
                path,
                size_bytes,
                has_jar,
            });
        }
        out.sort_by(|a, b| b.version_id.cmp(&a.version_id));
        out
    }

    /// Safely delete a single cached version folder under `cache/versions/<version_id>`,
    /// returning reclaimed bytes.
    pub fn delete_cached_version(paths: &LauncherPaths, version_id: &str) -> std::io::Result<u64> {
        let clean = version_id.trim();
        if clean.is_empty() || clean.contains('/') || clean.contains('\\') || clean.contains("..") {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid version identifier",
            ));
        }
        let dir = paths.version_dir(clean);
        if !dir.is_dir() {
            return Ok(0);
        }
        let size = Self::dir_size(&dir);
        fs::remove_dir_all(&dir)?;
        Ok(size)
    }
}

const UNIX_EPOCH_FALLBACK: SystemTime = SystemTime::UNIX_EPOCH;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn inspects_usage_and_clears_only_disposable_caches() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();

        fs::create_dir_all(paths.cache_root.join("api")).unwrap();
        fs::write(
            paths.cache_root.join("api").join("search.json"),
            vec![b'x'; 512],
        )
        .unwrap();
        fs::create_dir_all(paths.cache_root.join("assets")).unwrap();
        fs::write(
            paths.cache_root.join("assets").join("keep.dat"),
            vec![b'a'; 1024],
        )
        .unwrap();
        fs::create_dir_all(paths.instances_root.join("Demo")).unwrap();
        fs::write(
            paths.instances_root.join("Demo").join("world.dat"),
            vec![b'w'; 256],
        )
        .unwrap();

        let summary = StorageCleanupService::inspect_usage(&paths);
        assert_eq!(summary.cache_bytes, 1536);
        assert_eq!(summary.instances_bytes, 256);
        assert_eq!(summary.total_bytes, 1792);

        let report = StorageCleanupService::clear_api_and_temp_cache(&paths);
        assert_eq!(report.removed_count, 1);
        assert_eq!(report.reclaimed_bytes, 512);
        assert!(paths.cache_root.join("assets").join("keep.dat").is_file());
        assert!(paths
            .instances_root
            .join("Demo")
            .join("world.dat")
            .is_file());
    }

    #[test]
    fn scans_and_prunes_unreferenced_versions_respecting_active_set() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();

        let v1 = paths.version_dir("1.20.4");
        let v2 = paths.version_dir("1.21.1");
        fs::create_dir_all(&v1).unwrap();
        fs::create_dir_all(&v2).unwrap();
        fs::write(v1.join("1.20.4.jar"), vec![1u8; 400]).unwrap();
        fs::write(v2.join("1.21.1.jar"), vec![2u8; 800]).unwrap();

        let mut active = HashSet::new();
        active.insert("1.21.1".to_string());

        let cands = StorageCleanupService::scan_unused_versions(&paths, 0, &active);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].version_id, "1.20.4");

        // With 14-day retention, newly created versions are skipped
        assert!(StorageCleanupService::scan_unused_versions(&paths, 14, &active).is_empty());

        let pruned = StorageCleanupService::prune_unused_versions(&cands);
        assert_eq!(pruned.removed_count, 1);
        assert_eq!(pruned.reclaimed_bytes, 400);
        assert!(!v1.exists());
        assert!(v2.exists());
    }

    #[test]
    fn lists_and_deletes_cached_versions_safely() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();

        let v1 = paths.version_dir("1.20.4");
        let v2 = paths.version_dir("1.21.1");
        fs::create_dir_all(&v1).unwrap();
        fs::create_dir_all(&v2).unwrap();
        fs::write(v1.join("1.20.4.jar"), vec![1u8; 300]).unwrap();
        fs::write(v2.join("1.21.1.json"), b"{}").unwrap();

        let list = StorageCleanupService::list_cached_versions(&paths);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].version_id, "1.21.1");
        assert!(!list[0].has_jar);
        assert_eq!(list[1].version_id, "1.20.4");
        assert!(list[1].has_jar);
        assert_eq!(list[1].size_bytes, 300);

        let reclaimed = StorageCleanupService::delete_cached_version(&paths, "1.20.4").unwrap();
        assert_eq!(reclaimed, 300);
        assert!(!v1.exists());

        assert!(StorageCleanupService::delete_cached_version(&paths, "../escape").is_err());
    }
}
