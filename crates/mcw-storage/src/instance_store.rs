use crate::{atomic_write_json, InstanceRunLock, LauncherPaths, Result, StorageError};
use mcw_models::instance::{Instance, InstanceSettings, ModLoader};
use std::fs;
use std::thread;
use std::time::Duration;

const MAX_INSTANCE_NAME_LEN: usize = 80;
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
        if trimmed.chars().count() > MAX_INSTANCE_NAME_LEN {
            return Err(StorageError::InvalidInstanceName {
                name: name.to_string(),
                reason: format!("name exceeds {MAX_INSTANCE_NAME_LEN} characters"),
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

    pub fn list_instances(&self) -> Result<Vec<Instance>> {
        if !self.paths.instances_root.is_dir() {
            return Ok(Vec::new());
        }
        let entries =
            fs::read_dir(&self.paths.instances_root).map_err(|source| StorageError::Io {
                path: self.paths.instances_root.display().to_string(),
                source,
            })?;

        let mut instances = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let inst_json = path.join("instance.json");
            if !inst_json.is_file() {
                continue;
            }
            if let Ok(raw) = fs::read_to_string(&inst_json) {
                if let Ok(inst) = serde_json::from_str::<Instance>(&raw) {
                    instances.push(inst);
                }
            }
        }

        instances.sort_by(|a, b| {
            b.favorite
                .cmp(&a.favorite)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(instances)
    }

    pub fn create_instance(
        &self,
        name: &str,
        version_id: &str,
        mod_loader: ModLoader,
    ) -> Result<(Instance, InstanceSettings)> {
        let instance_id = Self::validate_instance_name(name)?;
        let inst_dir = self.paths.instance_dir(&instance_id);
        if inst_dir.exists() {
            return Err(StorageError::InstanceAlreadyExists(instance_id));
        }

        let mc_dir = self.paths.minecraft_dir(&instance_id);
        fs::create_dir_all(&mc_dir).map_err(|source| StorageError::Io {
            path: mc_dir.display().to_string(),
            source,
        })?;

        let mut instance = Instance::new(&instance_id, &instance_id, version_id);
        instance.mod_loader = mod_loader;
        self.save_instance(&instance)?;

        let settings = InstanceSettings::default();
        self.save_settings(&instance_id, &settings)?;

        Ok((instance, settings))
    }

    pub fn get_instance(&self, instance_id: &str) -> Result<(Instance, InstanceSettings)> {
        let inst_json = self.paths.instance_json(instance_id);
        if !inst_json.is_file() {
            return Err(StorageError::InstanceNotFound(instance_id.to_string()));
        }
        let instance = self.load_instance(instance_id)?;
        let settings = if self.paths.instance_settings_json(instance_id).is_file() {
            self.load_settings(instance_id)?
        } else {
            let defaults = InstanceSettings::default();
            self.save_settings(instance_id, &defaults)?;
            defaults
        };
        Ok((instance, settings))
    }

    pub fn ensure_instance(
        &self,
        name: &str,
        version_id: &str,
    ) -> Result<(Instance, InstanceSettings)> {
        let instance_id = Self::validate_instance_name(name)?;
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

        Ok((instance, settings))
    }

    pub fn rename_instance(&self, old_id: &str, new_name: &str) -> Result<Instance> {
        if !self.paths.instance_json(old_id).is_file() {
            return Err(StorageError::InstanceNotFound(old_id.to_string()));
        }
        if InstanceRunLock::is_running(self.paths, old_id) {
            return Err(StorageError::InstanceAlreadyRunning(old_id.to_string()));
        }

        let new_id = Self::validate_instance_name(new_name)?;
        if old_id == new_id {
            return self.load_instance(old_id);
        }

        let old_dir = self.paths.instance_dir(old_id);
        let new_dir = self.paths.instance_dir(&new_id);
        if new_dir.exists() {
            return Err(StorageError::InstanceAlreadyExists(new_id));
        }

        for attempt in 0..=5 {
            match fs::rename(&old_dir, &new_dir) {
                Ok(()) => break,
                Err(err) if attempt < 5 && err.kind() == std::io::ErrorKind::PermissionDenied => {
                    thread::sleep(Duration::from_millis(50 * (attempt as u64 + 1)));
                }
                Err(source) => {
                    return Err(StorageError::Io {
                        path: new_dir.display().to_string(),
                        source,
                    });
                }
            }
        }

        let mut instance = self.load_instance(&new_id)?;
        instance.instance_id = new_id.clone();
        instance.name = new_id;
        self.save_instance(&instance)?;
        Ok(instance)
    }

    pub fn delete_instance(&self, instance_id: &str) -> Result<()> {
        let dir = self.paths.instance_dir(instance_id);
        if !dir.exists() {
            return Err(StorageError::InstanceNotFound(instance_id.to_string()));
        }
        if InstanceRunLock::is_running(self.paths, instance_id) {
            return Err(StorageError::InstanceAlreadyRunning(
                instance_id.to_string(),
            ));
        }

        fs::remove_dir_all(&dir).map_err(|source| StorageError::Io {
            path: dir.display().to_string(),
            source,
        })
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

    /// Resolve the on-disk icon file path for an instance, supporting both Python's
    /// `.mcw/instance-icon.<ext>` relative paths, custom absolute paths, and legacy `icon.png`.
    pub fn resolve_instance_icon_path(&self, instance: &Instance) -> Option<std::path::PathBuf> {
        let icon = instance.icon.trim();
        let inst_dir = self.paths.instance_dir(&instance.instance_id);
        if !icon.is_empty() && !icon.eq_ignore_ascii_case("grass_block") {
            let candidate = std::path::Path::new(icon);
            if candidate.is_absolute() && candidate.is_file() {
                return Some(candidate.to_path_buf());
            }
            let rel = inst_dir.join(icon);
            if rel.is_file() {
                return Some(rel);
            }
        }
        // Fallback: check `.mcw/instance-icon.*` or `icon.png` in instance dir
        let mcw_dir = inst_dir.join(".mcw");
        for ext in ["png", "jpg", "jpeg", "webp", "bmp", "ico"] {
            let p = mcw_dir.join(format!("instance-icon.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
        let legacy = inst_dir.join("icon.png");
        if legacy.is_file() {
            return Some(legacy);
        }
        None
    }

    /// Save raw image bytes as the instance icon at `<instance_dir>/.mcw/instance-icon.<ext>`,
    /// detecting format from magic bytes (matching Python `instance_artwork_manager.py`).
    pub fn set_instance_icon_bytes(&self, instance_id: &str, bytes: &[u8]) -> Result<Instance> {
        if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
            return Err(StorageError::Io {
                path: instance_id.to_string(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Icon file must be between 1 byte and 8 MiB",
                ),
            });
        }
        let ext = detect_image_extension(bytes).unwrap_or("png");
        let inst_dir = self.paths.instance_dir(instance_id);
        let mcw_dir = inst_dir.join(".mcw");
        fs::create_dir_all(&mcw_dir).map_err(|source| StorageError::Io {
            path: mcw_dir.display().to_string(),
            source,
        })?;
        // Remove any previous icon with a different extension
        for old_ext in ["png", "jpg", "jpeg", "webp", "bmp", "ico"] {
            let old = mcw_dir.join(format!("instance-icon.{old_ext}"));
            if old.is_file() {
                let _ = fs::remove_file(&old);
            }
        }
        let dest = mcw_dir.join(format!("instance-icon.{ext}"));
        crate::atomic_fs::atomic_write_bytes(&dest, bytes)?;

        let mut instance = self.load_instance(instance_id)?;
        instance.icon = format!(".mcw/instance-icon.{ext}");
        self.save_instance(&instance)?;
        Ok(instance)
    }

    /// Copy a user-selected image file as the instance icon.
    pub fn set_instance_icon(
        &self,
        instance_id: &str,
        src_path: &std::path::Path,
    ) -> Result<Instance> {
        let bytes = fs::read(src_path).map_err(|source| StorageError::Io {
            path: src_path.display().to_string(),
            source,
        })?;
        self.set_instance_icon_bytes(instance_id, &bytes)
    }

    /// Reset an instance's icon back to the default `"grass_block"`.
    pub fn reset_instance_icon(&self, instance_id: &str) -> Result<Instance> {
        let inst_dir = self.paths.instance_dir(instance_id);
        let mcw_dir = inst_dir.join(".mcw");
        for ext in ["png", "jpg", "jpeg", "webp", "bmp", "ico"] {
            let old = mcw_dir.join(format!("instance-icon.{ext}"));
            if old.is_file() {
                let _ = fs::remove_file(&old);
            }
        }
        let mut instance = self.load_instance(instance_id)?;
        instance.icon = "grass_block".to_string();
        self.save_instance(&instance)?;
        Ok(instance)
    }
}

fn detect_image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        Some("jpg")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else if bytes.starts_with(b"BM") {
        Some("bmp")
    } else if bytes.len() >= 4 && bytes[0..4] == [0x00, 0x00, 0x01, 0x00] {
        Some("ico")
    } else {
        None
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
        assert!(InstanceStore::validate_instance_name(&"a".repeat(81)).is_err());
        assert!(InstanceStore::validate_instance_name("CON").is_err());
        assert!(InstanceStore::validate_instance_name("nul.txt").is_err());
        assert!(InstanceStore::validate_instance_name("bad:name").is_err());
        assert!(InstanceStore::validate_instance_name("..").is_err());
        assert!(InstanceStore::validate_instance_name("trailing.").is_err());
    }

    #[test]
    fn crud_lifecycle_and_lock_protection() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();
        let store = InstanceStore::new(&paths);

        // Create
        let (inst, _) = store
            .create_instance("alpha", "1.21.4", ModLoader::vanilla())
            .unwrap();
        assert_eq!(inst.instance_id, "alpha");
        assert!(matches!(
            store.create_instance("alpha", "1.21.4", ModLoader::vanilla()),
            Err(StorageError::InstanceAlreadyExists(_))
        ));

        // List
        let listed = store.list_instances().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "alpha");

        // Block rename & delete while locked
        {
            let _lock = InstanceRunLock::acquire(&paths, "alpha").unwrap();
            assert!(matches!(
                store.rename_instance("alpha", "beta"),
                Err(StorageError::InstanceAlreadyRunning(_))
            ));
            assert!(matches!(
                store.delete_instance("alpha"),
                Err(StorageError::InstanceAlreadyRunning(_))
            ));
        }

        // Rename after lock released
        let renamed = store.rename_instance("alpha", "beta").unwrap();
        assert_eq!(renamed.instance_id, "beta");
        assert!(!paths.instance_dir("alpha").exists());
        assert!(paths.instance_dir("beta").is_dir());

        // Delete
        store.delete_instance("beta").unwrap();
        assert!(store.list_instances().unwrap().is_empty());
    }
}
