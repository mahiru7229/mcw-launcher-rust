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

/// Storage boundary contract for reading and persisting Minecraft instances
/// (following `SOLID.md` §4 LSP and §6 DIP).
pub trait InstanceRepository {
    /// Returns `Ok(None)` when the instance does not exist, or `Err` on I/O/parse failure.
    fn find(&self, instance_id: &str) -> Result<Option<Instance>>;
    /// Returns all valid instances ordered by favorite then name.
    fn list(&self) -> Result<Vec<Instance>>;
    /// Persists the supplied instance metadata atomically.
    fn save(&self, instance: &Instance) -> Result<()>;
    /// Loads instance runtime settings.
    fn load_settings(&self, instance_id: &str) -> Result<InstanceSettings>;
    /// Saves instance runtime settings atomically.
    fn save_settings(&self, instance_id: &str, settings: &InstanceSettings) -> Result<()>;
}

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

    pub fn resolve_unique_instance_name(&self, requested_name: &str) -> Result<String> {
        let trimmed = requested_name.trim();
        let fallback_source = if trimmed.is_empty() {
            "Instance"
        } else {
            trimmed
        };
        let base = Self::validate_instance_name(fallback_source)?;
        if !self.paths.instance_dir(&base).exists() {
            return Ok(base);
        }
        for idx in 2usize.. {
            let suffix = format!(" ({idx})");
            let max_base_chars = MAX_INSTANCE_NAME_LEN.saturating_sub(suffix.chars().count());
            let mut truncated_base: String = base.chars().take(max_base_chars).collect();
            while truncated_base.ends_with('.') || truncated_base.ends_with(' ') {
                truncated_base.pop();
            }
            let prefix = if truncated_base.is_empty() {
                "Instance"
            } else {
                truncated_base.as_str()
            };
            let candidate = format!("{prefix}{suffix}");
            if !self.paths.instance_dir(&candidate).exists() {
                return Ok(candidate);
            }
        }
        Ok(base)
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
}

impl<'a> InstanceRepository for InstanceStore<'a> {
    fn find(&self, instance_id: &str) -> Result<Option<Instance>> {
        if !self.paths.instance_json(instance_id).is_file() {
            return Ok(None);
        }
        self.load_instance(instance_id).map(Some)
    }

    fn list(&self) -> Result<Vec<Instance>> {
        self.list_instances()
    }

    fn save(&self, instance: &Instance) -> Result<()> {
        self.save_instance(instance)
    }

    fn load_settings(&self, instance_id: &str) -> Result<InstanceSettings> {
        InstanceStore::load_settings(self, instance_id)
    }

    fn save_settings(&self, instance_id: &str, settings: &InstanceSettings) -> Result<()> {
        InstanceStore::save_settings(self, instance_id, settings)
    }
}

impl<'a> InstanceStore<'a> {
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
    pub fn set_instance_icon_bytes_with_origin(
        &self,
        instance_id: &str,
        bytes: &[u8],
        origin: &str,
    ) -> Result<Instance> {
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
        instance.icon_origin = origin.trim().to_string();
        self.save_instance(&instance)?;
        Ok(instance)
    }

    /// Save raw image bytes as a custom instance icon at `<instance_dir>/.mcw/instance-icon.<ext>`.
    pub fn set_instance_icon_bytes(&self, instance_id: &str, bytes: &[u8]) -> Result<Instance> {
        self.set_instance_icon_bytes_with_origin(instance_id, bytes, "custom")
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
        instance.icon_origin = "default".to_string();
        self.save_instance(&instance)?;
        Ok(instance)
    }

    /// Update an existing instance's target Minecraft version and ModLoader
    /// (port of `InstanceManager.set_runtime_profile` / `set_mod_loader`).
    pub fn set_runtime_profile(
        &self,
        instance_id: &str,
        version_id: &str,
        mod_loader: ModLoader,
    ) -> Result<Instance> {
        if !self.paths.instance_json(instance_id).is_file() {
            return Err(StorageError::InstanceNotFound(instance_id.to_string()));
        }
        if InstanceRunLock::is_running(self.paths, instance_id) {
            return Err(StorageError::InstanceAlreadyRunning(
                instance_id.to_string(),
            ));
        }
        let mut instance = self.load_instance(instance_id)?;
        let trimmed_ver = version_id.trim();
        if !trimmed_ver.is_empty() {
            instance.version_id = trimmed_ver.to_string();
        }
        instance.mod_loader = mod_loader;
        self.save_instance(&instance)?;
        Ok(instance)
    }

    /// Update favorite flag, group, and comma-separated tags on an instance
    /// (port of `InstanceManager.set_library_metadata` and `_normalize_tags`).
    pub fn set_library_metadata(
        &self,
        instance_id: &str,
        favorite: Option<bool>,
        group: Option<&str>,
        tags_csv: Option<&str>,
    ) -> Result<Instance> {
        let mut instance = self.load_instance(instance_id)?;
        if let Some(fav) = favorite {
            instance.favorite = fav;
        }
        if let Some(grp) = group {
            instance.group = grp.trim().to_string();
        }
        if let Some(raw_tags) = tags_csv {
            let mut normalized = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for item in raw_tags.split(',') {
                let tag = item.trim();
                if tag.is_empty() {
                    continue;
                }
                let lower = tag.to_ascii_lowercase();
                if seen.insert(lower) {
                    normalized.push(tag.to_string());
                }
            }
            instance.tags = normalized;
        }
        self.save_instance(&instance)?;
        Ok(instance)
    }

    /// Clone/duplicate an existing instance (port of `InstanceManager.clone`).
    /// Copies the instance tree while omitting `logs/`, `crash-reports/`, `.mcw/run.lock`
    /// (and `saves/` when `include_saves == false`), and resets runtime play counters.
    pub fn clone_instance(
        &self,
        source_id: &str,
        new_name: &str,
        include_saves: bool,
    ) -> Result<Instance> {
        if !self.paths.instance_json(source_id).is_file() {
            return Err(StorageError::InstanceNotFound(source_id.to_string()));
        }
        let new_id = Self::validate_instance_name(new_name)?;
        let source_dir = self.paths.instance_dir(source_id);
        let target_dir = self.paths.instance_dir(&new_id);
        if target_dir.exists() {
            return Err(StorageError::InstanceAlreadyExists(new_id));
        }

        copy_instance_tree_filtered(&source_dir, &source_dir, &target_dir, include_saves).map_err(
            |source| StorageError::Io {
                path: target_dir.display().to_string(),
                source,
            },
        )?;

        let mut cloned = self.load_instance(&new_id)?;
        cloned.instance_id = new_id.clone();
        cloned.name = new_id;
        cloned.last_played.clear();
        cloned.total_playtime_seconds = 0;
        cloned.last_exit_code = None;
        cloned.last_launch_crashed = false;
        cloned.last_launch_state = "ready".to_string();
        self.save_instance(&cloned)?;
        Ok(cloned)
    }
}

fn copy_instance_tree_filtered(
    root: &std::path::Path,
    current: &std::path::Path,
    dest_root: &std::path::Path,
    include_saves: bool,
) -> std::io::Result<()> {
    fs::create_dir_all(dest_root)?;
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        // Skip volatile runtime logs/crash-reports/locks and optionally saves
        if rel_str == "run.lock"
            || rel_str == ".mcw/run.lock"
            || rel_str == ".mcw/runtime-history.json"
            || rel_str == ".mcw/last-repair.json"
            || rel_str == ".minecraft/logs"
            || rel_str.starts_with(".minecraft/logs/")
            || rel_str == ".minecraft/crash-reports"
            || rel_str.starts_with(".minecraft/crash-reports/")
        {
            continue;
        }
        if !include_saves
            && (rel_str == ".minecraft/saves" || rel_str.starts_with(".minecraft/saves/"))
        {
            continue;
        }
        let target = dest_root.join(rel);
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            fs::create_dir_all(&target)?;
            copy_instance_tree_filtered(root, &path, dest_root, include_saves)?;
        } else if file_type.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
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

    #[test]
    fn clones_and_updates_runtime_and_library_metadata() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();
        let store = InstanceStore::new(&paths);

        store
            .create_instance("source", "1.20.1", ModLoader::vanilla())
            .unwrap();
        let mc_dir = paths.minecraft_dir("source");
        fs::create_dir_all(mc_dir.join("mods")).unwrap();
        fs::create_dir_all(mc_dir.join("saves/world1")).unwrap();
        fs::write(mc_dir.join("mods/a.jar"), b"mod").unwrap();
        fs::write(mc_dir.join("saves/world1/level.dat"), b"world").unwrap();

        let updated = store
            .set_runtime_profile(
                "source",
                "1.21.4",
                ModLoader {
                    name: "fabric".to_string(),
                    version: "0.16.10".to_string(),
                },
            )
            .unwrap();
        assert_eq!(updated.version_id, "1.21.4");
        assert_eq!(updated.mod_loader.name, "fabric");

        let meta = store
            .set_library_metadata(
                "source",
                Some(true),
                Some("Modded"),
                Some(" RPG, tech, rpg "),
            )
            .unwrap();
        assert!(meta.favorite);
        assert_eq!(meta.group, "Modded");
        assert_eq!(meta.tags, vec!["RPG".to_string(), "tech".to_string()]);

        let cloned_no_saves = store.clone_instance("source", "cloned_a", false).unwrap();
        assert_eq!(cloned_no_saves.instance_id, "cloned_a");
        assert!(paths.minecraft_dir("cloned_a").join("mods/a.jar").is_file());
        assert!(!paths
            .minecraft_dir("cloned_a")
            .join("saves/world1/level.dat")
            .exists());

        let cloned_with_saves = store.clone_instance("source", "cloned_b", true).unwrap();
        assert_eq!(cloned_with_saves.instance_id, "cloned_b");
        assert!(paths
            .minecraft_dir("cloned_b")
            .join("saves/world1/level.dat")
            .is_file());
    }
}
