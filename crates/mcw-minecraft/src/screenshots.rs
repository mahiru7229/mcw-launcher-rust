use crate::{MinecraftError, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone)]
pub struct ScreenshotInfo {
    pub path: PathBuf,
    pub file_name: String,
    pub file_size_bytes: u64,
    pub created_timestamp_secs: u64,
}

pub struct ScreenshotManager;

impl ScreenshotManager {
    pub fn list_screenshots(mc_dir: &Path) -> Vec<ScreenshotInfo> {
        let dir = mc_dir.join("screenshots");
        let Ok(entries) = fs::read_dir(&dir) else {
            return Vec::new();
        };

        let mut items = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default();
            if !matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
                continue;
            }
            let meta = entry.metadata().ok();
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let mtime = meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            items.push(ScreenshotInfo {
                file_name: entry.file_name().to_string_lossy().to_string(),
                path,
                file_size_bytes: size,
                created_timestamp_secs: mtime,
            });
        }

        items.sort_by_key(|a| std::cmp::Reverse(a.created_timestamp_secs));
        items
    }

    pub fn delete_screenshot(path: &Path) -> Result<()> {
        if path.is_file() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    pub fn rename_screenshot(path: &Path, new_name: &str) -> Result<PathBuf> {
        if !path.is_file() {
            return Err(MinecraftError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Screenshot file not found",
            )));
        }
        let trimmed = new_name.trim();
        if trimmed.is_empty()
            || trimmed.contains('/')
            || trimmed.contains('\\')
            || trimmed.contains("..")
            || trimmed.contains(':')
        {
            return Err(MinecraftError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid screenshot name",
            )));
        }
        let orig_ext = path
            .extension()
            .and_then(|e| e.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("png");
        let target_name = if trimmed
            .to_ascii_lowercase()
            .ends_with(&format!(".{}", orig_ext.to_ascii_lowercase()))
        {
            trimmed.to_string()
        } else {
            format!("{trimmed}.{orig_ext}")
        };
        let parent = path.parent().ok_or_else(|| {
            MinecraftError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Screenshot has no parent folder",
            ))
        })?;
        let dest = parent.join(target_name);
        if dest == path {
            return Ok(dest);
        }
        if dest.exists() {
            return Err(MinecraftError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "A screenshot with that name already exists",
            )));
        }
        fs::rename(path, &dest)?;
        Ok(dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn lists_and_deletes_screenshots() {
        let dir = tempdir().unwrap();
        let shots = dir.path().join("screenshots");
        fs::create_dir_all(&shots).unwrap();
        fs::write(shots.join("2026-01-01_12.00.00.png"), b"png-bytes").unwrap();
        fs::write(shots.join("notes.txt"), b"ignore").unwrap();

        let list = ScreenshotManager::list_screenshots(dir.path());
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].file_name, "2026-01-01_12.00.00.png");

        let renamed = ScreenshotManager::rename_screenshot(&list[0].path, "base-entrance").unwrap();
        assert_eq!(
            renamed.file_name().and_then(|n| n.to_str()),
            Some("base-entrance.png")
        );

        ScreenshotManager::delete_screenshot(&renamed).unwrap();
        assert!(ScreenshotManager::list_screenshots(dir.path()).is_empty());
    }
}
