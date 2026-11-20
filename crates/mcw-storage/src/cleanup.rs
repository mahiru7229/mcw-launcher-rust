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
}
