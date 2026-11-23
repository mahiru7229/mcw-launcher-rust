use crate::{Result, StorageError};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LauncherPaths {
    pub project_root: PathBuf,
    pub cache_root: PathBuf,
    pub instances_root: PathBuf,
    pub accounts_root: PathBuf,
    pub config_root: PathBuf,
    pub logs_root: PathBuf,
    pub backups_root: PathBuf,
    pub theme_root: PathBuf,
    pub runtimes_root: PathBuf,
    pub instance_locks_root: PathBuf,
}

impl LauncherPaths {
    /// Portable storage layout matching MCW Launcher 1.8.x on Windows.
    pub fn portable(root: impl Into<PathBuf>) -> Self {
        let project_root = root.into();
        let instances_root = project_root.join("instances");
        Self {
            cache_root: project_root.join("cache"),
            instance_locks_root: instances_root.join(".runtime").join("locks"),
            instances_root,
            accounts_root: project_root.join("accounts"),
            config_root: project_root.join("config"),
            logs_root: project_root.join("logs"),
            backups_root: project_root.join("backups"),
            theme_root: project_root.join("themes"),
            runtimes_root: project_root.join("runtimes"),
            project_root,
        }
    }

    pub fn initialize(&self) -> Result<()> {
        let dirs = [
            &self.cache_root,
            &self.instances_root,
            &self.accounts_root,
            &self.config_root,
            &self.logs_root,
            &self.backups_root,
            &self.theme_root,
            &self.runtimes_root,
            &self.instance_locks_root,
        ];
        for dir in dirs {
            fs::create_dir_all(dir).map_err(|source| StorageError::Io {
                path: dir.display().to_string(),
                source,
            })?;
        }
        Ok(())
    }

    pub fn version_manifest(&self) -> PathBuf {
        self.cache_root
            .join("versions")
            .join("version_manifest_v2.json")
    }

    pub fn version_dir(&self, version_id: &str) -> PathBuf {
        self.cache_root.join("versions").join(version_id)
    }

    pub fn version_json(&self, version_id: &str) -> PathBuf {
        self.version_dir(version_id)
            .join(format!("{version_id}.json"))
    }

    pub fn client_jar(&self, version_id: &str) -> PathBuf {
        self.version_dir(version_id)
            .join(format!("{version_id}.jar"))
    }

    pub fn libraries_dir(&self) -> PathBuf {
        self.cache_root.join("libraries")
    }

    pub fn natives_dir(&self, version_id: &str) -> PathBuf {
        self.version_dir(version_id).join("natives")
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.cache_root.join("assets")
    }

    pub fn asset_indexes_dir(&self) -> PathBuf {
        self.assets_dir().join("indexes")
    }

    pub fn asset_index_path(&self, index_id: &str) -> PathBuf {
        self.asset_indexes_dir().join(format!("{index_id}.json"))
    }

    pub fn asset_objects_dir(&self) -> PathBuf {
        self.assets_dir().join("objects")
    }

    pub fn asset_object_path(&self, sha1: &str) -> Result<PathBuf> {
        if sha1.len() < 2 || !sha1.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(StorageError::InvalidRelativePath(format!(
                "Invalid asset SHA-1: {sha1}"
            )));
        }
        Ok(self.asset_objects_dir().join(&sha1[..2]).join(sha1))
    }

    pub fn content_store_dir(&self) -> PathBuf {
        self.cache_root.join("content_store")
    }

    pub fn content_store_blob(&self, sha256: &str) -> Result<PathBuf> {
        let lower = sha256.trim().to_ascii_lowercase();
        if lower.len() != 64 || !lower.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(StorageError::InvalidRelativePath(format!(
                "Invalid content store SHA-256: {sha256}"
            )));
        }
        Ok(self.content_store_dir().join(&lower[..2]).join(lower))
    }

    pub fn instance_dir(&self, instance_id: &str) -> PathBuf {
        self.instances_root.join(instance_id)
    }

    pub fn instance_json(&self, instance_id: &str) -> PathBuf {
        self.instance_dir(instance_id).join("instance.json")
    }

    pub fn instance_settings_json(&self, instance_id: &str) -> PathBuf {
        self.instance_dir(instance_id).join("settings.json")
    }

    pub fn minecraft_dir(&self, instance_id: &str) -> PathBuf {
        self.instance_dir(instance_id).join(".minecraft")
    }

    /// Safely join a relative slash-separated metadata path under `base`, rejecting
    /// path traversal (`..`), absolute paths, or drive prefixes.
    pub fn safe_join_relative(base: &Path, relative: &str) -> Result<PathBuf> {
        let normalized = relative.replace('\\', "/");
        if normalized.is_empty() || normalized.starts_with('/') {
            return Err(StorageError::InvalidRelativePath(relative.to_string()));
        }
        let mut out = base.to_path_buf();
        for component in Path::new(&normalized).components() {
            match component {
                Component::Normal(part) => {
                    let s = part.to_string_lossy();
                    if s.contains(':') {
                        return Err(StorageError::InvalidRelativePath(relative.to_string()));
                    }
                    out.push(part);
                }
                _ => return Err(StorageError::InvalidRelativePath(relative.to_string())),
            }
        }
        Ok(out)
    }

    /// Safe user-writable data directory when the portable location (e.g. `C:\` / `Program Files`)
    /// refuses write access (`os error 5`).
    pub fn default_safe_data_dir() -> PathBuf {
        #[cfg(windows)]
        {
            if let Some(local) = std::env::var_os("LOCALAPPDATA") {
                return PathBuf::from(local).join("MCWLauncher");
            }
            if let Some(profile) = std::env::var_os("USERPROFILE") {
                return PathBuf::from(profile)
                    .join("AppData")
                    .join("Local")
                    .join("MCWLauncher");
            }
        }
        #[cfg(not(windows))]
        {
            if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
                return PathBuf::from(xdg).join("mcw-launcher");
            }
            if let Some(home) = std::env::var_os("HOME") {
                return PathBuf::from(home)
                    .join(".local")
                    .join("share")
                    .join("mcw-launcher");
            }
        }
        std::env::temp_dir().join("MCWLauncher")
    }

    /// Tests whether the process can create and write files inside `dir`.
    pub fn probe_dir_writable(dir: &Path) -> bool {
        if fs::create_dir_all(dir).is_err() {
            return false;
        }
        let probe = dir.join(format!(".mcw-write-probe-{}", std::process::id()));
        match fs::File::create(&probe) {
            Ok(f) => {
                drop(f);
                let _ = fs::remove_file(&probe);
                true
            }
            Err(_) => false,
        }
    }

    /// Returns `true` if `dir` already contains user data (instances/worlds, accounts, config, themes, backups).
    pub fn dir_has_user_data(dir: &Path) -> bool {
        ["instances", "accounts", "config", "themes", "backups"]
            .iter()
            .any(|sub| {
                let p = dir.join(sub);
                p.is_dir()
                    && fs::read_dir(&p)
                        .map(|mut rd| rd.next().is_some())
                        .unwrap_or(false)
            })
    }

    /// Backs up (optional ZIP into `new_root/backups/`) and copies user data & worlds from `old_root` into `new_root`.
    pub fn relocate_user_data(old_root: &Path, new_root: &Path, backup_first: bool) -> Result<()> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        use zip::{CompressionMethod, ZipWriter};

        const USER_SUBDIRS: [&str; 5] = ["instances", "accounts", "config", "themes", "backups"];
        fs::create_dir_all(new_root).map_err(|source| StorageError::Io {
            path: new_root.display().to_string(),
            source,
        })?;

        if backup_first {
            let backups_dir = new_root.join("backups");
            fs::create_dir_all(&backups_dir).map_err(|source| StorageError::Io {
                path: backups_dir.display().to_string(),
                source,
            })?;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let backup_path = backups_dir.join(format!("pre_relocation_{ts}.zip"));
            let file = fs::File::create(&backup_path).map_err(|source| StorageError::Io {
                path: backup_path.display().to_string(),
                source,
            })?;
            let mut zip = ZipWriter::new(file);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            for sub in USER_SUBDIRS {
                let src = old_root.join(sub);
                if !src.is_dir() {
                    continue;
                }
                for entry in walkdir::WalkDir::new(&src)
                    .follow_links(false)
                    .into_iter()
                    .flatten()
                {
                    let path = entry.path();
                    let rel = match path.strip_prefix(old_root) {
                        Ok(r) => r,
                        Err(_) => continue,
                    };
                    let rel_posix = rel.to_string_lossy().replace('\\', "/");
                    if rel_posix.is_empty() || rel_posix.ends_with(".lock") {
                        continue;
                    }
                    if entry.file_type().is_dir() {
                        let _ = zip.add_directory(format!("{rel_posix}/"), opts);
                    } else if entry.file_type().is_file()
                        && zip.start_file(&rel_posix, opts).is_ok()
                    {
                        if let Ok(mut f) = fs::File::open(path) {
                            let _ = std::io::copy(&mut f, &mut zip);
                        }
                    }
                }
            }
            zip.flush().map_err(|source| StorageError::Io {
                path: backup_path.display().to_string(),
                source,
            })?;
            zip.finish().map_err(|e| {
                StorageError::Backup(format!("Failed to finalize relocation backup: {e}"))
            })?;
        }

        for sub in USER_SUBDIRS {
            let src = old_root.join(sub);
            if !src.is_dir() {
                continue;
            }
            for entry in walkdir::WalkDir::new(&src)
                .follow_links(false)
                .into_iter()
                .flatten()
            {
                let path = entry.path();
                let rel = match path.strip_prefix(old_root) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                if path.extension().and_then(|e| e.to_str()) == Some("lock") {
                    continue;
                }
                let dest = new_root.join(rel);
                if entry.file_type().is_dir() {
                    fs::create_dir_all(&dest).map_err(|source| StorageError::Io {
                        path: dest.display().to_string(),
                        source,
                    })?;
                } else if entry.file_type().is_file() {
                    if let Some(parent) = dest.parent() {
                        fs::create_dir_all(parent).map_err(|source| StorageError::Io {
                            path: parent.display().to_string(),
                            source,
                        })?;
                    }
                    fs::copy(path, &dest).map_err(|source| StorageError::Io {
                        path: dest.display().to_string(),
                        source,
                    })?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_rejects_traversal() {
        let base = Path::new("cache/libraries");
        assert!(LauncherPaths::safe_join_relative(base, "../evil.jar").is_err());
        assert!(LauncherPaths::safe_join_relative(base, "/abs/path.jar").is_err());
        assert!(LauncherPaths::safe_join_relative(base, "C:/evil.jar").is_err());
        assert!(LauncherPaths::safe_join_relative(base, "org/lwjgl/lwjgl.jar").is_ok());
    }
}
