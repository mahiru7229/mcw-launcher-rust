use crate::{atomic_write_json, LauncherPaths, Result, StorageError};
use mcw_models::instance::{Instance, InstanceSettings};
use std::fs;

const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

const INVALID_FILENAME_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

pub struct InstanceStore<'a> {
    paths: &'a LauncherPaths,
}

impl<'a> InstanceStore<'a> {
    pub fn new(paths: &'a LauncherPaths) -> Self {
        Self { paths }
    }

    pub fn validate_instance_name(name: &str) -> Result<String> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(StorageError::InvalidInstanceName {
                name: name.to_string(),
                reason: "name cannot be empty".to_string(),
            });
        }
        if trimmed == "." || trimmed == ".." || trimmed.ends_with('.') || trimmed.ends_with(' ') {
            return Err(StorageError::InvalidInstanceName {
                name: name.to_string(),
                reason: "name cannot be '.'/'..' or end with a dot/space".to_string(),
            });
        }
        if trimmed
            .chars()
            .any(|c| c.is_control() || INVALID_FILENAME_CHARS.contains(&c))
        {
            return Err(StorageError::InvalidInstanceName {
                name: name.to_string(),
                reason: "name contains invalid Windows filename characters".to_string(),
            });
        }
        let stem = trimmed.split('.').next().unwrap_or(trimmed).to_uppercase();
        if WINDOWS_RESERVED_NAMES.contains(&stem.as_str()) {
            return Err(StorageError::InvalidInstanceName {
                name: name.to_string(),
                reason: "name is a Windows reserved device name".to_string(),
            });
        }
        Ok(trimmed.to_string())
    }

    pub fn ensure_instance(
        &self,
        name: &str,
        version_id: &str,
    ) -> Result<(Instance, InstanceSettings)> {
        let instance_id = Self::validate_instance_name(name)?;
        let inst_dir = self.paths.instance_dir(&instance_id);
        let mc_dir = self.paths.minecraft_dir(&instance_id);
        fs::create_dir_all(&mc_dir).map_err(|source| StorageError::Io {
            path: mc_dir.display().to_string(),
            source,
        })?;

        let inst_json = self.paths.instance_json(&instance_id);
        let instance = if inst_json.is_file() {
            let mut loaded = self.load_instance(&instance_id)?;
            if loaded.version_id != version_id {
                loaded.version_id = version_id.to_string();
                self.save_instance(&loaded)?;
            }
            loaded
        } else {
            let created = Instance::new(&instance_id, &instance_id, version_id);
            self.save_instance(&created)?;
            created
        };

        let settings_json = self.paths.instance_settings_json(&instance_id);
        let settings = if settings_json.is_file() {
            self.load_settings(&instance_id)?
        } else {
            let default_settings = InstanceSettings::default();
            self.save_settings(&instance_id, &default_settings)?;
            default_settings
        };

        let _ = inst_dir;
        Ok((instance, settings))
    }

    pub fn load_instance(&self, instance_id: &str) -> Result<Instance> {
        let path = self.paths.instance_json(instance_id);
        let raw = fs::read_to_string(&path).map_err(|source| StorageError::Io {
            path: path.display().to_string(),
            source,
        })?;
        serde_json::from_str(&raw).map_err(|source| StorageError::Json {
            path: path.display().to_string(),
            source,
        })
    }

    pub fn save_instance(&self, instance: &Instance) -> Result<()> {
        let path = self.paths.instance_json(&instance.instance_id);
        atomic_write_json(&path, instance)
    }

    pub fn load_settings(&self, instance_id: &str) -> Result<InstanceSettings> {
        let path = self.paths.instance_settings_json(instance_id);
        let raw = fs::read_to_string(&path).map_err(|source| StorageError::Io {
            path: path.display().to_string(),
            source,
        })?;
        serde_json::from_str(&raw).map_err(|source| StorageError::Json {
            path: path.display().to_string(),
            source,
        })
    }

    pub fn save_settings(&self, instance_id: &str, settings: &InstanceSettings) -> Result<()> {
        let path = self.paths.instance_settings_json(instance_id);
        atomic_write_json(&path, settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn validates_windows_instance_names() {
        assert!(InstanceStore::validate_instance_name("test").is_ok());
        assert!(InstanceStore::validate_instance_name("Survival 1.21.4").is_ok());
        assert!(InstanceStore::validate_instance_name("").is_err());
        assert!(InstanceStore::validate_instance_name("CON").is_err());
        assert!(InstanceStore::validate_instance_name("nul.txt").is_err());
        assert!(InstanceStore::validate_instance_name("bad:name").is_err());
        assert!(InstanceStore::validate_instance_name("..").is_err());
        assert!(InstanceStore::validate_instance_name("trailing.").is_err());
    }

    #[test]
    fn ensure_instance_persists_files() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();
        let store = InstanceStore::new(&paths);

        let (inst, settings) = store.ensure_instance("test", "1.21.4").unwrap();
        assert_eq!(inst.instance_id, "test");
        assert_eq!(inst.version_id, "1.21.4");
        assert_eq!(settings.max_memory, 2048);
        assert!(paths.minecraft_dir("test").is_dir());
        assert!(paths.instance_json("test").is_file());
        assert!(paths.instance_settings_json("test").is_file());
    }
}
