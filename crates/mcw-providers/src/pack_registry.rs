use mcw_storage::atomic_write_json;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtbManagedFile {
    #[serde(rename = "fileId", default)]
    pub file_id: u64,
    #[serde(rename = "fileName", default)]
    pub file_name: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub optional: bool,
    #[serde(rename = "clientOnly", default)]
    pub client_only: bool,
    #[serde(rename = "fileType", default)]
    pub file_type: String,
    #[serde(default = "default_ftb")]
    pub provider: String,
}

fn default_ftb() -> String {
    "ftb".to_string()
}

/// Persisted metadata for an FTB modpack installation (`.mcw/ftb-pack.json`), Schema 2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtbPackRecord {
    #[serde(rename = "schemaVersion", default = "default_ftb_schema")]
    pub schema_version: u32,
    #[serde(default = "default_ftb")]
    pub source: String,
    #[serde(rename = "packId")]
    pub pack_id: u64,
    #[serde(rename = "packName", default)]
    pub pack_name: String,
    #[serde(rename = "versionId")]
    pub version_id: u64,
    #[serde(rename = "versionName", default)]
    pub version_name: String,
    #[serde(rename = "managedFiles", default)]
    pub managed_files: Vec<FtbManagedFile>,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: String,
}

fn default_ftb_schema() -> u32 {
    2
}

pub struct FtbPackRegistry;

impl FtbPackRegistry {
    pub fn registry_path(instance_dir: &Path) -> PathBuf {
        instance_dir.join(".mcw").join("ftb-pack.json")
    }

    pub fn load(instance_dir: &Path) -> Option<FtbPackRecord> {
        let path = Self::registry_path(instance_dir);
        let raw = fs::read_to_string(path).ok()?;
        serde_json::from_str(&raw).ok()
    }

    pub fn save(instance_dir: &Path, record: &FtbPackRecord) -> crate::Result<()> {
        let mut normalized = record.clone();
        normalized.schema_version = 2;
        normalized.source = "ftb".to_string();
        normalized.updated_at = unix_timestamp_string();
        for f in &mut normalized.managed_files {
            f.path = safe_relative_posix(&f.path, &f.file_name);
            f.sha1 = f.sha1.trim().to_ascii_lowercase();
            f.provider = "ftb".to_string();
        }
        normalized
            .managed_files
            .sort_by_key(|a| a.path.to_ascii_lowercase());
        atomic_write_json(&Self::registry_path(instance_dir), &normalized)
            .map_err(|e| crate::ProviderError::Io(std::io::Error::other(e.to_string())))?;
        Ok(())
    }

    pub fn is_managed(instance_dir: &Path) -> bool {
        Self::load(instance_dir).is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtLauncherManagedFile {
    #[serde(rename = "fileId", default)]
    pub file_id: String,
    #[serde(rename = "fileName", default)]
    pub file_name: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub optional: bool,
    #[serde(rename = "clientOnly", default)]
    pub client_only: bool,
    #[serde(default = "default_atl")]
    pub provider: String,
}

fn default_atl() -> String {
    "atlauncher".to_string()
}

/// Persisted metadata for an ATLauncher modpack installation (`.mcw/atlauncher-pack.json`), Schema 1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtLauncherPackRecord {
    #[serde(rename = "schemaVersion", default = "default_atl_schema")]
    pub schema_version: u32,
    #[serde(default = "default_atl")]
    pub source: String,
    #[serde(rename = "packId")]
    pub pack_id: String,
    #[serde(rename = "packName", default)]
    pub pack_name: String,
    #[serde(rename = "safeName", default)]
    pub safe_name: String,
    #[serde(rename = "versionId", default)]
    pub version_id: String,
    #[serde(rename = "versionName", default)]
    pub version_name: String,
    #[serde(rename = "managedFiles", default)]
    pub managed_files: Vec<AtLauncherManagedFile>,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: String,
}

fn default_atl_schema() -> u32 {
    1
}

pub struct AtLauncherPackRegistry;

impl AtLauncherPackRegistry {
    pub fn registry_path(instance_dir: &Path) -> PathBuf {
        instance_dir.join(".mcw").join("atlauncher-pack.json")
    }

    pub fn load(instance_dir: &Path) -> Option<AtLauncherPackRecord> {
        let path = Self::registry_path(instance_dir);
        let raw = fs::read_to_string(path).ok()?;
        serde_json::from_str(&raw).ok()
    }

    pub fn save(instance_dir: &Path, record: &AtLauncherPackRecord) -> crate::Result<()> {
        let mut normalized = record.clone();
        normalized.schema_version = 1;
        normalized.source = "atlauncher".to_string();
        normalized.updated_at = unix_timestamp_string();
        for f in &mut normalized.managed_files {
            f.path = safe_relative_posix(&f.path, &f.file_name);
            f.sha1 = f.sha1.trim().to_ascii_lowercase();
            f.provider = "atlauncher".to_string();
        }
        normalized
            .managed_files
            .sort_by_key(|a| a.path.to_ascii_lowercase());
        atomic_write_json(&Self::registry_path(instance_dir), &normalized)
            .map_err(|e| crate::ProviderError::Io(std::io::Error::other(e.to_string())))?;
        Ok(())
    }

    pub fn is_managed(instance_dir: &Path) -> bool {
        Self::load(instance_dir).is_some()
    }
}

#[derive(Debug, Clone)]
pub struct InstalledPackOrigin {
    pub provider: String,
    pub pack_id: String,
    pub pack_name: String,
    pub version_name: String,
    pub managed_files_count: usize,
}

pub fn detect_installed_pack(instance_dir: &Path) -> Option<InstalledPackOrigin> {
    if let Some(ftb) = FtbPackRegistry::load(instance_dir) {
        return Some(InstalledPackOrigin {
            provider: "FTB".to_string(),
            pack_id: ftb.pack_id.to_string(),
            pack_name: if ftb.pack_name.is_empty() {
                format!("FTB #{}", ftb.pack_id)
            } else {
                ftb.pack_name
            },
            version_name: if ftb.version_name.is_empty() {
                format!("v{}", ftb.version_id)
            } else {
                ftb.version_name
            },
            managed_files_count: ftb.managed_files.len(),
        });
    }
    if let Some(atl) = AtLauncherPackRegistry::load(instance_dir) {
        return Some(InstalledPackOrigin {
            provider: "ATLauncher".to_string(),
            pack_id: atl.pack_id.clone(),
            pack_name: if atl.pack_name.is_empty() {
                atl.safe_name.clone()
            } else {
                atl.pack_name.clone()
            },
            version_name: if atl.version_name.is_empty() {
                atl.version_id
            } else {
                atl.version_name
            },
            managed_files_count: atl.managed_files.len(),
        });
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceExportOrigin {
    Modrinth,
    CurseForge,
    OtherProvider,
    Custom,
}

/// Determines which modpack provider (if any) originally created this instance,
/// so Export Modpack filters allowed formats cleanly.
pub fn detect_export_origin(instance_dir: &Path, icon_source: &str) -> InstanceExportOrigin {
    let mcw_dir = instance_dir.join(".mcw");
    let src = icon_source.trim().to_ascii_lowercase();
    if mcw_dir.join("modrinth-pack.json").is_file() || src == "modpack:modrinth" {
        return InstanceExportOrigin::Modrinth;
    }
    if mcw_dir.join("curseforge-pack.json").is_file()
        || instance_dir.join("pending_manual_downloads.json").is_file()
        || src == "modpack:curseforge"
    {
        return InstanceExportOrigin::CurseForge;
    }
    if FtbPackRegistry::is_managed(instance_dir)
        || AtLauncherPackRegistry::is_managed(instance_dir)
        || matches!(src.as_str(), "modpack:ftb" | "modpack:atlauncher")
    {
        return InstanceExportOrigin::OtherProvider;
    }
    InstanceExportOrigin::Custom
}

pub fn safe_relative_posix(raw: &str, fallback_filename: &str) -> String {
    let fallback = Path::new(fallback_filename)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("download.bin");
    let normalized = raw
        .replace('\\', "/")
        .trim()
        .trim_start_matches('/')
        .to_string();
    if normalized.is_empty() {
        return fallback.to_string();
    }
    let mut parts = Vec::new();
    for c in Path::new(&normalized).components() {
        match c {
            Component::Normal(seg) => {
                let s = seg.to_string_lossy();
                if s.contains(':') {
                    return fallback.to_string();
                }
                parts.push(s.into_owned());
            }
            _ => return fallback.to_string(),
        }
    }
    if parts.is_empty() {
        fallback.to_string()
    } else {
        parts.join("/")
    }
}

fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn saves_and_loads_ftb_and_atlauncher_pack_registries_with_path_sanitization() {
        let dir = tempdir().unwrap();
        let inst_ftb = dir.path().join("ftb_inst");
        fs::create_dir_all(&inst_ftb).unwrap();

        FtbPackRegistry::save(
            &inst_ftb,
            &FtbPackRecord {
                schema_version: 2,
                source: "ftb".to_string(),
                pack_id: 105,
                pack_name: "FTB StoneBlock".to_string(),
                version_id: 999,
                version_name: "1.4.0".to_string(),
                managed_files: vec![FtbManagedFile {
                    file_id: 42,
                    file_name: "mod.jar".to_string(),
                    path: "../escape/mod.jar".to_string(),
                    sha1: "AABBCC".to_string(),
                    size: 1024,
                    urls: vec!["https://example.com/mod.jar".to_string()],
                    optional: false,
                    client_only: true,
                    file_type: "mod".to_string(),
                    provider: "ftb".to_string(),
                }],
                updated_at: String::new(),
            },
        )
        .unwrap();

        assert!(FtbPackRegistry::is_managed(&inst_ftb));
        let loaded_ftb = FtbPackRegistry::load(&inst_ftb).unwrap();
        assert_eq!(loaded_ftb.managed_files[0].path, "mod.jar");
        assert_eq!(loaded_ftb.managed_files[0].sha1, "aabbcc");

        let origin = detect_installed_pack(&inst_ftb).unwrap();
        assert_eq!(origin.provider, "FTB");
        assert_eq!(origin.pack_name, "FTB StoneBlock");

        let inst_atl = dir.path().join("atl_inst");
        fs::create_dir_all(&inst_atl).unwrap();
        AtLauncherPackRegistry::save(
            &inst_atl,
            &AtLauncherPackRecord {
                schema_version: 1,
                source: "atlauncher".to_string(),
                pack_id: "55".to_string(),
                pack_name: "ATDemo".to_string(),
                safe_name: "ATDemo".to_string(),
                version_id: "2.1.0".to_string(),
                version_name: "2.1.0".to_string(),
                managed_files: vec![AtLauncherManagedFile {
                    file_id: "opt1".to_string(),
                    file_name: "optifine-alt.jar".to_string(),
                    name: "OptiAlt".to_string(),
                    path: "mods/optifine-alt.jar".to_string(),
                    sha1: "1234".to_string(),
                    size: 55,
                    urls: vec![],
                    optional: true,
                    client_only: true,
                    provider: "atlauncher".to_string(),
                }],
                updated_at: String::new(),
            },
        )
        .unwrap();
        assert!(AtLauncherPackRegistry::is_managed(&inst_atl));
        let atl_origin = detect_installed_pack(&inst_atl).unwrap();
        assert_eq!(atl_origin.provider, "ATLauncher");
        assert_eq!(atl_origin.version_name, "2.1.0");
    }

    #[test]
    fn detects_instance_export_origin_from_markers_and_icon_source() {
        let dir = tempdir().unwrap();

        let custom = dir.path().join("custom");
        fs::create_dir_all(&custom).unwrap();
        assert_eq!(
            detect_export_origin(&custom, ""),
            InstanceExportOrigin::Custom
        );

        let mr = dir.path().join("mr");
        fs::create_dir_all(mr.join(".mcw")).unwrap();
        fs::write(mr.join(".mcw/modrinth-pack.json"), "{}").unwrap();
        assert_eq!(
            detect_export_origin(&mr, ""),
            InstanceExportOrigin::Modrinth
        );
        assert_eq!(
            detect_export_origin(&custom, "modpack:modrinth"),
            InstanceExportOrigin::Modrinth
        );

        let cf = dir.path().join("cf");
        fs::create_dir_all(cf.join(".mcw")).unwrap();
        fs::write(cf.join(".mcw/curseforge-pack.json"), "{}").unwrap();
        assert_eq!(
            detect_export_origin(&cf, ""),
            InstanceExportOrigin::CurseForge
        );
        assert_eq!(
            detect_export_origin(&custom, "modpack:curseforge"),
            InstanceExportOrigin::CurseForge
        );

        assert_eq!(
            detect_export_origin(&custom, "modpack:ftb"),
            InstanceExportOrigin::OtherProvider
        );
        assert_eq!(
            detect_export_origin(&custom, "modpack:atlauncher"),
            InstanceExportOrigin::OtherProvider
        );
    }
}
