use crate::{InstanceStore, LauncherPaths, Result, StorageError};
use mcw_models::instance::{Instance, InstanceSettings, ModLoader};
use serde::{Deserialize, Serialize};
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

/// Migrate instances and settings from a Python MCW Launcher `1.8.x` directory
/// into the Rust Edition workspace without mutating or deleting source data.
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
        if entry.file_name().to_string_lossy().starts_with('.') {
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
        let val: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                report.errors.push(format!("{}: {e}", inst_json.display()));
                continue;
            }
        };

        let Some(parsed_inst) = parse_legacy_instance(&val) else {
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
            if src_mc.is_dir() {
                if let Err(e) = copy_tree(&src_mc, &dest_mc) {
                    report.errors.push(format!("{}: {e}", parsed_inst.name));
                    continue;
                }
            } else {
                let _ = fs::create_dir_all(&dest_mc);
            }

            if let Err(e) = store.save_instance(&parsed_inst) {
                report.errors.push(format!("{}: {e}", parsed_inst.name));
                continue;
            }

            let settings_path = dir.join("settings.json");
            let settings = fs::read_to_string(&settings_path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .map(|v| parse_legacy_settings(&v))
                .unwrap_or_default();
            let _ = store.save_settings(&parsed_inst.instance_id, &settings);
        }

        report.migrated_instances.push(parsed_inst.instance_id);
    }

    Ok(report)
}

fn parse_legacy_instance(v: &serde_json::Value) -> Option<Instance> {
    let raw_name = v
        .get("name")
        .or_else(|| v.get("id"))
        .and_then(|x| x.as_str())?
        .trim();
    let instance_id = InstanceStore::validate_instance_name(raw_name).ok()?;
    let version_id = v.get("version_id").and_then(|x| x.as_str())?.trim().to_string();

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

    let mut inst = Instance::new(&instance_id, &instance_id, version_id);
    inst.mod_loader = mod_loader;
    if let Some(icon) = v.get("icon").and_then(|x| x.as_str()) {
        inst.icon = icon.to_string();
    }
    if let Some(pt) = v.get("total_play_time_seconds").and_then(|x| x.as_u64()) {
        inst.total_playtime_seconds = pt;
    }
    if let Some(fav) = v.get("favorite").and_then(|x| x.as_bool()) {
        inst.favorite = fav;
    }
    Some(inst)
}

fn parse_legacy_settings(v: &serde_json::Value) -> InstanceSettings {
    let mut s = InstanceSettings::default();
    if let Some(min_m) = v.get("min_memory").and_then(|x| x.as_u64()) {
        s.min_memory = min_m as u32;
    }
    if let Some(max_m) = v.get("max_memory").and_then(|x| x.as_u64()) {
        s.max_memory = max_m as u32;
    }
    if let Some(w) = v.get("width").and_then(|x| x.as_u64()) {
        s.width = w as u32;
    }
    if let Some(h) = v.get("height").and_then(|x| x.as_u64()) {
        s.height = h as u32;
    }
    if let Some(fs_flag) = v.get("fullscreen").and_then(|x| x.as_bool()) {
        s.fullscreen = fs_flag;
    }
    s
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
        fs::write(py_inst.join(".minecraft").join("options.txt"), b"lang:vi_vn\n").unwrap();

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

        // Second run skips already migrated instance
        let second = migrate_from_python_1_8(src.path(), &dest_paths, false).unwrap();
        assert_eq!(second.skipped_existing, vec!["Classic".to_string()]);
    }
}
