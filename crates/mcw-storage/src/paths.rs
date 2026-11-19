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
