use crate::{ModError, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentKind {
    ResourcePack,
    ShaderPack,
    DataPack,
}

impl ContentKind {
    pub fn folder_name(self) -> &'static str {
        match self {
            Self::ResourcePack => "resourcepacks",
            Self::ShaderPack => "shaderpacks",
            Self::DataPack => "datapacks",
        }
    }

    pub fn modrinth_project_type(self) -> &'static str {
        match self {
            Self::ResourcePack => "resourcepack",
            Self::ShaderPack => "shader",
            Self::DataPack => "datapack",
        }
    }

    pub fn curseforge_kind(self) -> &'static str {
        match self {
            Self::ResourcePack => "resourcepack",
            Self::ShaderPack => "shader",
            Self::DataPack => "datapack",
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
    pub icon_png: Option<Vec<u8>>,
}

const MAX_PACK_ICON_BYTES: usize = 256 * 1024;

fn read_pack_icon_png(path: &Path, is_dir: bool) -> Option<Vec<u8>> {
    use std::io::Read as _;

    if is_dir {
        let icon_path = path.join("pack.png");
        let meta = fs::metadata(&icon_path).ok()?;
        if meta.len() == 0 || meta.len() > MAX_PACK_ICON_BYTES as u64 {
            return None;
        }
        let bytes = fs::read(&icon_path).ok()?;
        return (!bytes.is_empty() && bytes.len() <= MAX_PACK_ICON_BYTES).then_some(bytes);
    }

    let file = fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let idx_opt = if archive.by_name("pack.png").is_ok() {
        Some("pack.png".to_string())
    } else {
        archive
            .file_names()
            .find(|n| n.eq_ignore_ascii_case("pack.png"))
            .map(str::to_string)
    };
    let name = idx_opt?;
    let entry = archive.by_name(&name).ok()?;
    if entry.size() > MAX_PACK_ICON_BYTES as u64 {
        return None;
    }
    let mut buf = Vec::new();
    entry
        .take((MAX_PACK_ICON_BYTES as u64).saturating_add(1))
        .read_to_end(&mut buf)
        .ok()?;
    if buf.is_empty() || buf.len() > MAX_PACK_ICON_BYTES {
        None
    } else {
        Some(buf)
    }
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

            let icon_png = read_pack_icon_png(&path, is_dir);
            items.push(ContentPackInfo {
                file_name,
                display_name,
                enabled,
                size_bytes,
                is_dir,
                path,
                icon_png,
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

    #[test]
    fn supports_datapack_folder_provider_names_and_toggle() {
        assert_eq!(ContentKind::DataPack.folder_name(), "datapacks");
        assert_eq!(ContentKind::DataPack.modrinth_project_type(), "datapack");
        assert_eq!(ContentKind::DataPack.curseforge_kind(), "datapack");

        let dir = tempdir().unwrap();
        let dp_dir = dir.path().join("datapacks");
        fs::create_dir_all(&dp_dir).unwrap();
        fs::write(dp_dir.join("Terralith.zip"), b"dp-zip").unwrap();

        let list = ContentPackManager::list(dir.path(), ContentKind::DataPack);
        assert_eq!(list.len(), 1);
        assert!(list[0].enabled);

        ContentPackManager::set_enabled(dir.path(), ContentKind::DataPack, "Terralith.zip", false)
            .unwrap();
        let list2 = ContentPackManager::list(dir.path(), ContentKind::DataPack);
        assert!(!list2[0].enabled);
    }

    #[test]
    fn reads_pack_png_from_zip_and_directory_packs() {
        use std::io::Write as _;
        use zip::write::SimpleFileOptions;

        let dir = tempdir().unwrap();
        let rp_dir = dir.path().join("resourcepacks");
        fs::create_dir_all(&rp_dir).unwrap();

        let dir_pack = rp_dir.join("DirPack");
        fs::create_dir_all(&dir_pack).unwrap();
        let fake_png = b"\x89PNG\r\n\x1a\nfake-dir-icon";
        fs::write(dir_pack.join("pack.png"), fake_png).unwrap();

        let zip_pack_path = rp_dir.join("ZipPack.zip");
        {
            let file = fs::File::create(&zip_pack_path).unwrap();
            let mut zw = zip::ZipWriter::new(file);
            zw.start_file("pack.png", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(b"\x89PNG\r\n\x1a\nfake-zip-icon").unwrap();
            zw.finish().unwrap();
        }

        let listed = ContentPackManager::list(dir.path(), ContentKind::ResourcePack);
        assert_eq!(listed.len(), 2);
        let dir_item = listed.iter().find(|p| p.file_name == "DirPack").unwrap();
        assert_eq!(dir_item.icon_png.as_deref(), Some(fake_png.as_slice()));
        let zip_item = listed
            .iter()
            .find(|p| p.file_name == "ZipPack.zip")
            .unwrap();
        assert_eq!(
            zip_item.icon_png.as_deref(),
            Some(b"\x89PNG\r\n\x1a\nfake-zip-icon".as_slice())
        );
    }
}
