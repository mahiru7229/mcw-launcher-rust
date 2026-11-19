use crate::{ModError, ModInfo, ModScanner, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub struct ModManager<'a> {
    mods_dir: &'a Path,
}

impl<'a> ModManager<'a> {
    pub fn new(mods_dir: &'a Path) -> Self {
        Self { mods_dir }
    }

    pub fn list(&self) -> Vec<ModInfo> {
        ModScanner::scan_dir(self.mods_dir)
    }

    pub fn set_enabled(&self, file_name: &str, enable: bool) -> Result<PathBuf> {
        let src = self.safe_mod_path(file_name)?;
        if !src.is_file() {
            return Err(ModError::NotFound(file_name.to_string()));
        }

        let lower = file_name.to_ascii_lowercase();
        let target_name = if enable {
            if lower.ends_with(".disabled") {
                file_name[..file_name.len() - ".disabled".len()].to_string()
            } else {
                return Ok(src);
            }
        } else if lower.ends_with(".disabled") {
            return Ok(src);
        } else {
            format!("{file_name}.disabled")
        };

        let dst = self.mods_dir.join(target_name);
        fs::rename(&src, &dst)?;
        Ok(dst)
    }

    pub fn delete(&self, file_name: &str) -> Result<()> {
        let path = self.safe_mod_path(file_name)?;
        if !path.is_file() {
            return Err(ModError::NotFound(file_name.to_string()));
        }
        fs::remove_file(&path)?;
        Ok(())
    }

    fn safe_mod_path(&self, file_name: &str) -> Result<PathBuf> {
        if file_name.is_empty()
            || file_name.contains('/')
            || file_name.contains('\\')
            || file_name == "."
            || file_name == ".."
        {
            return Err(ModError::InvalidFilename(file_name.to_string()));
        }
        Ok(self.mods_dir.join(file_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn toggles_mod_enabled_state_and_deletes() {
        let dir = tempdir().unwrap();
        let jar = dir.path().join("example.jar");
        fs::write(&jar, b"dummy").unwrap();

        let mgr = ModManager::new(dir.path());
        let disabled = mgr.set_enabled("example.jar", false).unwrap();
        assert!(disabled.ends_with("example.jar.disabled"));
        assert!(disabled.is_file());

        let enabled = mgr.set_enabled("example.jar.disabled", true).unwrap();
        assert!(enabled.ends_with("example.jar"));
        assert!(enabled.is_file());

        mgr.delete("example.jar").unwrap();
        assert!(!enabled.exists());
    }
}
