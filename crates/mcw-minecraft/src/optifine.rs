use crate::{MinecraftError, Result};
use mcw_models::instance::Instance;
use mcw_models::minecraft::{
    Arguments, DownloadArtifact, LibraryDownloads, LibraryEntry, VersionMetadata,
};
use mcw_storage::LauncherPaths;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;
use zip::ZipArchive;

const MAX_OPTIFINE_SIZE: u64 = 128 * 1024 * 1024;
const REQUIRED_CLASSES: &[&str] = &[
    "optifine/Installer.class",
    "optifine/Config.class",
    "net/optifine/Config.class",
];
pub const OFFICIAL_OPTIFINE_URL: &str = "https://optifine.net/downloads";

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

    pub fn version_id(&self) -> String {
        format!("{}_{}", self.minecraft_version, self.edition)
    }
}

#[derive(Debug, Clone)]
pub struct InspectedOptiFine {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub sha256: String,
    pub sha1: String,
    pub version: OptiFineVersion,
}

/// Persisted per-instance OptiFine tracking state (`.mcw/optifine.json`),
/// mirroring Python `OptiFineRegistry` & `OptiFineState`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptiFineState {
    #[serde(rename = "schemaVersion", default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub installed: bool,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(rename = "minecraftVersion", default)]
    pub minecraft_version: String,
    #[serde(rename = "versionId", default)]
    pub version_id: String,
    #[serde(rename = "fileName", default)]
    pub file_name: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub managed: bool,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
    #[serde(rename = "sourcePath", default)]
    pub source_path: String,
    #[serde(rename = "installedPath", default)]
    pub installed_path: String,
    #[serde(rename = "profilePath", default)]
    pub profile_path: String,
    #[serde(default)]
    pub preview: bool,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: String,
}

fn default_schema_version() -> u32 {
    1
}

fn default_status() -> String {
    "not_installed".to_string()
}

impl Default for OptiFineState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            installed: false,
            status: default_status(),
            minecraft_version: String::new(),
            version_id: String::new(),
            file_name: String::new(),
            mode: String::new(),
            managed: false,
            sha256: String::new(),
            sha1: String::new(),
            size: 0,
            source_path: String::new(),
            installed_path: String::new(),
            profile_path: String::new(),
            preview: false,
            updated_at: String::new(),
        }
    }
}

/// Port of Python `OptiFineRegistry` (`src/core/optifine/optifine_registry.py`).
pub struct OptiFineRegistry;

impl OptiFineRegistry {
    pub fn registry_path(instance_dir: &Path) -> PathBuf {
        instance_dir.join(".mcw").join("optifine.json")
    }

    pub fn profile_path(instance_dir: &Path) -> PathBuf {
        instance_dir.join(".mcw").join("optifine-profile.json")
    }

    pub fn load(instance_dir: &Path) -> OptiFineState {
        let path = Self::registry_path(instance_dir);
        let Ok(raw) = fs::read_to_string(&path) else {
            return OptiFineState::default();
        };
        let mut state: OptiFineState = serde_json::from_str(&raw).unwrap_or_default();
        state.mode = state.mode.trim().to_ascii_lowercase();
        if state.mode != "standalone" && state.mode != "forge_mod" {
            state.mode.clear();
        }
        state.installed = state.installed && !state.mode.is_empty();
        if !state.installed && state.status.is_empty() {
            state.status = "not_installed".to_string();
        }
        state
    }

    pub fn save(instance_dir: &Path, state: &OptiFineState) -> Result<()> {
        let path = Self::registry_path(instance_dir);
        let mut normalized = state.clone();
        normalized.schema_version = 1;
        normalized.updated_at = unix_timestamp_string();
        mcw_storage::atomic_write_json(&path, &normalized)?;
        Ok(())
    }

    pub fn clear(instance_dir: &Path) {
        let _ = fs::remove_file(Self::registry_path(instance_dir));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransactionBackedFile {
    path: String,
    existed: bool,
    backup: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TransactionJournal {
    state: String,
    files: Vec<TransactionBackedFile>,
    outputs: Vec<String>,
}

/// Atomic rollback transaction journal for OptiFine profile/mod modifications
/// (port of Python `OptiFineTransaction`).
pub struct OptiFineTransaction {
    instance_dir: PathBuf,
    journal_path: PathBuf,
    backup_dir: PathBuf,
    journal: TransactionJournal,
}

impl OptiFineTransaction {
    fn journal_file(instance_dir: &Path) -> PathBuf {
        instance_dir.join(".mcw").join("optifine-transaction.json")
    }

    fn backup_folder(instance_dir: &Path) -> PathBuf {
        instance_dir
            .join(".mcw")
            .join("optifine-transaction-backup")
    }

    pub fn begin(instance_dir: &Path) -> Result<Self> {
        let _ = Self::recover(instance_dir);
        let journal_path = Self::journal_file(instance_dir);
        let backup_dir = Self::backup_folder(instance_dir);
        if backup_dir.exists() {
            let _ = fs::remove_dir_all(&backup_dir);
        }
        fs::create_dir_all(&backup_dir)?;

        let mut tx = Self {
            instance_dir: instance_dir.to_path_buf(),
            journal_path,
            backup_dir,
            journal: TransactionJournal {
                state: "prepared".to_string(),
                files: Vec::new(),
                outputs: Vec::new(),
            },
        };

        let mut to_backup = vec![OptiFineRegistry::registry_path(instance_dir)];
        let prev = OptiFineRegistry::load(instance_dir);
        if prev.installed && prev.managed {
            if prev.mode == "standalone" {
                to_backup.push(OptiFineRegistry::profile_path(instance_dir));
            } else if prev.mode == "forge_mod" && !prev.installed_path.is_empty() {
                to_backup.push(PathBuf::from(&prev.installed_path));
            }
        }
        let mut seen = HashSet::new();
        for p in to_backup {
            if let Ok(resolved) = tx.require_allowed(&p) {
                if seen.insert(resolved.clone()) {
                    tx.backup_file(&resolved)?;
                }
            }
        }
        tx.write_journal()?;
        Ok(tx)
    }

    pub fn recover(instance_dir: &Path) -> bool {
        let journal_path = Self::journal_file(instance_dir);
        let backup_dir = Self::backup_folder(instance_dir);
        if !journal_path.is_file() {
            if backup_dir.exists() {
                let _ = fs::remove_dir_all(&backup_dir);
            }
            return false;
        }
        let Ok(raw) = fs::read_to_string(&journal_path) else {
            let _ = fs::remove_file(&journal_path);
            let _ = fs::remove_dir_all(&backup_dir);
            return false;
        };
        let Ok(journal) = serde_json::from_str::<TransactionJournal>(&raw) else {
            let _ = fs::remove_file(&journal_path);
            let _ = fs::remove_dir_all(&backup_dir);
            return false;
        };
        let mut tx = Self {
            instance_dir: instance_dir.to_path_buf(),
            journal_path,
            backup_dir,
            journal,
        };
        tx.rollback();
        true
    }

    pub fn register_output(&mut self, path: &Path) -> Result<()> {
        let norm = self.require_allowed(path)?;
        let s = norm.to_string_lossy().to_string();
        if !self.journal.outputs.contains(&s) {
            self.journal.outputs.push(s);
            self.write_journal()?;
        }
        Ok(())
    }

    pub fn commit(&mut self) -> Result<()> {
        self.journal.state = "committed".to_string();
        self.write_journal()?;
        let _ = fs::remove_file(&self.journal_path);
        let _ = fs::remove_dir_all(&self.backup_dir);
        Ok(())
    }

    pub fn rollback(&mut self) {
        let original_paths: HashSet<String> =
            self.journal.files.iter().map(|f| f.path.clone()).collect();
        for out_raw in self.journal.outputs.iter().rev() {
            if let Ok(out) = self.require_allowed(Path::new(out_raw)) {
                let key = out.to_string_lossy().to_string();
                if !original_paths.contains(&key) {
                    let _ = fs::remove_file(&out);
                }
            }
        }
        for backed in self.journal.files.iter().rev() {
            let Ok(target) = self.require_allowed(Path::new(&backed.path)) else {
                continue;
            };
            if backed.existed && !backed.backup.is_empty() {
                let bak = self.backup_dir.join(&backed.backup);
                if bak.is_file() {
                    if let Some(parent) = target.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    let _ = fs::copy(&bak, &target);
                }
            } else if !backed.existed {
                let _ = fs::remove_file(&target);
            }
        }
        let _ = fs::remove_file(&self.journal_path);
        let _ = fs::remove_dir_all(&self.backup_dir);
    }

    fn backup_file(&mut self, target: &Path) -> Result<()> {
        let index = self.journal.files.len();
        let existed = target.is_file();
        let backup = if existed {
            let name = format!("{index:03}.bak");
            fs::copy(target, self.backup_dir.join(&name))?;
            name
        } else {
            String::new()
        };
        self.journal.files.push(TransactionBackedFile {
            path: target.to_string_lossy().to_string(),
            existed,
            backup,
        });
        Ok(())
    }

    fn require_allowed(&self, path: &Path) -> Result<PathBuf> {
        let root =
            fs::canonicalize(&self.instance_dir).unwrap_or_else(|_| self.instance_dir.clone());
        let resolved = if path.is_absolute() {
            if let Some(parent) = path.parent() {
                if let Ok(canon_parent) = fs::canonicalize(parent) {
                    canon_parent.join(path.file_name().unwrap_or_default())
                } else {
                    path.to_path_buf()
                }
            } else {
                path.to_path_buf()
            }
        } else {
            root.join(path)
        };
        if !resolved.starts_with(&root) {
            return Err(MinecraftError::InvalidMetadata(format!(
                "OptiFine transaction path escapes instance directory: {}",
                resolved.display()
            )));
        }
        Ok(resolved)
    }

    fn write_journal(&self) -> Result<()> {
        mcw_storage::atomic_write_json(&self.journal_path, &self.journal)?;
        Ok(())
    }
}

pub struct OptiFineManager;

impl OptiFineManager {
    /// Returns the tracked OptiFine state after recovering any interrupted transaction.
    pub fn state(instance_dir: &Path) -> OptiFineState {
        OptiFineTransaction::recover(instance_dir);
        OptiFineRegistry::load(instance_dir)
    }

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

        let raw_bytes = fs::read(jar_path)?;
        let sha256 = sha256_hex(&raw_bytes);
        let sha1 = sha1_hex(&raw_bytes);

        Ok(InspectedOptiFine {
            path: jar_path.to_path_buf(),
            size_bytes: meta.len(),
            sha256,
            sha1,
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

    /// Installs a validated OptiFine JAR into `<mc_dir>/mods/` (simple unmanaged helper).
    pub fn install_into_mods(
        jar_path: &Path,
        mc_dir: &Path,
        expected_mc_version: Option<&str>,
    ) -> Result<PathBuf> {
        let inspected = Self::inspect(jar_path, expected_mc_version)?;
        let mods_dir = mc_dir.join("mods");
        fs::create_dir_all(&mods_dir)?;

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

    /// Transactional, registry-tracked Forge OptiFine mod installation.
    pub fn install_managed_forge_mod(
        paths: &LauncherPaths,
        instance: &Instance,
        jar_path: &Path,
    ) -> Result<OptiFineState> {
        let loader = instance.mod_loader.name.trim().to_ascii_lowercase();
        if loader != "forge" && loader != "neoforge" {
            return Err(MinecraftError::InvalidMetadata(
                "OptiFine mod mode requires a Forge or NeoForge instance.".to_string(),
            ));
        }
        let inspected = Self::inspect(jar_path, Some(&instance.version_id))?;
        let cached_source = Self::cache_source_jar(paths, &inspected)?;
        let inst_dir = paths.instance_dir(&instance.instance_id);
        let mc_dir = paths.minecraft_dir(&instance.instance_id);
        let mods_dir = mc_dir.join("mods");
        fs::create_dir_all(&mods_dir)?;

        let prev = Self::state(&inst_dir);
        let mut tx = OptiFineTransaction::begin(&inst_dir)?;
        let res = (|| -> Result<OptiFineState> {
            let dest = mods_dir.join(&inspected.version.filename);
            tx.register_output(&dest)?;

            if prev.installed
                && prev.managed
                && prev.mode == "forge_mod"
                && !prev.installed_path.is_empty()
            {
                let old_path = PathBuf::from(&prev.installed_path);
                if old_path != dest && old_path.is_file() {
                    let _ = fs::remove_file(&old_path);
                }
            }
            if let Ok(entries) = fs::read_dir(&mods_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                    if name.starts_with("optifine_") || name.starts_with("preview_optifine_") {
                        let p = entry.path();
                        if p != dest {
                            let _ = fs::remove_file(&p);
                        }
                    }
                }
            }

            let tmp = dest.with_extension("jar.part");
            fs::copy(&cached_source, &tmp)?;
            if dest.exists() {
                let _ = fs::remove_file(&dest);
            }
            fs::rename(&tmp, &dest)?;

            let state = OptiFineState {
                schema_version: 1,
                installed: true,
                status: "installed".to_string(),
                minecraft_version: inspected.version.minecraft_version.clone(),
                version_id: inspected.version.version_id(),
                file_name: inspected.version.filename.clone(),
                mode: "forge_mod".to_string(),
                managed: true,
                sha256: inspected.sha256.clone(),
                sha1: inspected.sha1.clone(),
                size: inspected.size_bytes,
                source_path: cached_source.to_string_lossy().to_string(),
                installed_path: dest.to_string_lossy().to_string(),
                profile_path: String::new(),
                preview: inspected.version.is_preview,
                updated_at: String::new(),
            };
            OptiFineRegistry::save(&inst_dir, &state)?;
            tx.commit()?;
            Ok(state)
        })();

        if res.is_err() {
            tx.rollback();
        }
        res
    }

    /// Transactional, isolated standalone OptiFine profile installation for Vanilla instances.
    pub fn install_standalone_profile(
        paths: &LauncherPaths,
        instance: &Instance,
        base_metadata: &VersionMetadata,
        jar_path: &Path,
        java_exe: &Path,
    ) -> Result<OptiFineState> {
        if !instance.mod_loader.is_vanilla() {
            return Err(MinecraftError::InvalidMetadata(
                "Standalone OptiFine profile can only be installed on a Vanilla instance."
                    .to_string(),
            ));
        }
        let inspected = Self::inspect(jar_path, Some(&instance.version_id))?;
        let cached_source = Self::cache_source_jar(paths, &inspected)?;
        let inst_dir = paths.instance_dir(&instance.instance_id);
        let base_client_jar = paths.client_jar(&base_metadata.id);
        if !base_client_jar.is_file() {
            return Err(MinecraftError::InvalidMetadata(format!(
                "Base Minecraft {} client JAR is missing ({}). Launch once or Repair Files first.",
                base_metadata.id,
                base_client_jar.display()
            )));
        }

        let staging = inst_dir.join(".mcw").join("optifine-staging");
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        let staging_mc = staging.join(".minecraft");
        Self::prepare_staging(base_metadata, &base_client_jar, &staging_mc)?;

        let run_res = Self::run_isolated_installer(&cached_source, java_exe, &staging);
        if let Err(err) = run_res {
            let _ = fs::remove_dir_all(&staging);
            return Err(err);
        }

        let mut tx = OptiFineTransaction::begin(&inst_dir)?;
        let res = (|| -> Result<OptiFineState> {
            let generated = Self::find_generated_profile(&staging_mc, &base_metadata.id)?;
            Self::import_staging_libraries(paths, &staging_mc)?;
            let normalized = Self::normalize_local_libraries(&generated, &staging_mc, paths)?;
            let version_id = inspected.version.version_id();
            let merged =
                merge_optifine_profile(base_metadata, &normalized, &base_metadata.id, &version_id);

            let profile_path = OptiFineRegistry::profile_path(&inst_dir);
            tx.register_output(&profile_path)?;
            mcw_storage::atomic_write_json(&profile_path, &merged)?;

            let raw = fs::read_to_string(&profile_path)?;
            let _: VersionMetadata = serde_json::from_str(&raw)?;

            let state = OptiFineState {
                schema_version: 1,
                installed: true,
                status: "installed".to_string(),
                minecraft_version: inspected.version.minecraft_version.clone(),
                version_id,
                file_name: inspected.version.filename.clone(),
                mode: "standalone".to_string(),
                managed: true,
                sha256: inspected.sha256.clone(),
                sha1: inspected.sha1.clone(),
                size: inspected.size_bytes,
                source_path: cached_source.to_string_lossy().to_string(),
                installed_path: profile_path.to_string_lossy().to_string(),
                profile_path: profile_path.to_string_lossy().to_string(),
                preview: inspected.version.is_preview,
                updated_at: String::new(),
            };
            OptiFineRegistry::save(&inst_dir, &state)?;
            tx.commit()?;
            let _ = fs::remove_dir_all(&staging);
            Ok(state)
        })();

        if res.is_err() {
            tx.rollback();
            let _ = fs::remove_dir_all(&staging);
        }
        res
    }

    /// Removes MCW-managed OptiFine (standalone profile or managed Forge mod) transactionally.
    pub fn uninstall(paths: &LauncherPaths, instance: &Instance) -> Result<bool> {
        let inst_dir = paths.instance_dir(&instance.instance_id);
        let state = Self::state(&inst_dir);
        if !state.installed {
            return Ok(false);
        }
        if !state.managed {
            return Err(MinecraftError::InvalidMetadata(
                "Refusing to remove an unmanaged OptiFine file.".to_string(),
            ));
        }

        let mut tx = OptiFineTransaction::begin(&inst_dir)?;
        let res = (|| -> Result<bool> {
            if state.mode == "standalone" {
                let p = OptiFineRegistry::profile_path(&inst_dir);
                if p.is_file() {
                    fs::remove_file(&p)?;
                }
            } else if state.mode == "forge_mod" && !state.installed_path.is_empty() {
                let p = PathBuf::from(&state.installed_path);
                if p.is_file() {
                    fs::remove_file(&p)?;
                }
            }
            OptiFineRegistry::clear(&inst_dir);
            tx.commit()?;
            Ok(true)
        })();
        if res.is_err() {
            tx.rollback();
        }
        res
    }

    /// Applies the tracked standalone OptiFine profile on launch (or verifies managed Forge mod).
    pub fn apply_to_version(
        paths: &LauncherPaths,
        instance: &Instance,
        base_metadata: VersionMetadata,
    ) -> Result<VersionMetadata> {
        let inst_dir = paths.instance_dir(&instance.instance_id);
        let state = Self::state(&inst_dir);
        if !state.installed {
            return Ok(base_metadata);
        }
        if !state.minecraft_version.is_empty() && state.minecraft_version != instance.version_id {
            return Err(MinecraftError::InvalidMetadata(format!(
                "Installed OptiFine targets Minecraft {}, not {}. Uninstall or reinstall OptiFine.",
                state.minecraft_version, instance.version_id
            )));
        }
        if state.mode == "standalone" {
            let profile_path = OptiFineRegistry::profile_path(&inst_dir);
            let raw = fs::read_to_string(&profile_path).map_err(|_| {
                MinecraftError::InvalidMetadata(
                    "Managed OptiFine standalone profile is missing. Reinstall OptiFine before launching."
                        .to_string(),
                )
            })?;
            let meta: VersionMetadata = serde_json::from_str(&raw)?;
            return Ok(meta);
        }
        if state.mode == "forge_mod" && !state.installed_path.is_empty() {
            let p = PathBuf::from(&state.installed_path);
            if !p.is_file() {
                return Err(MinecraftError::InvalidMetadata(
                    "Managed OptiFine mod file is missing from mods/. Uninstall or reinstall OptiFine."
                        .to_string(),
                ));
            }
            if !state.sha256.is_empty() {
                let actual = sha256_hex(&fs::read(&p)?);
                if actual != state.sha256 {
                    return Err(MinecraftError::InvalidMetadata(
                        "Managed OptiFine mod failed SHA-256 verification.".to_string(),
                    ));
                }
            }
        }
        Ok(base_metadata)
    }

    /// Launches the official OptiFine GUI installer using the resolved Java runtime.
    pub fn launch_official_installer(
        jar_path: &Path,
        java_exe: &Path,
        expected_mc_version: Option<&str>,
    ) -> Result<()> {
        let inspected = Self::inspect(jar_path, expected_mc_version)?;
        let resolved_java = mcw_runtime::prefer_javaw_on_windows(java_exe);
        let mut cmd = Command::new(&resolved_java);
        cmd.arg("-jar").arg(&inspected.path);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd.spawn()?;
        Ok(())
    }

    fn cache_source_jar(paths: &LauncherPaths, inspected: &InspectedOptiFine) -> Result<PathBuf> {
        let dest = paths
            .cache_root
            .join("optifine")
            .join(&inspected.sha256)
            .join(&inspected.version.filename);
        if dest.is_file()
            && fs::metadata(&dest)
                .map(|m| m.len() == inspected.size_bytes)
                .unwrap_or(false)
        {
            return Ok(dest);
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = dest.with_extension("jar.part");
        fs::copy(&inspected.path, &tmp)?;
        if dest.exists() {
            let _ = fs::remove_file(&dest);
        }
        fs::rename(&tmp, &dest)?;
        Ok(dest)
    }

    fn prepare_staging(
        base: &VersionMetadata,
        base_client_jar: &Path,
        staging_mc: &Path,
    ) -> Result<()> {
        fs::create_dir_all(staging_mc)?;
        let profiles_val = serde_json::json!({
            "profiles": {},
            "selectedProfile": null,
            "clientToken": "mcw-launcher"
        });
        mcw_storage::atomic_write_json(&staging_mc.join("launcher_profiles.json"), &profiles_val)?;

        let version_dir = staging_mc.join("versions").join(&base.id);
        fs::create_dir_all(&version_dir)?;
        mcw_storage::atomic_write_json(&version_dir.join(format!("{}.json", base.id)), base)?;
        fs::copy(
            base_client_jar,
            version_dir.join(format!("{}.jar", base.id)),
        )?;
        Ok(())
    }

    fn run_isolated_installer(
        source_jar: &Path,
        java_exe: &Path,
        staging_home: &Path,
    ) -> Result<()> {
        let resolved_java = mcw_runtime::prefer_javaw_on_windows(java_exe);
        let mut cmd = Command::new(&resolved_java);
        if let Some(parent) = source_jar.parent() {
            cmd.current_dir(parent);
        }
        cmd.env("APPDATA", staging_home)
            .env("HOME", staging_home)
            .env("USERPROFILE", staging_home)
            .arg(format!("-Duser.home={}", staging_home.display()))
            .arg("-jar")
            .arg(source_jar);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let status = cmd.status()?;
        if !status.success() {
            return Err(MinecraftError::InvalidMetadata(format!(
                "The official OptiFine installer exited with status {:?}.",
                status.code()
            )));
        }
        Ok(())
    }

    fn find_generated_profile(staging_mc: &Path, base_id: &str) -> Result<VersionMetadata> {
        let versions_dir = staging_mc.join("versions");
        let mut candidates: Vec<(SystemTime, VersionMetadata)> = Vec::new();
        if let Ok(entries) = fs::read_dir(&versions_dir) {
            for e in entries.flatten() {
                let folder = e.path();
                if !folder.is_dir() || e.file_name().to_string_lossy() == base_id {
                    continue;
                }
                if let Ok(files) = fs::read_dir(&folder) {
                    for f in files.flatten() {
                        let fp = f.path();
                        if fp
                            .extension()
                            .and_then(|ex| ex.to_str())
                            .is_some_and(|ex| ex.eq_ignore_ascii_case("json"))
                        {
                            if let Ok(raw) = fs::read_to_string(&fp) {
                                if let Ok(meta) = serde_json::from_str::<VersionMetadata>(&raw) {
                                    let identity =
                                        format!("{} {}", e.file_name().to_string_lossy(), meta.id)
                                            .to_ascii_lowercase();
                                    if identity.contains("optifine") {
                                        let mtime = fs::metadata(&fp)
                                            .and_then(|m| m.modified())
                                            .unwrap_or(UNIX_EPOCH);
                                        candidates.push((mtime, meta));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        candidates.sort_by_key(|c| c.0);
        candidates.pop().map(|c| c.1).ok_or_else(|| {
            MinecraftError::InvalidMetadata(
                "OptiFine installer closed without generating an OptiFine profile. Click 'Install' in the OptiFine window first."
                    .to_string(),
            )
        })
    }

    fn import_staging_libraries(paths: &LauncherPaths, staging_mc: &Path) -> Result<()> {
        let src_root = staging_mc.join("libraries");
        if !src_root.is_dir() {
            return Err(MinecraftError::InvalidMetadata(
                "OptiFine installer did not generate runtime libraries.".to_string(),
            ));
        }
        let dst_root = paths.libraries_dir();
        for entry in WalkDir::new(&src_root).into_iter().flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = entry.path().strip_prefix(&src_root).unwrap_or(entry.path());
            let mut clean = PathBuf::new();
            for c in rel.components() {
                if let Component::Normal(p) = c {
                    clean.push(p);
                }
            }
            let target = dst_root.join(clean);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let tmp = target.with_extension("jar.part");
            fs::copy(entry.path(), &tmp)?;
            if target.exists() {
                let _ = fs::remove_file(&target);
            }
            fs::rename(&tmp, &target)?;
        }
        Ok(())
    }

    fn normalize_local_libraries(
        profile: &VersionMetadata,
        staging_mc: &Path,
        paths: &LauncherPaths,
    ) -> Result<VersionMetadata> {
        let mut normalized = profile.clone();
        let libs_dir = paths.libraries_dir();
        for lib in &mut normalized.libraries {
            let has_artifact = lib
                .downloads
                .as_ref()
                .and_then(|d| d.artifact.as_ref())
                .is_some();
            if has_artifact {
                continue;
            }
            if let Some(rel) = maven_coordinate_to_path(&lib.name) {
                let target = libs_dir.join(&rel);
                let staging_cand = staging_mc.join("libraries").join(&rel);
                let cand = if target.is_file() {
                    Some(target)
                } else if staging_cand.is_file() {
                    Some(staging_cand)
                } else {
                    None
                };
                if let Some(found) = cand {
                    if let Ok(bytes) = fs::read(&found) {
                        let digest = sha1_hex(&bytes);
                        let rel_posix = rel.to_string_lossy().replace('\\', "/");
                        lib.downloads = Some(LibraryDownloads {
                            artifact: Some(DownloadArtifact {
                                path: Some(rel_posix),
                                sha1: Some(digest),
                                size: bytes.len() as u64,
                                url: "https://optifine.net/".to_string(),
                            }),
                            classifiers: HashMap::new(),
                        });
                    }
                }
            }
        }
        Ok(normalized)
    }
}

/// Merges a base Mojang `VersionMetadata` with a generated OptiFine profile
/// (`optifine_profile_installer.py::_merge_profiles`).
pub fn merge_optifine_profile(
    base: &VersionMetadata,
    optifine: &VersionMetadata,
    game_version: &str,
    version_id: &str,
) -> VersionMetadata {
    let mut merged = base.clone();
    merged.id = format!("optifine-{version_id}");
    merged.inherits_from = Some(game_version.to_string());
    if !optifine.main_class.trim().is_empty() {
        merged.main_class = optifine.main_class.clone();
    }

    let mut libs: Vec<LibraryEntry> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();
    for item in base.libraries.iter().chain(optifine.libraries.iter()) {
        let key = item.name.clone();
        if let Some(&idx) = positions.get(&key) {
            libs[idx] = item.clone();
        } else {
            positions.insert(key, libs.len());
            libs.push(item.clone());
        }
    }
    merged.libraries = libs;

    if let Some(ref of_args) = optifine.arguments {
        let mut args = merged.arguments.take().unwrap_or(Arguments {
            game: Vec::new(),
            jvm: Vec::new(),
        });
        args.game.extend(of_args.game.clone());
        args.jvm.extend(of_args.jvm.clone());
        merged.arguments = Some(args);
    }

    if let Some(ref of_mc_args) = optifine.minecraft_arguments {
        let trimmed = of_mc_args.trim();
        if !trimmed.is_empty() {
            let combined = match merged.minecraft_arguments.take() {
                Some(existing) if !existing.trim().is_empty() => {
                    format!("{} {trimmed}", existing.trim())
                }
                _ => trimmed.to_string(),
            };
            merged.minecraft_arguments = Some(combined);
        }
    }

    if optifine.java_version.is_some() {
        merged.java_version = optifine.java_version.clone();
    }
    merged
}

fn maven_coordinate_to_path(coord: &str) -> Option<PathBuf> {
    let parts: Vec<&str> = coord.trim().split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group = parts[0];
    let artifact = parts[1];
    let version = parts[2];
    let classifier = parts.get(3).copied().unwrap_or("");
    let filename = if classifier.is_empty() {
        format!("{artifact}-{version}.jar")
    } else {
        format!("{artifact}-{version}-{classifier}.jar")
    };
    let mut p = PathBuf::new();
    for seg in group.split('.') {
        if !seg.is_empty() && seg != ".." {
            p.push(seg);
        }
    }
    p.push(artifact);
    p.push(version);
    p.push(filename);
    Some(p)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn sha1_hex(bytes: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
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
    use mcw_models::instance::ModLoader;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    fn write_mock_optifine_jar(path: &Path) {
        let f = File::create(path).unwrap();
        let mut zw = zip::ZipWriter::new(f);
        zw.start_file("META-INF/MANIFEST.MF", SimpleFileOptions::default())
            .unwrap();
        zw.write_all(b"Manifest-Version: 1.0\n").unwrap();
        zw.start_file("optifine/Config.class", SimpleFileOptions::default())
            .unwrap();
        zw.write_all(b"\xCA\xFE\xBA\xBE").unwrap();
        zw.finish().unwrap();
    }

    #[test]
    fn parses_stable_and_preview_optifine_filenames() {
        let stable = OptiFineVersion::parse_filename("OptiFine_1.20.4_HD_U_I7.jar").unwrap();
        assert_eq!(stable.minecraft_version, "1.20.4");
        assert_eq!(stable.edition, "HD_U_I7");
        assert_eq!(stable.version_id(), "1.20.4_HD_U_I7");
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
        write_mock_optifine_jar(&jar_path);

        assert!(OptiFineManager::inspect(&jar_path, Some("1.21.4")).is_err());

        let mc_dir = tmp.path().join(".minecraft");
        let dest = OptiFineManager::install_into_mods(&jar_path, &mc_dir, Some("1.20.4")).unwrap();
        assert!(dest.is_file());
        assert_eq!(
            OptiFineManager::detect_installed_in_mods(&mc_dir).as_deref(),
            Some("OptiFine_1.20.4_HD_U_I7.jar")
        );
    }

    #[test]
    fn managed_forge_optifine_install_registry_and_uninstall_lifecycle() {
        let tmp = tempdir().unwrap();
        let paths = LauncherPaths::portable(tmp.path());
        paths.initialize().unwrap();
        let store = mcw_storage::InstanceStore::new(&paths);
        let (inst, _) = store
            .create_instance(
                "forge-optifine",
                "1.20.4",
                ModLoader {
                    name: "forge".to_string(),
                    version: "49.0.3".to_string(),
                },
            )
            .unwrap();

        let jar_path = tmp.path().join("OptiFine_1.20.4_HD_U_I7.jar");
        write_mock_optifine_jar(&jar_path);

        let state = OptiFineManager::install_managed_forge_mod(&paths, &inst, &jar_path).unwrap();
        assert!(state.installed);
        assert!(state.managed);
        assert_eq!(state.mode, "forge_mod");
        assert!(Path::new(&state.installed_path).is_file());

        let inst_dir = paths.instance_dir(&inst.instance_id);
        let loaded = OptiFineManager::state(&inst_dir);
        assert_eq!(loaded.version_id, "1.20.4_HD_U_I7");

        assert!(OptiFineManager::uninstall(&paths, &inst).unwrap());
        assert!(!OptiFineManager::state(&inst_dir).installed);
        assert!(!Path::new(&state.installed_path).exists());
    }

    #[test]
    fn transaction_rollback_restores_previous_file_and_cleans_outputs() {
        let tmp = tempdir().unwrap();
        let inst_dir = tmp.path().join("demo");
        fs::create_dir_all(inst_dir.join(".mcw")).unwrap();

        let existing_profile = OptiFineRegistry::profile_path(&inst_dir);
        fs::write(&existing_profile, r#"{"id":"old"}"#).unwrap();
        OptiFineRegistry::save(
            &inst_dir,
            &OptiFineState {
                installed: true,
                managed: true,
                mode: "standalone".to_string(),
                version_id: "1.20.4_OLD".to_string(),
                ..OptiFineState::default()
            },
        )
        .unwrap();

        {
            let mut tx = OptiFineTransaction::begin(&inst_dir).unwrap();
            let extra_out = inst_dir.join(".mcw").join("new-temp-output.json");
            tx.register_output(&extra_out).unwrap();
            fs::write(&extra_out, b"new-out").unwrap();
            fs::write(&existing_profile, r#"{"id":"corrupted"}"#).unwrap();
            tx.rollback();
            assert!(!extra_out.exists());
        }
        assert_eq!(
            fs::read_to_string(&existing_profile).unwrap(),
            r#"{"id":"old"}"#
        );
        assert_eq!(OptiFineRegistry::load(&inst_dir).version_id, "1.20.4_OLD");
    }

    #[test]
    fn merges_standalone_optifine_profile_with_base_metadata() {
        let base = VersionMetadata {
            id: "1.20.4".to_string(),
            version_type: "release".to_string(),
            main_class: "net.minecraft.client.main.Main".to_string(),
            inherits_from: None,
            asset_index: None,
            assets: None,
            downloads: None,
            libraries: vec![LibraryEntry {
                name: "com.mojang:brigadier:1.2.9".to_string(),
                url: None,
                downloads: None,
                rules: Vec::new(),
                natives: None,
                clientreq: None,
            }],
            java_version: None,
            arguments: None,
            minecraft_arguments: None,
        };
        let of_prof = VersionMetadata {
            id: "1.20.4-OptiFine_HD_U_I7".to_string(),
            version_type: "release".to_string(),
            main_class: "net.minecraft.launchwrapper.Launch".to_string(),
            inherits_from: Some("1.20.4".to_string()),
            asset_index: None,
            assets: None,
            downloads: None,
            libraries: vec![LibraryEntry {
                name: "optifine:OptiFine:1.20.4_HD_U_I7".to_string(),
                url: None,
                downloads: None,
                rules: Vec::new(),
                natives: None,
                clientreq: None,
            }],
            java_version: None,
            arguments: None,
            minecraft_arguments: Some("--tweakClass optifine.OptiFineTweaker".to_string()),
        };

        let merged = merge_optifine_profile(&base, &of_prof, "1.20.4", "1.20.4_HD_U_I7");
        assert_eq!(merged.id, "optifine-1.20.4_HD_U_I7");
        assert_eq!(merged.main_class, "net.minecraft.launchwrapper.Launch");
        assert_eq!(merged.libraries.len(), 2);
        assert_eq!(
            merged.minecraft_arguments.as_deref(),
            Some("--tweakClass optifine.OptiFineTweaker")
        );
    }
}
