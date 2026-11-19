use crate::{ModError, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentKind {
    ResourcePack,
    ShaderPack,
}

impl ContentKind {
    pub fn folder_name(self) -> &'static str {
        match self {
            Self::ResourcePack => "resourcepacks",
            Self::ShaderPack => "shaderpacks",
        }
    }

    pub fn modrinth_project_type(self) -> &'static str {
        match self {
            Self::ResourcePack => "resourcepack",
            Self::ShaderPack => "shader",
        }
    }

    pub fn curseforge_kind(self) -> &'static str {
        match self {
            Self::ResourcePack => "resourcepack",
            Self::ShaderPack => "shader",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContentPackInfo {
    pub file_name: String,
    pub display_name: String,
    pub enabled: bool,
    pub size_bytes: u64,
    pub is_dir: bool,
    pub path: PathBuf,
}

pub struct ContentPackManager;

impl ContentPackManager {
    pub fn list(mc_dir: &Path, kind: ContentKind) -> Vec<ContentPackInfo> {
        let dir = mc_dir.join(kind.folder_name());
        let Ok(entries) = fs::read_dir(&dir) else {
            return Vec::new();
        };

        let mut items = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with('.') {
                continue;
            }
            let is_dir = path.is_dir();
            let lower = file_name.to_ascii_lowercase();
            let (enabled, display_name) = if is_dir {
                let en = !lower.ends_with(".disabled");
                let disp = file_name
                    .strip_suffix(".disabled")
                    .unwrap_or(&file_name)
                    .to_string();
                (en, disp)
            } else if lower.ends_with(".zip") {
                let disp = file_name
                    .strip_suffix(".zip")
                    .or_else(|| file_name.strip_suffix(".ZIP"))
                    .unwrap_or(&file_name)
                    .to_string();
                (true, disp)
            } else if lower.ends_with(".zip.disabled") {
                let base = &file_name[..file_name.len() - ".zip.disabled".len()];
                (false, base.to_string())
            } else {
                continue;
            };

            let size_bytes = if is_dir {
                0
            } else {
                entry.metadata().map(|m| m.len()).unwrap_or(0)
            };

            items.push(ContentPackInfo {
                file_name,
                display_name,
                enabled,
                size_bytes,
                is_dir,
                path,
            });
        }

        items.sort_by_key(|a| a.display_name.to_ascii_lowercase());
        items
    }

    pub fn set_enabled(
        mc_dir: &Path,
        kind: ContentKind,
        file_name: &str,
        enable: bool,
    ) -> Result<PathBuf> {
        validate_name(file_name)?;
        let dir = mc_dir.join(kind.folder_name());
        let src = dir.join(file_name);
        if !src.exists() {
            return Err(ModError::NotFound(file_name.to_string()));
        }

        let new_name = if enable {
            file_name
                .strip_suffix(".disabled")
                .unwrap_or(file_name)
                .to_string()
        } else if file_name.ends_with(".disabled") {
            file_name.to_string()
        } else {
            format!("{file_name}.disabled")
        };

        let dst = dir.join(&new_name);
        if src != dst {
            fs::rename(&src, &dst)?;
        }
        Ok(dst)
    }

    pub fn delete(mc_dir: &Path, kind: ContentKind, file_name: &str) -> Result<()> {
        validate_name(file_name)?;
        let target = mc_dir.join(kind.folder_name()).join(file_name);
        if target.is_dir() {
            fs::remove_dir_all(&target)?;
        } else if target.is_file() {
            fs::remove_file(&target)?;
        }
        Ok(())
    }
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(ModError::InvalidFilename(name.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn lists_toggles_and_deletes_resource_and_shader_packs() {
        let dir = tempdir().unwrap();
        let rp_dir = dir.path().join("resourcepacks");
        fs::create_dir_all(&rp_dir).unwrap();
        fs::write(rp_dir.join("Faithful.zip"), b"zip-bytes").unwrap();

        let list = ContentPackManager::list(dir.path(), ContentKind::ResourcePack);
        assert_eq!(list.len(), 1);
        assert!(list[0].enabled);
        assert_eq!(list[0].display_name, "Faithful");

        ContentPackManager::set_enabled(
            dir.path(),
            ContentKind::ResourcePack,
            "Faithful.zip",
            false,
        )
        .unwrap();
        let list2 = ContentPackManager::list(dir.path(), ContentKind::ResourcePack);
        assert!(!list2[0].enabled);
        assert_eq!(list2[0].file_name, "Faithful.zip.disabled");

        ContentPackManager::delete(
            dir.path(),
            ContentKind::ResourcePack,
            "Faithful.zip.disabled",
        )
        .unwrap();
        assert!(ContentPackManager::list(dir.path(), ContentKind::ResourcePack).is_empty());
    }
}
