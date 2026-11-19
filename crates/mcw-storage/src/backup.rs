use crate::{InstanceRunLock, LauncherPaths, Result, StorageError};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const BACKUP_MANIFEST_NAME: &str = "mcw-backup.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub schema_version: u32,
    pub instance_id: String,
    pub created_at_epoch: u64,
    pub note: String,
    pub file_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub id: String,
    pub instance_id: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub created_at_epoch: u64,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct RestoreOutcome {
    pub instance_id: String,
    pub restored_files: u64,
    pub backup_path: PathBuf,
}

pub struct BackupManager<'a> {
    paths: &'a LauncherPaths,
}

impl<'a> BackupManager<'a> {
    pub fn new(paths: &'a LauncherPaths) -> Self {
        Self { paths }
    }

    pub fn instance_backups_dir(&self, instance_id: &str) -> PathBuf {
        self.paths.backups_root.join(instance_id)
    }

    pub fn create_backup(&self, instance_id: &str, note: impl Into<String>) -> Result<BackupInfo> {
        let mc_dir = self.paths.minecraft_dir(instance_id);
        if !mc_dir.is_dir() {
            return Err(StorageError::InstanceNotFound(instance_id.to_string()));
        }
        if InstanceRunLock::is_running(self.paths, instance_id) {
            return Err(StorageError::InstanceAlreadyRunning(
                instance_id.to_string(),
            ));
        }

        let backups_dir = self.instance_backups_dir(instance_id);
        fs::create_dir_all(&backups_dir).map_err(|source| StorageError::Io {
            path: backups_dir.display().to_string(),
            source,
        })?;

        let now = now_epoch_secs();
        let short_id = &Uuid::new_v4().simple().to_string()[..8];
        let backup_id = format!("backup-{now}-{short_id}");
        let final_path = backups_dir.join(format!("{backup_id}.zip"));
        let temp_path = backups_dir.join(format!(".{backup_id}.zip.part"));

        let file = File::create(&temp_path).map_err(|source| StorageError::Io {
            path: temp_path.display().to_string(),
            source,
        })?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        let mut file_count = 0u64;
        for entry in WalkDir::new(&mc_dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            let rel = match path.strip_prefix(&mc_dir) {
                Ok(r) if !r.as_os_str().is_empty() => r,
                _ => continue,
            };
            let rel_posix = format!(".minecraft/{}", rel.to_string_lossy().replace('\\', "/"));
            if entry.file_type().is_dir() {
                zip.add_directory(&rel_posix, options)
                    .map_err(|e| StorageError::Backup(e.to_string()))?;
            } else if entry.file_type().is_file() {
                zip.start_file(&rel_posix, options)
                    .map_err(|e| StorageError::Backup(e.to_string()))?;
                let mut f = File::open(path).map_err(|source| StorageError::Io {
                    path: path.display().to_string(),
                    source,
                })?;
                io::copy(&mut f, &mut zip).map_err(|source| StorageError::Io {
                    path: temp_path.display().to_string(),
                    source,
                })?;
                file_count += 1;
            }
        }

        let note_str = note.into();
        let manifest = BackupManifest {
            schema_version: 1,
            instance_id: instance_id.to_string(),
            created_at_epoch: now,
            note: note_str.clone(),
            file_count,
        };
        let manifest_bytes =
            serde_json::to_vec_pretty(&manifest).map_err(|source| StorageError::Json {
                path: BACKUP_MANIFEST_NAME.to_string(),
                source,
            })?;
        zip.start_file(BACKUP_MANIFEST_NAME, options)
            .map_err(|e| StorageError::Backup(e.to_string()))?;
        zip.write_all(&manifest_bytes)
            .map_err(|source| StorageError::Io {
                path: temp_path.display().to_string(),
                source,
            })?;
        zip.finish()
            .map_err(|e| StorageError::Backup(e.to_string()))?;

        fs::rename(&temp_path, &final_path).map_err(|source| StorageError::Io {
            path: final_path.display().to_string(),
            source,
        })?;

        let size_bytes = fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);
        Ok(BackupInfo {
            id: backup_id,
            instance_id: instance_id.to_string(),
            path: final_path,
            size_bytes,
            created_at_epoch: now,
            note: note_str,
        })
    }

    pub fn list_backups(&self, instance_id: &str) -> Result<Vec<BackupInfo>> {
        let dir = self.instance_backups_dir(instance_id);
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        let entries = fs::read_dir(&dir).map_err(|source| StorageError::Io {
            path: dir.display().to_string(),
            source,
        })?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.ends_with(".zip") || name.starts_with('.') {
                continue;
            }
            let id = name.trim_end_matches(".zip").to_string();
            let size_bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let manifest = read_backup_manifest(&path);
            out.push(BackupInfo {
                id,
                instance_id: instance_id.to_string(),
                path,
                size_bytes,
                created_at_epoch: manifest.as_ref().map(|m| m.created_at_epoch).unwrap_or(0),
                note: manifest.map(|m| m.note).unwrap_or_default(),
            });
        }
        out.sort_by_key(|a| std::cmp::Reverse(a.created_at_epoch));
        Ok(out)
    }

    /// Safely restore an instance backup using a staging directory + rollback swap.
    /// Lesson #6: NEVER delete the `rollback` directory if restoration fails!
    pub fn restore_backup(&self, instance_id: &str, backup_id: &str) -> Result<RestoreOutcome> {
        if InstanceRunLock::is_running(self.paths, instance_id) {
            return Err(StorageError::InstanceAlreadyRunning(
                instance_id.to_string(),
            ));
        }

        let backup_path = self
            .instance_backups_dir(instance_id)
            .join(format!("{}.zip", backup_id.trim_end_matches(".zip")));
        if !backup_path.is_file() {
            return Err(StorageError::Backup(format!(
                "Backup '{}' not found for instance '{}'",
                backup_id, instance_id
            )));
        }

        let inst_dir = self.paths.instance_dir(instance_id);
        if !inst_dir.is_dir() {
            return Err(StorageError::InstanceNotFound(instance_id.to_string()));
        }

        let staging_root = self.paths.cache_root.join("backups").join("staging");
        let run_id = Uuid::new_v4().simple().to_string();
        let staging_dir = staging_root.join(format!("stage-{run_id}"));
        let staged_mc = staging_dir.join(".minecraft");
        fs::create_dir_all(&staged_mc).map_err(|source| StorageError::Io {
            path: staged_mc.display().to_string(),
            source,
        })?;

        // 1. Extract and validate archive into staging directory first.
        let restored_files = match extract_backup_to_staging(&backup_path, &staging_dir) {
            Ok(count) => count,
            Err(err) => {
                let _ = fs::remove_dir_all(&staging_dir);
                return Err(err);
            }
        };

        // 2. Move live `.minecraft` to a rollback folder inside the instance directory.
        let live_mc = self.paths.minecraft_dir(instance_id);
        let rollback_dir = inst_dir.join(format!(".mcw-rollback-{run_id}"));
        let had_live = live_mc.exists();
        if had_live {
            if let Err(source) = fs::rename(&live_mc, &rollback_dir) {
                let _ = fs::remove_dir_all(&staging_dir);
                return Err(StorageError::Io {
                    path: rollback_dir.display().to_string(),
                    source,
                });
            }
        }

        // 3. Swap staged `.minecraft` into place. If this fails, roll back and NEVER delete rollback_dir!
        if let Err(swap_err) = move_dir_fallback(&staged_mc, &live_mc) {
            if had_live && rollback_dir.exists() && fs::rename(&rollback_dir, &live_mc).is_err() {
                // Lesson #6: Preserve rollback_dir untouched so user data is never lost.
                return Err(StorageError::Backup(format!(
                    "Restore failed ({swap_err}) and automatic rollback failed; original data is preserved at {}",
                    rollback_dir.display()
                )));
            }
            return Err(StorageError::Backup(format!(
                "Restore failed ({swap_err}); rolled back to previous state."
            )));
        }

        // 4. Only delete rollback_dir and staging_dir AFTER restore completely succeeded.
        if rollback_dir.exists() {
            let _ = fs::remove_dir_all(&rollback_dir);
        }
        let _ = fs::remove_dir_all(&staging_dir);

        Ok(RestoreOutcome {
            instance_id: instance_id.to_string(),
            restored_files,
            backup_path,
        })
    }

    pub fn delete_backup(&self, instance_id: &str, backup_id: &str) -> Result<()> {
        let backup_path = self
            .instance_backups_dir(instance_id)
            .join(format!("{}.zip", backup_id.trim_end_matches(".zip")));
        if !backup_path.is_file() {
            return Err(StorageError::Backup(format!(
                "Backup '{}' not found",
                backup_id
            )));
        }
        fs::remove_file(&backup_path).map_err(|source| StorageError::Io {
            path: backup_path.display().to_string(),
            source,
        })
    }
}

fn read_backup_manifest(zip_path: &Path) -> Option<BackupManifest> {
    let file = File::open(zip_path).ok()?;
    let mut archive = ZipArchive::new(file).ok()?;
    let entry = archive.by_name(BACKUP_MANIFEST_NAME).ok()?;
    serde_json::from_reader(entry).ok()
}

fn extract_backup_to_staging(zip_path: &Path, staging_dir: &Path) -> Result<u64> {
    let file = File::open(zip_path).map_err(|source| StorageError::Io {
        path: zip_path.display().to_string(),
        source,
    })?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| StorageError::Backup(format!("Invalid zip: {e}")))?;

    let mut count = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| StorageError::Backup(e.to_string()))?;
        let name = entry.name().replace('\\', "/");
        if name == BACKUP_MANIFEST_NAME || name.trim().is_empty() {
            continue;
        }
        let rel = safe_backup_rel_path(&name)?;
        let target = staging_dir.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|source| StorageError::Io {
                path: target.display().to_string(),
                source,
            })?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|source| StorageError::Io {
                path: parent.display().to_string(),
                source,
            })?;
        }
        let mut out = File::create(&target).map_err(|source| StorageError::Io {
            path: target.display().to_string(),
            source,
        })?;
        io::copy(&mut entry, &mut out).map_err(|source| StorageError::Io {
            path: target.display().to_string(),
            source,
        })?;
        count += 1;
    }
    Ok(count)
}

fn safe_backup_rel_path(name: &str) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for comp in Path::new(name).components() {
        match comp {
            Component::Normal(part) => {
                if part.to_string_lossy().contains(':') {
                    return Err(StorageError::InvalidRelativePath(name.to_string()));
                }
                out.push(part);
            }
            _ => return Err(StorageError::InvalidRelativePath(name.to_string())),
        }
    }
    Ok(out)
}

fn move_dir_fallback(from: &Path, to: &Path) -> io::Result<()> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Cross-device fallback: copy recursively then remove source.
            copy_dir_recursive(from, to)?;
            fs::remove_dir_all(from)
        }
    }
}

fn copy_dir_recursive(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in WalkDir::new(from).into_iter().filter_map(|e| e.ok()) {
        let rel = entry.path().strip_prefix(from).map_err(io::Error::other)?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let dest = to.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&dest)?;
        } else if entry.file_type().is_file() {
            if let Some(p) = dest.parent() {
                fs::create_dir_all(p)?;
            }
            fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InstanceStore;
    use mcw_models::instance::ModLoader;
    use tempfile::tempdir;

    #[test]
    fn creates_lists_and_restores_instance_backup_safely() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();

        let store = InstanceStore::new(&paths);
        store
            .create_instance("survival", "1.21.4", ModLoader::vanilla())
            .unwrap();

        let mc_dir = paths.minecraft_dir("survival");
        fs::create_dir_all(mc_dir.join("saves").join("world1")).unwrap();
        fs::write(mc_dir.join("saves").join("world1").join("level.dat"), b"v1").unwrap();

        let mgr = BackupManager::new(&paths);
        let info = mgr.create_backup("survival", "before nether").unwrap();
        assert!(info.path.is_file());

        let list = mgr.list_backups("survival").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].note, "before nether");

        // Mutate live world
        fs::write(
            mc_dir.join("saves").join("world1").join("level.dat"),
            b"corrupted",
        )
        .unwrap();

        // Restore backup
        let restored = mgr.restore_backup("survival", &info.id).unwrap();
        assert_eq!(restored.restored_files, 1);
        assert_eq!(
            fs::read(mc_dir.join("saves").join("world1").join("level.dat")).unwrap(),
            b"v1"
        );
    }
}
