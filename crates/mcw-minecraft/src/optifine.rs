use crate::{MinecraftError, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use zip::ZipArchive;

const MAX_OPTIFINE_SIZE: u64 = 128 * 1024 * 1024;
const REQUIRED_CLASSES: &[&str] = &[
    "optifine/Installer.class",
    "optifine/Config.class",
    "net/optifine/Config.class",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptiFineVersion {
    pub minecraft_version: String,
    pub edition: String,
    pub is_preview: bool,
    pub filename: String,
}

impl OptiFineVersion {
    /// Parses official OptiFine JAR filenames such as:
    /// - `OptiFine_1.20.4_HD_U_I7.jar`
    /// - `preview_OptiFine_1.21.4_HD_U_J3_pre12.jar`
    pub fn parse_filename(filename: &str) -> Option<Self> {
        let trimmed = filename.trim();
        let stem = trimmed
            .strip_suffix(".jar")
            .or_else(|| trimmed.strip_suffix(".JAR"))?;

        let (is_preview, rest) = if let Some(r) = stem.strip_prefix("preview_OptiFine_") {
            (true, r)
        } else {
            let r = stem.strip_prefix("OptiFine_")?;
            (false, r)
        };

        let (mc_ver, edition) = rest.split_once('_')?;
        if mc_ver.is_empty() || edition.is_empty() || !mc_ver.chars().next()?.is_ascii_digit() {
            return None;
        }

        Some(Self {
            minecraft_version: mc_ver.to_string(),
            edition: edition.to_string(),
            is_preview,
            filename: trimmed.to_string(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct InspectedOptiFine {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub version: OptiFineVersion,
}

pub struct OptiFineManager;

impl OptiFineManager {
    /// Inspects and validates a local OptiFine `.jar` file.
    pub fn inspect(
        jar_path: &Path,
        expected_mc_version: Option<&str>,
    ) -> Result<InspectedOptiFine> {
        if !jar_path.is_file() {
            return Err(MinecraftError::Io(io::Error::new(
                io::ErrorKind::NotFound,
                format!("OptiFine JAR not found: {}", jar_path.display()),
            )));
        }
        let meta = fs::metadata(jar_path)?;
        if meta.len() == 0 || meta.len() > MAX_OPTIFINE_SIZE {
            return Err(MinecraftError::InvalidMetadata(
                "Selected OptiFine JAR is empty or unexpectedly large.".to_string(),
            ));
        }

        let file_name = jar_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                MinecraftError::InvalidMetadata("Invalid OptiFine filename.".to_string())
            })?;

        let version = OptiFineVersion::parse_filename(file_name).ok_or_else(|| {
            MinecraftError::InvalidMetadata(
                "Filename does not match official OptiFine format (e.g. OptiFine_1.20.4_HD_U_I7.jar)."
                    .to_string(),
            )
        })?;

        if let Some(expected) = expected_mc_version.map(str::trim).filter(|s| !s.is_empty()) {
            if version.minecraft_version != expected {
                return Err(MinecraftError::InvalidMetadata(format!(
                    "OptiFine JAR targets Minecraft {}, but this instance is Minecraft {expected}.",
                    version.minecraft_version
                )));
            }
        }

        let file = File::open(jar_path)?;
        let mut archive = ZipArchive::new(file)?;
        let mut has_manifest = false;
        let mut has_optifine_class = false;
        for i in 0..archive.len() {
            let entry = archive.by_index(i)?;
            let name = entry.name().replace('\\', "/");
            if name == "META-INF/MANIFEST.MF" {
                has_manifest = true;
            }
            if REQUIRED_CLASSES.iter().any(|req| *req == name) {
                has_optifine_class = true;
            }
            if has_manifest && has_optifine_class {
                break;
            }
        }

        if !has_manifest || !has_optifine_class {
            return Err(MinecraftError::InvalidMetadata(
                "Selected JAR does not contain valid OptiFine classes and manifest.".to_string(),
            ));
        }

        Ok(InspectedOptiFine {
            path: jar_path.to_path_buf(),
            size_bytes: meta.len(),
            version,
        })
    }

    /// Detects an installed OptiFine JAR inside `<mc_dir>/mods/`.
    pub fn detect_installed_in_mods(mc_dir: &Path) -> Option<String> {
        let mods_dir = mc_dir.join("mods");
        let entries = fs::read_dir(mods_dir).ok()?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let lower = name.to_ascii_lowercase();
            if lower.contains("optifine") && lower.ends_with(".jar") {
                return Some(name);
            }
        }
        None
    }

    /// Installs a validated OptiFine JAR into `<mc_dir>/mods/` (for Forge / NeoForge instances),
    /// replacing any older `OptiFine*.jar` already present in `mods/`.
    pub fn install_into_mods(
        jar_path: &Path,
        mc_dir: &Path,
        expected_mc_version: Option<&str>,
    ) -> Result<PathBuf> {
        let inspected = Self::inspect(jar_path, expected_mc_version)?;
        let mods_dir = mc_dir.join("mods");
        fs::create_dir_all(&mods_dir)?;

        // Remove older OptiFine jars in mods/ to avoid duplicate class conflicts
        if let Ok(entries) = fs::read_dir(&mods_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if name.contains("optifine") && name.ends_with(".jar") {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }

        let dest = mods_dir.join(&inspected.version.filename);
        fs::copy(&inspected.path, &dest)?;
        Ok(dest)
    }

    /// Launches the official OptiFine GUI installer using the resolved Java runtime,
    /// useful for standalone (Vanilla) OptiFine installation.
    pub fn launch_official_installer(
        jar_path: &Path,
        java_exe: &Path,
        expected_mc_version: Option<&str>,
    ) -> Result<()> {
        let inspected = Self::inspect(jar_path, expected_mc_version)?;
        Command::new(java_exe)
            .arg("-jar")
            .arg(&inspected.path)
            .spawn()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    #[test]
    fn parses_stable_and_preview_optifine_filenames() {
        let stable = OptiFineVersion::parse_filename("OptiFine_1.20.4_HD_U_I7.jar").unwrap();
        assert_eq!(stable.minecraft_version, "1.20.4");
        assert_eq!(stable.edition, "HD_U_I7");
        assert!(!stable.is_preview);

        let preview =
            OptiFineVersion::parse_filename("preview_OptiFine_1.21.4_HD_U_J3_pre12.jar").unwrap();
        assert_eq!(preview.minecraft_version, "1.21.4");
        assert_eq!(preview.edition, "HD_U_J3_pre12");
        assert!(preview.is_preview);

        assert!(OptiFineVersion::parse_filename("sodium-fabric-0.6.jar").is_none());
    }

    #[test]
    fn validates_and_installs_optifine_jar_into_mods() {
        let tmp = tempdir().unwrap();
        let jar_path = tmp.path().join("OptiFine_1.20.4_HD_U_I7.jar");
        {
            let f = File::create(&jar_path).unwrap();
            let mut zw = zip::ZipWriter::new(f);
            zw.start_file("META-INF/MANIFEST.MF", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(b"Manifest-Version: 1.0\n").unwrap();
            zw.start_file("optifine/Config.class", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(b"\xCA\xFE\xBA\xBE").unwrap();
            zw.finish().unwrap();
        }

        // Wrong MC version should fail
        assert!(OptiFineManager::inspect(&jar_path, Some("1.21.4")).is_err());

        // Matching MC version should install into mods/
        let mc_dir = tmp.path().join(".minecraft");
        let dest = OptiFineManager::install_into_mods(&jar_path, &mc_dir, Some("1.20.4")).unwrap();
        assert!(dest.is_file());
        assert_eq!(
            OptiFineManager::detect_installed_in_mods(&mc_dir).as_deref(),
            Some("OptiFine_1.20.4_HD_U_I7.jar")
        );
    }
}
