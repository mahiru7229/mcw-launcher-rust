use crate::{atomic_write_json, InstanceStore, LauncherPaths, Result, StorageError};
use mcw_models::instance::{Instance, InstanceSettings, ModLoader};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MigrationReport {
    pub source_root: PathBuf,
    pub migrated_instances: Vec<String>,
    pub skipped_existing: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PythonLayoutKind {
    /// Modern MCW Launcher Python (2.x / v0.5+): game folders (`mods/`, `saves/`, etc.) directly inside `instances/<name>/`.
    ModernFlat,
    /// Legacy MCW Launcher Python (1.8.x): game folders inside `instances/<name>/.minecraft/`.
    LegacyMinecraftSubdir,
}

impl PythonLayoutKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ModernFlat => "Modern Python (flat instance dir)",
            Self::LegacyMinecraftSubdir => "Legacy Python 1.8.x (.minecraft subdir)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ConflictStrategy {
    #[default]
    Skip,
    RenameImported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPythonInstance {
    pub folder_name: String,
    pub instance_id: String,
    pub name: String,
    pub version_id: String,
    pub loader_name: String,
    pub loader_version: String,
    pub layout: PythonLayoutKind,
    pub estimated_bytes: u64,
    pub has_mcw_meta: bool,
    pub already_exists_in_dest: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PythonMigrationInspection {
    pub source_root: PathBuf,
    pub instances: Vec<DiscoveredPythonInstance>,
    pub has_launcher_settings: bool,
    pub has_accounts_db: bool,
    pub managed_runtimes: Vec<String>,
    pub custom_themes: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PythonMigrationOptions {
    pub selected_folders: Vec<String>,
    pub conflict_strategy: ConflictStrategy,
    pub import_global_settings: bool,
    pub import_runtimes: bool,
    pub import_themes: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportedGlobalSettings {
    pub language: Option<String>,
    pub show_snapshots: Option<bool>,
    pub remember_window_size: Option<bool>,
    pub show_content_descriptions: Option<bool>,
    pub debug_mode: Option<bool>,
    pub prefer_dedicated_gpu: Option<bool>,
    pub discord_rpc_enabled: Option<bool>,
    pub discord_client_id: Option<String>,
    pub minimize_on_launch: Option<bool>,
    pub theme: Option<String>,
    pub motion_mode: Option<String>,
    pub custom_accent_hex: Option<String>,
    pub modrinth_include_beta: Option<bool>,
    pub modrinth_include_alpha: Option<bool>,
    pub modrinth_failure_policy: Option<String>,
    pub curseforge_failure_policy: Option<String>,
    pub download_limit_mbps: Option<f64>,
    pub download_concurrency: Option<u32>,
    pub unused_version_retention_days: Option<u32>,
    pub default_min_memory: Option<u32>,
    pub default_max_memory: Option<u32>,
    pub default_width: Option<u32>,
    pub default_height: Option<u32>,
    pub default_fullscreen: Option<bool>,
    pub auto_check_updates: Option<bool>,
    pub update_channel: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PythonMigrationResult {
    pub imported_instances: Vec<String>,
    pub skipped_instances: Vec<String>,
    pub imported_runtimes: Vec<String>,
    pub imported_themes: Vec<String>,
    pub imported_global_settings: Option<ImportedGlobalSettings>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// Validates that `source_root` is not the same directory (or ancestor/descendant) of `dest_root`.
fn validate_not_same_root(source_root: &Path, dest_root: &Path) -> Result<()> {
    let src_canon = fs::canonicalize(source_root).unwrap_or_else(|_| source_root.to_path_buf());
    let dst_canon = fs::canonicalize(dest_root).unwrap_or_else(|_| dest_root.to_path_buf());
    if src_canon == dst_canon
        || dst_canon.starts_with(&src_canon)
        || src_canon.starts_with(&dst_canon)
    {
        return Err(StorageError::Backup(
            "Source Python launcher directory cannot overlap with the current Rust launcher folder."
                .to_string(),
        ));
    }
    Ok(())
}

/// Strictly read-only inspection of an existing MCW Launcher Python installation directory
/// (supports both legacy 1.8.x `.minecraft/` layout and modern 2.x/v0.5+ flat layout).
pub fn inspect_python_launcher(
    source_root: &Path,
    dest_paths: &LauncherPaths,
) -> Result<PythonMigrationInspection> {
    if !source_root.is_dir() {
        return Err(StorageError::Io {
            path: source_root.display().to_string(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Source Python launcher folder does not exist",
            ),
        });
    }
    validate_not_same_root(source_root, &dest_paths.project_root)?;

    let src_instances = source_root.join("instances");
    if !src_instances.is_dir() {
        return Err(StorageError::Io {
            path: src_instances.display().to_string(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Selected folder does not contain an instances/ directory",
            ),
        });
    }

    let mut inspection = PythonMigrationInspection {
        source_root: source_root.to_path_buf(),
        has_launcher_settings: source_root
            .join("config")
            .join("launcher_settings.json")
            .is_file(),
        has_accounts_db: source_root.join("accounts").join("accounts.db").is_file(),
        ..Default::default()
    };

    if inspection.has_accounts_db {
        inspection.warnings.push(
            "accounts/accounts.db detected: Microsoft OAuth/refresh tokens are encrypted by Python Windows DPAPI and cannot be imported directly. Please sign in again in Accounts after migration."
                .to_string(),
        );
    }

    if let Ok(entries) = fs::read_dir(&src_instances) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let folder_name = entry.file_name().to_string_lossy().to_string();
            if folder_name.starts_with('.') {
                continue;
            }
            let inst_json = dir.join("instance.json");
            if !inst_json.is_file() {
                continue;
            }
            let Ok(raw) = fs::read_to_string(&inst_json) else {
                inspection
                    .warnings
                    .push(format!("Could not read {}", inst_json.display()));
                continue;
            };
            let Ok(val) = serde_json::from_str::<Value>(&raw) else {
                inspection
                    .warnings
                    .push(format!("Invalid JSON in {}", inst_json.display()));
                continue;
            };
            let Some(parsed) = parse_python_instance(&val, &folder_name, &dir) else {
                inspection.warnings.push(format!(
                    "Skipping '{}': missing valid name/version_id",
                    folder_name
                ));
                continue;
            };

            let layout = if dir.join(".minecraft").is_dir() {
                PythonLayoutKind::LegacyMinecraftSubdir
            } else {
                PythonLayoutKind::ModernFlat
            };
            let estimated_bytes = estimate_instance_copy_bytes(&dir, layout);
            let already_exists = dest_paths.instance_dir(&parsed.instance_id).exists();

            inspection.instances.push(DiscoveredPythonInstance {
                folder_name,
                instance_id: parsed.instance_id,
                name: parsed.name,
                version_id: parsed.version_id,
                loader_name: parsed.mod_loader.name,
                loader_version: parsed.mod_loader.version,
                layout,
                estimated_bytes,
                has_mcw_meta: dir.join(".mcw").is_dir(),
                already_exists_in_dest: already_exists,
            });
        }
    }
    inspection.instances.sort_by_key(|a| a.name.to_lowercase());

    // Detect valid managed Java runtimes (`runtimes/java-*` containing `bin/java(.exe)`)
    let src_runtimes = source_root.join("runtimes");
    if src_runtimes.is_dir() {
        if let Ok(entries) = fs::read_dir(&src_runtimes) {
            for entry in entries.flatten() {
                let p = entry.path();
                if !p.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if name.eq_ignore_ascii_case("downloads") || name.starts_with('.') {
                    continue;
                }
                if p.join("bin").join("java.exe").is_file() || p.join("bin").join("java").is_file()
                {
                    inspection.managed_runtimes.push(name);
                }
            }
        }
        inspection.managed_runtimes.sort();
    }

    // Detect custom themes (`themes/<id>` with `theme.json` or standalone `.json`, skipping `mcw-default`)
    let src_themes = source_root.join("themes");
    if src_themes.is_dir() {
        if let Ok(entries) = fs::read_dir(&src_themes) {
            for entry in entries.flatten() {
                let p = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.')
                    || name.eq_ignore_ascii_case("mcw-default")
                    || name.eq_ignore_ascii_case("README.md")
                {
                    continue;
                }
                if (p.is_dir() && p.join("theme.json").is_file())
                    || (p.is_file()
                        && p.extension()
                            .and_then(|e| e.to_str())
                            .map(|e| e.eq_ignore_ascii_case("json"))
                            .unwrap_or(false))
                {
                    inspection.custom_themes.push(name);
                }
            }
        }
        inspection.custom_themes.sort();
    }

    Ok(inspection)
}

/// Safely migrates instances, `.mcw` registries, per-instance settings, optional global settings,
/// managed Java runtimes, and custom themes from MCW Launcher Python via strictly read-only copy.
pub fn migrate_from_python_launcher(
    source_root: &Path,
    dest_paths: &LauncherPaths,
    options: &PythonMigrationOptions,
    progress_cb: Option<&dyn Fn(usize, usize, String)>,
) -> Result<PythonMigrationResult> {
    let inspection = inspect_python_launcher(source_root, dest_paths)?;
    let selected: HashSet<&str> = options
        .selected_folders
        .iter()
        .map(String::as_str)
        .collect();

    let chosen_instances: Vec<&DiscoveredPythonInstance> = if selected.is_empty() {
        Vec::new()
    } else {
        inspection
            .instances
            .iter()
            .filter(|i| selected.contains(i.folder_name.as_str()))
            .collect()
    };

    let total_steps = chosen_instances.len()
        + usize::from(options.import_global_settings)
        + if options.import_runtimes {
            inspection.managed_runtimes.len()
        } else {
            0
        }
        + if options.import_themes {
            inspection.custom_themes.len()
        } else {
            0
        };
    let mut step = 0usize;

    let mut result = PythonMigrationResult {
        warnings: inspection.warnings.clone(),
        ..Default::default()
    };

    // Import managed Java runtimes FIRST so instance settings can remap `runtimes/...` paths cleanly.
    if options.import_runtimes {
        let src_runtimes = source_root.join("runtimes");
        for rt_name in &inspection.managed_runtimes {
            step += 1;
            if let Some(cb) = progress_cb {
                cb(
                    step,
                    total_steps.max(1),
                    format!("Copying runtime {rt_name}..."),
                );
            }
            let src_rt = src_runtimes.join(rt_name);
            let dst_rt = dest_paths.runtimes_root.join(rt_name);
            if dst_rt.exists() {
                result.warnings.push(format!(
                    "Runtime '{rt_name}' already exists in destination; kept existing."
                ));
                continue;
            }
            match copy_tree(&src_rt, &dst_rt) {
                Ok(()) => result.imported_runtimes.push(rt_name.clone()),
                Err(e) => result
                    .errors
                    .push(format!("Failed copying runtime '{rt_name}': {e}")),
            }
        }
    }

    // Import custom themes
    if options.import_themes {
        let src_themes = source_root.join("themes");
        for theme_name in &inspection.custom_themes {
            step += 1;
            if let Some(cb) = progress_cb {
                cb(
                    step,
                    total_steps.max(1),
                    format!("Copying theme {theme_name}..."),
                );
            }
            let src_th = src_themes.join(theme_name);
            let dst_th = dest_paths.theme_root.join(theme_name);
            if dst_th.exists() {
                continue;
            }
            let copy_res = if src_th.is_dir() {
                copy_tree(&src_th, &dst_th)
            } else {
                fs::create_dir_all(&dest_paths.theme_root)
                    .and_then(|_| fs::copy(&src_th, &dst_th).map(|_| ()))
            };
            match copy_res {
                Ok(()) => result.imported_themes.push(theme_name.clone()),
                Err(e) => result
                    .errors
                    .push(format!("Failed copying theme '{theme_name}': {e}")),
            }
        }
    }

    // Parse optional global launcher settings
    if options.import_global_settings && inspection.has_launcher_settings {
        step += 1;
        if let Some(cb) = progress_cb {
            cb(
                step,
                total_steps.max(1),
                "Importing global launcher preferences...".to_string(),
            );
        }
        let settings_file = source_root.join("config").join("launcher_settings.json");
        match fs::read_to_string(&settings_file)
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .map(|v| parse_python_global_settings(&v))
        {
            Some(gs) => result.imported_global_settings = Some(gs),
            None => result.warnings.push(
                "Could not parse config/launcher_settings.json; kept current global settings."
                    .to_string(),
            ),
        }
    }

    let store = InstanceStore::new(dest_paths);
    let src_instances = source_root.join("instances");
    for item in chosen_instances {
        step += 1;
        if let Some(cb) = progress_cb {
            cb(
                step,
                total_steps.max(1),
                format!("Importing instance '{}'...", item.name),
            );
        }
        let src_dir = src_instances.join(&item.folder_name);
        let inst_json_path = src_dir.join("instance.json");
        let Some(mut inst) = fs::read_to_string(&inst_json_path)
            .ok()
            .and_then(|r| serde_json::from_str::<Value>(&r).ok())
            .and_then(|v| parse_python_instance(&v, &item.folder_name, &src_dir))
        else {
            result
                .errors
                .push(format!("{}: invalid instance.json", item.folder_name));
            continue;
        };

        let target_id = if dest_paths.instance_dir(&inst.instance_id).exists() {
            match options.conflict_strategy {
                ConflictStrategy::Skip => {
                    result.skipped_instances.push(inst.instance_id);
                    continue;
                }
                ConflictStrategy::RenameImported => {
                    let unique = next_available_imported_id(dest_paths, &inst.instance_id);
                    inst.instance_id = unique.clone();
                    inst.name = unique.clone();
                    unique
                }
            }
        } else {
            inst.instance_id.clone()
        };

        let dest_inst_dir = dest_paths.instance_dir(&target_id);
        let dest_mc_dir = dest_paths.minecraft_dir(&target_id);
        if let Err(e) = fs::create_dir_all(&dest_mc_dir) {
            result.errors.push(format!("{target_id}: {e}"));
            continue;
        }

        // Copy game files according to source layout
        let game_copy_res = match item.layout {
            PythonLayoutKind::LegacyMinecraftSubdir => {
                copy_tree(&src_dir.join(".minecraft"), &dest_mc_dir)
            }
            PythonLayoutKind::ModernFlat => copy_modern_flat_game_files(&src_dir, &dest_mc_dir),
        };
        if let Err(e) = game_copy_res {
            let _ = fs::remove_dir_all(&dest_inst_dir);
            result.errors.push(format!("{target_id}: {e}"));
            continue;
        }

        // Copy & normalize `.mcw/` metadata + custom/pack icon
        if let Err(e) =
            migrate_mcw_metadata(&src_dir, &dest_inst_dir, &mut inst, &mut result.warnings)
        {
            result
                .warnings
                .push(format!("{target_id}: partial .mcw metadata copy ({e})"));
        }

        // Parse & sanitize instance `settings.json` (both modern nested & legacy flat)
        let settings_path = src_dir.join("settings.json");
        let mut settings = fs::read_to_string(&settings_path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .map(|v| parse_python_instance_settings(&v))
            .unwrap_or_default();
        sanitize_instance_java_path(
            &mut settings,
            source_root,
            dest_paths,
            &target_id,
            &mut result.warnings,
        );

        if let Err(e) = store.save_instance(&inst) {
            result.errors.push(format!("{target_id}: {e}"));
            continue;
        }
        if let Err(e) = store.save_settings(&target_id, &settings) {
            result.errors.push(format!("{target_id}: {e}"));
            continue;
        }

        result.imported_instances.push(target_id);
    }

    Ok(result)
}

fn next_available_imported_id(dest_paths: &LauncherPaths, base_id: &str) -> String {
    let base = format!("{base_id} (Imported)");
    let truncated = if base.chars().count() <= 80 {
        base
    } else {
        base_id
            .chars()
            .take(66)
            .collect::<String>()
            .trim_end()
            .to_string()
            + " (Imported)"
    };
    if !dest_paths.instance_dir(&truncated).exists() {
        if let Ok(valid) = InstanceStore::validate_instance_name(&truncated) {
            return valid;
        }
    }
    let mut idx = 2usize;
    loop {
        let suffix = format!(" (Imported {idx})");
        let max_prefix = 80usize.saturating_sub(suffix.chars().count());
        let prefix: String = base_id.chars().take(max_prefix).collect();
        let candidate = format!("{}{suffix}", prefix.trim_end());
        if !dest_paths.instance_dir(&candidate).exists() {
            if let Ok(valid) = InstanceStore::validate_instance_name(&candidate) {
                return valid;
            }
        }
        idx += 1;
    }
}

fn estimate_instance_copy_bytes(src_dir: &Path, layout: PythonLayoutKind) -> u64 {
    let walk_root = match layout {
        PythonLayoutKind::LegacyMinecraftSubdir => src_dir.join(".minecraft"),
        PythonLayoutKind::ModernFlat => src_dir.to_path_buf(),
    };
    if !walk_root.is_dir() {
        return 0;
    }
    let mut total = 0u64;
    let walker = WalkDir::new(&walk_root).into_iter().filter_entry(|e| {
        if e.depth() == 1 {
            let name = e.file_name().to_string_lossy();
            return !matches!(
                name.as_ref(),
                "logs" | "crash-reports" | ".mcw" | "downloads"
            );
        }
        true
    });
    for entry in walker.flatten() {
        if entry.file_type().is_file() {
            if let Ok(meta) = entry.metadata() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

fn parse_python_instance(v: &Value, folder_name: &str, src_dir: &Path) -> Option<Instance> {
    let raw_name = v
        .get("name")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(folder_name);
    let instance_id = InstanceStore::validate_instance_name(raw_name)
        .or_else(|_| InstanceStore::validate_instance_name(folder_name))
        .ok()?;
    let version_id = v
        .get("version_id")
        .and_then(|x| x.as_str())?
        .trim()
        .to_string();

    let mod_loader = match v.get("mod_loader").and_then(|x| x.as_array()) {
        Some(arr) if arr.len() >= 2 => ModLoader {
            name: arr[0]
                .as_str()
                .unwrap_or("vanilla")
                .trim()
                .to_ascii_lowercase(),
            version: arr[1].as_str().unwrap_or("-1").trim().to_string(),
        },
        _ => ModLoader::vanilla(),
    };

    let mut inst = Instance::new(&instance_id, raw_name, version_id);
    inst.mod_loader = mod_loader;

    if let Some(icon) = v.get("icon").and_then(|x| x.as_str()) {
        let trimmed = icon.trim();
        if !trimmed.is_empty() {
            inst.icon = trimmed.replace('\\', "/");
        }
    } else if let Some(found_ext) = ["png", "webp", "jpg", "jpeg", "gif"].iter().find(|ext| {
        src_dir
            .join(".mcw")
            .join(format!("instance-icon.{ext}"))
            .is_file()
    }) {
        inst.icon = format!(".mcw/instance-icon.{found_ext}");
    }

    if let Some(origin) = v.get("icon_origin") {
        if let Some(s) = origin.as_str() {
            inst.icon_origin = s.trim().to_string();
        } else if let Some(obj) = origin.as_object() {
            let prov = obj
                .get("provider")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim();
            let pid = obj
                .get("project_id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim();
            if !prov.is_empty() && !pid.is_empty() {
                inst.icon_origin = format!("{prov}:{pid}");
            } else if !prov.is_empty() {
                inst.icon_origin = format!("modpack:{prov}");
            }
        }
    }

    if let Some(pt) = v
        .get("total_play_time_seconds")
        .or_else(|| v.get("total_playtime_seconds"))
        .and_then(|x| x.as_u64())
    {
        inst.total_playtime_seconds = pt;
    }
    if let Some(lp) = v.get("last_played").and_then(|x| x.as_str()) {
        inst.last_played = lp.to_string();
    }
    if let Some(code) = v.get("last_exit_code").and_then(|x| x.as_i64()) {
        inst.last_exit_code = Some(code as i32);
    }
    if let Some(crashed) = v.get("last_launch_crashed").and_then(|x| x.as_bool()) {
        inst.last_launch_crashed = crashed;
    }
    if let Some(state) = v.get("last_launch_state").and_then(|x| x.as_str()) {
        let s = state.trim();
        if !s.is_empty() {
            inst.last_launch_state = s.to_string();
        }
    }
    if let Some(fav) = v.get("favorite").and_then(|x| x.as_bool()) {
        inst.favorite = fav;
    }
    if let Some(grp) = v.get("group").and_then(|x| x.as_str()) {
        inst.group = grp.trim().to_string();
    }
    if let Some(tags) = v.get("tags").and_then(|x| x.as_array()) {
        inst.tags = tags
            .iter()
            .filter_map(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .collect();
    }
    Some(inst)
}

fn parse_string_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_python_instance_settings(v: &Value) -> InstanceSettings {
    let mut s = InstanceSettings::default();
    if let Some(java) = v.get("java").and_then(|x| x.as_object()) {
        if let Some(path) = java
            .get("path")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            s.java_path = Some(PathBuf::from(path));
        }
        if let Some(min_m) = java.get("min_memory").and_then(|x| x.as_u64()) {
            s.min_memory = min_m as u32;
        }
        if let Some(max_m) = java.get("max_memory").and_then(|x| x.as_u64()) {
            s.max_memory = max_m as u32;
        }
        let args = parse_string_list(java.get("arguments"));
        if !args.is_empty() {
            s.jvm_arguments = args;
        }
    } else {
        if let Some(path) = v
            .get("java_path")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            s.java_path = Some(PathBuf::from(path));
        }
        if let Some(min_m) = v.get("min_memory").and_then(|x| x.as_u64()) {
            s.min_memory = min_m as u32;
        }
        if let Some(max_m) = v.get("max_memory").and_then(|x| x.as_u64()) {
            s.max_memory = max_m as u32;
        }
        let args = parse_string_list(v.get("jvm_arguments"));
        if !args.is_empty() {
            s.jvm_arguments = args;
        }
    }

    if let Some(win) = v.get("window").and_then(|x| x.as_object()) {
        if let Some(w) = win.get("width").and_then(|x| x.as_u64()) {
            s.width = w as u32;
        }
        if let Some(h) = win.get("height").and_then(|x| x.as_u64()) {
            s.height = h as u32;
        }
        if let Some(fs_flag) = win.get("fullscreen").and_then(|x| x.as_bool()) {
            s.fullscreen = fs_flag;
        }
    } else {
        if let Some(w) = v.get("width").and_then(|x| x.as_u64()) {
            s.width = w as u32;
        }
        if let Some(h) = v.get("height").and_then(|x| x.as_u64()) {
            s.height = h as u32;
        }
        if let Some(fs_flag) = v.get("fullscreen").and_then(|x| x.as_bool()) {
            s.fullscreen = fs_flag;
        }
    }

    if let Some(launch) = v.get("launch").and_then(|x| x.as_object()) {
        let gargs = parse_string_list(launch.get("game_arguments"));
        if !gargs.is_empty() {
            s.game_arguments = gargs;
        }
        if let Some(off) = launch
            .get("offline_multiplayer_enabled")
            .and_then(|x| x.as_bool())
        {
            s.offline_multiplayer_enabled = off;
        }
        if let Some(mode) = launch.get("lan_auth_mode").and_then(|x| x.as_str()) {
            s.lan_auth_mode = mode.to_string();
        }
        // Per DESIGN.md/v0.13 parity rule: map `e4mc` -> `manual` (no built-in tunnel option).
        if let Some(prov) = launch
            .get("lan_connection_provider")
            .and_then(|x| x.as_str())
        {
            s.lan_connection_provider = if prov.eq_ignore_ascii_case("e4mc") {
                "manual".to_string()
            } else {
                prov.to_string()
            };
        }
        if let Some(mr) = launch
            .get("modrinth_failure_policy")
            .and_then(|x| x.as_str())
        {
            s.modrinth_failure_policy = mr.to_string();
        }
        if let Some(cf) = launch
            .get("curseforge_failure_policy")
            .and_then(|x| x.as_str())
        {
            s.curseforge_failure_policy = cf.to_string();
        }
    }

    s
}

fn sanitize_instance_java_path(
    settings: &mut InstanceSettings,
    source_root: &Path,
    dest_paths: &LauncherPaths,
    instance_id: &str,
    warnings: &mut Vec<String>,
) {
    let Some(ref jp) = settings.java_path else {
        return;
    };
    let src_runtimes = source_root.join("runtimes");
    if let Ok(rel) = jp.strip_prefix(&src_runtimes) {
        let remapped = dest_paths.runtimes_root.join(rel);
        if remapped.is_file() {
            settings.java_path = Some(remapped);
            return;
        }
    }
    if jp.starts_with(source_root) || !jp.is_file() {
        warnings.push(format!(
            "Instance '{instance_id}': cleared unreachable custom Java path '{}' back to Automatic.",
            jp.display()
        ));
        settings.java_path = None;
    }
}

fn parse_python_global_settings(v: &Value) -> ImportedGlobalSettings {
    let mut gs = ImportedGlobalSettings::default();
    if let Some(gui) = v.get("gui").and_then(|x| x.as_object()) {
        gs.language = gui
            .get("language")
            .and_then(|x| x.as_str())
            .map(ToString::to_string);
        gs.show_snapshots = gui.get("show_snapshots").and_then(|x| x.as_bool());
        gs.remember_window_size = gui.get("remember_window_size").and_then(|x| x.as_bool());
        gs.show_content_descriptions = gui
            .get("show_content_descriptions")
            .and_then(|x| x.as_bool());
    }
    if let Some(launch) = v.get("launch").and_then(|x| x.as_object()) {
        gs.debug_mode = launch.get("debug_mode").and_then(|x| x.as_bool());
        gs.prefer_dedicated_gpu = launch.get("prefer_dedicated_gpu").and_then(|x| x.as_bool());
        gs.discord_rpc_enabled = launch.get("discord_rpc_enabled").and_then(|x| x.as_bool());
        gs.discord_client_id = launch
            .get("discord_client_id")
            .and_then(|x| x.as_str())
            .map(ToString::to_string);
        gs.minimize_on_launch = launch.get("minimize_on_launch").and_then(|x| x.as_bool());
    }
    if let Some(app) = v.get("appearance").and_then(|x| x.as_object()) {
        gs.theme = app
            .get("theme")
            .and_then(|x| x.as_str())
            .map(ToString::to_string);
        gs.motion_mode = app
            .get("motion_mode")
            .and_then(|x| x.as_str())
            .map(ToString::to_string);
        if app
            .get("accent_mode")
            .and_then(|x| x.as_str())
            .map(|m| m.eq_ignore_ascii_case("custom"))
            .unwrap_or(false)
        {
            gs.custom_accent_hex = app
                .get("accent_color")
                .and_then(|x| x.as_str())
                .map(ToString::to_string);
        }
    }
    if let Some(mr) = v.get("modrinth").and_then(|x| x.as_object()) {
        gs.modrinth_include_beta = mr.get("include_beta").and_then(|x| x.as_bool());
        gs.modrinth_include_alpha = mr.get("include_alpha").and_then(|x| x.as_bool());
    }
    if let Some(mc) = v.get("managed_content").and_then(|x| x.as_object()) {
        gs.modrinth_failure_policy = mc
            .get("modrinth_failure_policy")
            .and_then(|x| x.as_str())
            .map(ToString::to_string);
        gs.curseforge_failure_policy = mc
            .get("curseforge_failure_policy")
            .and_then(|x| x.as_str())
            .map(ToString::to_string);
    }
    if let Some(net) = v.get("network").and_then(|x| x.as_object()) {
        gs.download_limit_mbps = net.get("download_limit_mbps").and_then(|x| x.as_f64());
        gs.download_concurrency = net
            .get("download_concurrency")
            .and_then(|x| x.as_u64())
            .map(|n| n as u32);
    }
    if let Some(st) = v.get("storage").and_then(|x| x.as_object()) {
        gs.unused_version_retention_days = st
            .get("unused_version_retention_days")
            .and_then(|x| x.as_u64())
            .map(|n| n as u32);
    }
    if let Some(defs) = v.get("instance_defaults").and_then(|x| x.as_object()) {
        if let Some(j) = defs.get("java").and_then(|x| x.as_object()) {
            gs.default_min_memory = j
                .get("min_memory")
                .and_then(|x| x.as_u64())
                .map(|n| n as u32);
            gs.default_max_memory = j
                .get("max_memory")
                .and_then(|x| x.as_u64())
                .map(|n| n as u32);
        }
        if let Some(w) = defs.get("window").and_then(|x| x.as_object()) {
            gs.default_width = w.get("width").and_then(|x| x.as_u64()).map(|n| n as u32);
            gs.default_height = w.get("height").and_then(|x| x.as_u64()).map(|n| n as u32);
            gs.default_fullscreen = w.get("fullscreen").and_then(|x| x.as_bool());
        }
    }
    if let Some(upd) = v.get("updates").and_then(|x| x.as_object()) {
        gs.auto_check_updates = upd.get("auto_check").and_then(|x| x.as_bool());
        gs.update_channel = upd
            .get("channel")
            .and_then(|x| x.as_str())
            .map(ToString::to_string);
    }
    gs
}

fn copy_modern_flat_game_files(src_inst_dir: &Path, dest_mc_dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dest_mc_dir)?;
    for entry in fs::read_dir(src_inst_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if matches!(
            name_str.as_ref(),
            "instance.json" | "settings.json" | ".mcw" | "logs" | "crash-reports"
        ) {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dest_mc_dir.join(&name);
        if src_path.is_dir() {
            copy_tree(&src_path, &dst_path)?;
        } else if src_path.is_file() {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn migrate_mcw_metadata(
    src_inst_dir: &Path,
    dest_inst_dir: &Path,
    inst: &mut Instance,
    warnings: &mut Vec<String>,
) -> std::io::Result<()> {
    let src_mcw = src_inst_dir.join(".mcw");
    let dst_mcw = dest_inst_dir.join(".mcw");
    if src_mcw.is_dir() {
        fs::create_dir_all(&dst_mcw)?;
        for entry in fs::read_dir(&src_mcw)? {
            let entry = entry?;
            let fname = entry.file_name().to_string_lossy().to_string();
            // Skip ephemeral locks / runtime caches that should regenerate natively
            if matches!(
                fname.as_str(),
                "run.lock" | "repair-verification-cache.json" | "runtime-history.json" | "launch"
            ) {
                continue;
            }
            let src_path = entry.path();
            let dst_path = dst_mcw.join(&fname);
            if fname == "modrinth-pack.json" {
                normalize_and_write_modrinth_pack(&src_path, &dst_path)?;
            } else if fname == "ftb-pack.json" {
                normalize_and_write_pack_id_name(&src_path, &dst_path, "ftb", 2)?;
            } else if fname == "atlauncher-pack.json" {
                normalize_and_write_pack_id_name(&src_path, &dst_path, "atlauncher", 1)?;
            } else if fname == "mod-provenance.json" {
                normalize_and_write_mod_provenance(&src_path, &dst_path)?;
            } else if src_path.is_dir() {
                copy_tree(&src_path, &dst_path)?;
            } else if src_path.is_file() {
                fs::copy(&src_path, &dst_path)?;
            }
        }
    }

    // Handle external/absolute custom icon path if one was referenced outside `.mcw/`
    let icon_trimmed = inst.icon.trim().to_string();
    if icon_trimmed.contains('/') || icon_trimmed.contains('\\') {
        let icon_as_path = PathBuf::from(&icon_trimmed);
        if icon_as_path.is_absolute() {
            if icon_as_path.is_file() {
                if let Some(ext) = icon_as_path.extension().and_then(|e| e.to_str()) {
                    fs::create_dir_all(&dst_mcw)?;
                    let dest_icon = dst_mcw.join(format!("instance-icon.{ext}"));
                    if fs::copy(&icon_as_path, &dest_icon).is_ok() {
                        inst.icon = format!(".mcw/instance-icon.{ext}");
                        return Ok(());
                    }
                }
            }
            warnings.push(format!(
                "Instance '{}': external icon '{}' was missing; reverted to grass_block.",
                inst.name, icon_trimmed
            ));
            inst.icon = "grass_block".to_string();
        } else {
            let candidate = dest_inst_dir.join(&icon_trimmed);
            if !candidate.is_file() {
                inst.icon = "grass_block".to_string();
            }
        }
    }
    Ok(())
}

fn normalize_and_write_modrinth_pack(src: &Path, dst: &Path) -> std::io::Result<()> {
    let raw = fs::read_to_string(src)?;
    let Ok(mut val) = serde_json::from_str::<Value>(&raw) else {
        fs::copy(src, dst)?;
        return Ok(());
    };
    let Some(obj) = val.as_object_mut() else {
        fs::copy(src, dst)?;
        return Ok(());
    };
    let project_id = obj
        .get("projectId")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let managed_empty = obj
        .get("managedFiles")
        .and_then(|x| x.as_array())
        .map(|a| a.is_empty())
        .unwrap_or(true);
    // Python creates empty `modrinth-pack.json` verificationCache stubs even on custom instances;
    // omit empty stubs so custom instances are not misclassified as Modrinth modpacks.
    if project_id.is_empty() && managed_empty {
        return Ok(());
    }

    if !obj.contains_key("packName") {
        if let Some(name) = obj
            .get("name")
            .and_then(|x| x.as_str())
            .map(ToString::to_string)
        {
            obj.insert("packName".to_string(), Value::String(name));
        }
    }
    if !obj.contains_key("source") {
        obj.insert("source".to_string(), Value::String("modrinth".to_string()));
    }
    if let Some(Value::Array(arr)) = obj.get("managedFiles").cloned() {
        let converted: Vec<Value> = arr
            .into_iter()
            .filter_map(|item| match item {
                Value::String(s) => Some(Value::String(s)),
                Value::Object(map) => map
                    .get("path")
                    .and_then(|p| p.as_str())
                    .map(|p| Value::String(p.replace('\\', "/"))),
                _ => None,
            })
            .collect();
        obj.insert("managedFiles".to_string(), Value::Array(converted));
    }

    atomic_write_json(dst, &val).map_err(std::io::Error::other)
}

fn normalize_and_write_pack_id_name(
    src: &Path,
    dst: &Path,
    source_name: &str,
    default_schema: u32,
) -> std::io::Result<()> {
    let raw = fs::read_to_string(src)?;
    let Ok(mut val) = serde_json::from_str::<Value>(&raw) else {
        fs::copy(src, dst)?;
        return Ok(());
    };
    let Some(obj) = val.as_object_mut() else {
        fs::copy(src, dst)?;
        return Ok(());
    };
    if !obj.contains_key("packId") {
        if let Some(pid) = obj.get("projectId").cloned() {
            obj.insert("packId".to_string(), pid);
        }
    }
    if !obj.contains_key("packName") {
        if let Some(name) = obj.get("name").cloned() {
            obj.insert("packName".to_string(), name);
        }
    }
    if !obj.contains_key("source") {
        obj.insert("source".to_string(), Value::String(source_name.to_string()));
    }
    if !obj.contains_key("schemaVersion") {
        obj.insert(
            "schemaVersion".to_string(),
            Value::Number((default_schema as u64).into()),
        );
    }
    atomic_write_json(dst, &val).map_err(std::io::Error::other)
}

fn normalize_and_write_mod_provenance(src: &Path, dst: &Path) -> std::io::Result<()> {
    let raw = fs::read_to_string(src)?;
    let Ok(val) = serde_json::from_str::<Value>(&raw) else {
        fs::copy(src, dst)?;
        return Ok(());
    };
    let Some(mods_map) = val.get("mods").and_then(|x| x.as_object()) else {
        fs::copy(src, dst)?;
        return Ok(());
    };

    // Already in Rust snake_case format?
    let already_rust = mods_map
        .values()
        .next()
        .and_then(|v| v.as_object())
        .map(|o| o.contains_key("file_name") && o.contains_key("project_id"))
        .unwrap_or(false);
    if already_rust {
        fs::copy(src, dst)?;
        return Ok(());
    }

    let mut out_mods = Map::new();
    for (orig_key, entry_val) in mods_map {
        let Some(entry) = entry_val.as_object() else {
            continue;
        };
        let provider = entry
            .get("provider")
            .and_then(|x| x.as_str())
            .unwrap_or("modrinth")
            .trim()
            .to_ascii_lowercase();
        let project_id = entry
            .get("projectId")
            .and_then(|x| match x {
                Value::String(s) => Some(s.trim().to_string()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .unwrap_or_default();
        if project_id.is_empty() {
            continue;
        }
        let file_name = entry
            .get("fileName")
            .and_then(|x| x.as_str())
            .unwrap_or(orig_key)
            .to_string();
        let title = entry
            .get("title")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| {
                file_name
                    .trim_end_matches(".disabled")
                    .trim_end_matches(".jar")
                    .to_string()
            });
        let version_id = entry
            .get("versionId")
            .or_else(|| entry.get("fileId"))
            .and_then(|x| match x {
                Value::String(s) => Some(s.trim().to_string()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .unwrap_or_default();
        let version_number = entry
            .get("versionNumber")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();

        let mut rust_entry = Map::new();
        rust_entry.insert("project_id".to_string(), Value::String(project_id.clone()));
        rust_entry.insert("provider".to_string(), Value::String(provider.clone()));
        rust_entry.insert("title".to_string(), Value::String(title));
        rust_entry.insert("file_name".to_string(), Value::String(file_name));
        rust_entry.insert("version_id".to_string(), Value::String(version_id));
        rust_entry.insert("version_number".to_string(), Value::String(version_number));
        rust_entry.insert("locked".to_string(), Value::Bool(false));
        if let Some(mb) = entry.get("managedByModpack").and_then(|x| x.as_bool()) {
            if mb {
                rust_entry.insert(
                    "managed_by".to_string(),
                    Value::String("modpack".to_string()),
                );
            }
        }
        let rust_key = format!("{provider}:{project_id}");
        out_mods.insert(rust_key, Value::Object(rust_entry));
    }

    let mut root = Map::new();
    root.insert("mods".to_string(), Value::Object(out_mods));
    atomic_write_json(dst, &Value::Object(root)).map_err(std::io::Error::other)
}

/// Migrate instances and settings from a Python MCW Launcher `1.8.x` directory
/// into the Rust Edition workspace without mutating or deleting source data (used by CLI `migrate`).
pub fn migrate_from_python_1_8(
    source_root: &Path,
    dest_paths: &LauncherPaths,
    dry_run: bool,
) -> Result<MigrationReport> {
    let src_instances = source_root.join("instances");
    if !src_instances.is_dir() {
        return Err(StorageError::Io {
            path: src_instances.display().to_string(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Source directory does not contain an instances/ folder",
            ),
        });
    }

    let mut report = MigrationReport {
        source_root: source_root.to_path_buf(),
        ..Default::default()
    };
    let store = InstanceStore::new(dest_paths);

    let entries = fs::read_dir(&src_instances).map_err(|source| StorageError::Io {
        path: src_instances.display().to_string(),
        source,
    })?;

    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let folder_name = entry.file_name().to_string_lossy().to_string();
        if folder_name.starts_with('.') {
            continue;
        }
        let inst_json = dir.join("instance.json");
        if !inst_json.is_file() {
            continue;
        }

        let raw = match fs::read_to_string(&inst_json) {
            Ok(r) => r,
            Err(e) => {
                report.errors.push(format!("{}: {e}", inst_json.display()));
                continue;
            }
        };
        let val: Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                report.errors.push(format!("{}: {e}", inst_json.display()));
                continue;
            }
        };

        let Some(parsed_inst) = parse_python_instance(&val, &folder_name, &dir) else {
            report
                .errors
                .push(format!("{}: missing required fields", inst_json.display()));
            continue;
        };

        if dest_paths.instance_dir(&parsed_inst.instance_id).exists() {
            report.skipped_existing.push(parsed_inst.instance_id);
            continue;
        }

        if !dry_run {
            let dest_mc = dest_paths.minecraft_dir(&parsed_inst.instance_id);
            let src_mc = dir.join(".minecraft");
            let copy_res = if src_mc.is_dir() {
                copy_tree(&src_mc, &dest_mc)
            } else {
                copy_modern_flat_game_files(&dir, &dest_mc)
            };
            if let Err(e) = copy_res {
                report.errors.push(format!("{}: {e}", parsed_inst.name));
                continue;
            }

            if let Err(e) = store.save_instance(&parsed_inst) {
                report.errors.push(format!("{}: {e}", parsed_inst.name));
                continue;
            }

            let settings_path = dir.join("settings.json");
            let settings = fs::read_to_string(&settings_path)
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .map(|v| parse_python_instance_settings(&v))
                .unwrap_or_default();
            let _ = store.save_settings(&parsed_inst.instance_id, &settings);
        }

        report.migrated_instances.push(parsed_inst.instance_id);
    }

    Ok(report)
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in WalkDir::new(from).into_iter().filter_map(|e| e.ok()) {
        let rel = entry
            .path()
            .strip_prefix(from)
            .map_err(std::io::Error::other)?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let dest = to.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&dest)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn migrates_python_1_8_instance_idempotently() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();

        let py_inst = src.path().join("instances").join("Classic");
        fs::create_dir_all(py_inst.join(".minecraft").join("saves")).unwrap();
        fs::write(
            py_inst.join("instance.json"),
            r#"{"id":"Classic","name":"Classic","version_id":"1.20.4","mod_loader":["fabric","0.16.0"],"total_play_time_seconds":1200}"#,
        )
        .unwrap();
        fs::write(
            py_inst.join("settings.json"),
            r#"{"min_memory":2048,"max_memory":4096,"width":1280,"height":720}"#,
        )
        .unwrap();
        fs::write(
            py_inst.join(".minecraft").join("options.txt"),
            b"lang:vi_vn\n",
        )
        .unwrap();

        let dest_paths = LauncherPaths::portable(dst.path());
        dest_paths.initialize().unwrap();

        let report = migrate_from_python_1_8(src.path(), &dest_paths, false).unwrap();
        assert_eq!(report.migrated_instances, vec!["Classic".to_string()]);
        assert!(report.errors.is_empty());

        let store = InstanceStore::new(&dest_paths);
        let (inst, settings) = store.get_instance("Classic").unwrap();
        assert_eq!(inst.mod_loader.name, "fabric");
        assert_eq!(inst.total_playtime_seconds, 1200);
        assert_eq!(settings.max_memory, 4096);

        let second = migrate_from_python_1_8(src.path(), &dest_paths, false).unwrap();
        assert_eq!(second.skipped_existing, vec!["Classic".to_string()]);
    }

    #[test]
    fn inspects_and_migrates_modern_flat_python_launcher_safely() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();

        fs::create_dir_all(src.path().join("config")).unwrap();
        fs::write(
            src.path().join("config").join("launcher_settings.json"),
            r##"{
                "gui": { "language": "vi-VN", "show_snapshots": true },
                "launch": { "debug_mode": true, "prefer_dedicated_gpu": true },
                "appearance": { "theme": "mcw-default", "accent_mode": "custom", "accent_color": "#10b981" },
                "instance_defaults": { "java": { "min_memory": 2048, "max_memory": 6144 } }
            }"##,
        )
        .unwrap();
        fs::create_dir_all(src.path().join("accounts")).unwrap();
        fs::write(src.path().join("accounts").join("accounts.db"), b"sqlite").unwrap();
        fs::create_dir_all(src.path().join("runtimes").join("downloads")).unwrap();
        fs::create_dir_all(src.path().join("runtimes").join("java-21").join("bin")).unwrap();
        fs::write(
            src.path()
                .join("runtimes")
                .join("java-21")
                .join("bin")
                .join("java.exe"),
            b"exe",
        )
        .unwrap();
        fs::create_dir_all(src.path().join("themes").join("ocean-blue")).unwrap();
        fs::write(
            src.path()
                .join("themes")
                .join("ocean-blue")
                .join("theme.json"),
            r#"{"id":"ocean-blue","name":"Ocean"}"#,
        )
        .unwrap();

        let fo_dir = src.path().join("instances").join("Fabulously Optimized");
        fs::create_dir_all(fo_dir.join("mods")).unwrap();
        fs::create_dir_all(fo_dir.join(".mcw")).unwrap();
        fs::write(fo_dir.join("mods").join("sodium.jar"), b"jar-bytes").unwrap();
        fs::write(fo_dir.join("options.txt"), b"fov:1.0\n").unwrap();
        fs::write(fo_dir.join(".mcw").join("instance-icon.webp"), b"webp").unwrap();
        fs::write(
            fo_dir.join(".mcw").join("modrinth-pack.json"),
            r#"{
                "projectId": "1KVo5zza",
                "versionId": "D35n5vLL",
                "name": "Fabulously Optimized",
                "versionNumber": "15.0.0",
                "managedFiles": [{ "path": "mods/sodium.jar", "sha1": "abc" }]
            }"#,
        )
        .unwrap();
        fs::write(
            fo_dir.join("instance.json"),
            r#"{
                "id": "084ba063-8213-4042-aedf-3e87df2d1745",
                "name": "Fabulously Optimized",
                "version_id": "26.3",
                "mod_loader": ["fabric", "0.19.5"],
                "icon": ".mcw/instance-icon.webp",
                "icon_origin": { "provider": "modrinth", "project_id": "1KVo5zza" },
                "total_play_time_seconds": 174
            }"#,
        )
        .unwrap();
        fs::write(
            fo_dir.join("settings.json"),
            format!(
                r#"{{
                    "java": {{ "path": "{}", "min_memory": 1024, "max_memory": 6144, "arguments": ["-XX:+UseG1GC"] }},
                    "window": {{ "width": 1280, "height": 720, "fullscreen": false }},
                    "launch": {{ "lan_auth_mode": "private_offline", "lan_connection_provider": "e4mc" }}
                }}"#,
                src.path()
                    .join("runtimes")
                    .join("java-21")
                    .join("bin")
                    .join("java.exe")
                    .display()
                    .to_string()
                    .replace('\\', "\\\\")
            ),
        )
        .unwrap();

        let dest_paths = LauncherPaths::portable(dst.path());
        dest_paths.initialize().unwrap();

        let insp = inspect_python_launcher(src.path(), &dest_paths).unwrap();
        assert!(insp.has_launcher_settings);
        assert!(insp.has_accounts_db);
        assert_eq!(insp.managed_runtimes, vec!["java-21".to_string()]);
        assert_eq!(insp.custom_themes, vec!["ocean-blue".to_string()]);
        assert_eq!(insp.instances.len(), 1);
        assert_eq!(insp.instances[0].layout, PythonLayoutKind::ModernFlat);
        assert_eq!(insp.instances[0].instance_id, "Fabulously Optimized");

        let opts = PythonMigrationOptions {
            selected_folders: vec!["Fabulously Optimized".to_string()],
            conflict_strategy: ConflictStrategy::RenameImported,
            import_global_settings: true,
            import_runtimes: true,
            import_themes: true,
        };
        let res = migrate_from_python_launcher(src.path(), &dest_paths, &opts, None).unwrap();
        assert_eq!(
            res.imported_instances,
            vec!["Fabulously Optimized".to_string()]
        );
        assert_eq!(res.imported_runtimes, vec!["java-21".to_string()]);
        assert_eq!(res.imported_themes, vec!["ocean-blue".to_string()]);
        let gs = res.imported_global_settings.unwrap();
        assert_eq!(gs.language.as_deref(), Some("vi-VN"));
        assert_eq!(gs.custom_accent_hex.as_deref(), Some("#10b981"));

        let store = InstanceStore::new(&dest_paths);
        let (inst, settings) = store.get_instance("Fabulously Optimized").unwrap();
        assert_eq!(inst.icon, ".mcw/instance-icon.webp");
        assert_eq!(inst.icon_origin, "modrinth:1KVo5zza");
        assert_eq!(settings.max_memory, 6144);
        assert_eq!(settings.lan_connection_provider, "manual");
        assert_eq!(
            settings.java_path.as_deref(),
            Some(
                dest_paths
                    .runtimes_root
                    .join("java-21")
                    .join("bin")
                    .join("java.exe")
                    .as_path()
            )
        );
        assert!(dest_paths
            .minecraft_dir("Fabulously Optimized")
            .join("mods")
            .join("sodium.jar")
            .is_file());
        assert!(dest_paths
            .instance_dir("Fabulously Optimized")
            .join(".mcw")
            .join("instance-icon.webp")
            .is_file());

        // Second run with RenameImported creates "(Imported)" safely without touching source
        let res2 = migrate_from_python_launcher(src.path(), &dest_paths, &opts, None).unwrap();
        assert_eq!(
            res2.imported_instances,
            vec!["Fabulously Optimized (Imported)".to_string()]
        );
        assert!(fo_dir.join("mods").join("sodium.jar").is_file());
    }
}
