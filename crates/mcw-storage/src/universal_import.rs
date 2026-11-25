use crate::migration::copy_tree;
use crate::{InstanceStore, LauncherPaths, Result, StorageError};
use mcw_models::instance::ModLoader;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExternalLauncherKind {
    PrismLauncher,
    ModrinthApp,
    CurseForgeApp,
    VanillaMinecraft,
}

impl ExternalLauncherKind {
    pub const ALL: [Self; 4] = [
        Self::PrismLauncher,
        Self::ModrinthApp,
        Self::CurseForgeApp,
        Self::VanillaMinecraft,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::PrismLauncher => "Prism Launcher / MultiMC",
            Self::ModrinthApp => "Modrinth App",
            Self::CurseForgeApp => "CurseForge App",
            Self::VanillaMinecraft => "Vanilla .minecraft",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredExternalInstance {
    pub kind: ExternalLauncherKind,
    pub id_key: String,
    pub name: String,
    pub version_id: String,
    pub loader_name: String,
    pub loader_version: String,
    pub mc_dir: PathBuf,
    pub notes: String,
    pub min_memory: Option<u32>,
    pub max_memory: Option<u32>,
    pub jvm_args: Vec<String>,
    pub mod_count: usize,
    pub world_count: usize,
}

pub fn default_root_candidates_for_kind(kind: ExternalLauncherKind) -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
        let userprofile = std::env::var_os("USERPROFILE").map(PathBuf::from);
        match kind {
            ExternalLauncherKind::PrismLauncher => {
                if let Some(ref ad) = appdata {
                    out.push(ad.join("PrismLauncher").join("instances"));
                    out.push(ad.join("MultiMC").join("instances"));
                }
            }
            ExternalLauncherKind::ModrinthApp => {
                if let Some(ref ad) = appdata {
                    out.push(ad.join("ModrinthApp").join("profiles"));
                    out.push(ad.join("com.modrinth.theseus").join("profiles"));
                }
            }
            ExternalLauncherKind::CurseForgeApp => {
                if let Some(ref up) = userprofile {
                    out.push(up.join("curseforge").join("minecraft").join("Instances"));
                }
            }
            ExternalLauncherKind::VanillaMinecraft => {
                if let Some(ref ad) = appdata {
                    out.push(ad.join(".minecraft"));
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(".local/share")));
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(".config")));
        match kind {
            ExternalLauncherKind::PrismLauncher => {
                if let Some(ref d) = data_home {
                    out.push(d.join("PrismLauncher").join("instances"));
                    out.push(d.join("MultiMC").join("instances"));
                }
                if let Some(ref h) = home {
                    out.push(h.join(
                        ".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher/instances",
                    ));
                }
            }
            ExternalLauncherKind::ModrinthApp => {
                if let Some(ref c) = config_home {
                    out.push(c.join("ModrinthApp").join("profiles"));
                }
                if let Some(ref d) = data_home {
                    out.push(d.join("ModrinthApp").join("profiles"));
                    out.push(d.join("com.modrinth.theseus").join("profiles"));
                }
            }
            ExternalLauncherKind::CurseForgeApp => {
                if let Some(ref h) = home {
                    out.push(h.join("curseforge/minecraft/Instances"));
                }
            }
            ExternalLauncherKind::VanillaMinecraft => {
                if let Some(ref h) = home {
                    out.push(h.join(".minecraft"));
                }
            }
        }
    }
    out
}

pub fn detect_default_launcher_roots() -> Vec<(ExternalLauncherKind, PathBuf)> {
    let mut detected = Vec::new();
    for kind in ExternalLauncherKind::ALL {
        for cand in default_root_candidates_for_kind(kind) {
            if cand.is_dir() {
                detected.push((kind, cand));
                break;
            }
        }
    }
    detected
}

pub fn inspect_external_launcher(
    kind: ExternalLauncherKind,
    root: &Path,
) -> Result<Vec<DiscoveredExternalInstance>> {
    if !root.is_dir() {
        return Err(StorageError::Io {
            path: root.display().to_string(),
            source: io::Error::new(io::ErrorKind::NotFound, "Directory does not exist"),
        });
    }
    match kind {
        ExternalLauncherKind::PrismLauncher => inspect_prism_root(root),
        ExternalLauncherKind::ModrinthApp => inspect_modrinth_app_root(root),
        ExternalLauncherKind::CurseForgeApp => inspect_curseforge_app_root(root),
        ExternalLauncherKind::VanillaMinecraft => inspect_vanilla_root(root),
    }
}

pub fn import_external_instances(
    dest_paths: &LauncherPaths,
    selected: &[DiscoveredExternalInstance],
) -> Result<Vec<String>> {
    dest_paths.initialize()?;
    let store = InstanceStore::new(dest_paths);
    let mut imported_ids = Vec::new();
    for item in selected {
        if !item.mc_dir.is_dir() {
            continue;
        }
        let clean_name = sanitize_instance_name(&item.name);
        let target_name = store.resolve_unique_instance_name(&clean_name)?;
        let loader = ModLoader {
            name: if item.loader_name.trim().is_empty() {
                "vanilla".to_string()
            } else {
                item.loader_name.clone()
            },
            version: if item.loader_name.eq_ignore_ascii_case("vanilla")
                || item.loader_name.trim().is_empty()
            {
                "-1".to_string()
            } else {
                item.loader_version.clone()
            },
        };
        let (created, mut settings) =
            store.create_instance(&target_name, &item.version_id, loader)?;
        let dest_mc = dest_paths.minecraft_dir(&created.instance_id);
        let dest_inst = dest_paths.instance_dir(&created.instance_id);

        copy_instance_game_content(&item.mc_dir, &dest_mc)?;

        if !item.notes.trim().is_empty() {
            let _ = fs::write(dest_inst.join("notes.txt"), &item.notes);
        }

        let mut save_custom = false;
        if let Some(min_m) = item.min_memory.filter(|&v| v >= 256) {
            settings.min_memory = min_m;
            save_custom = true;
        }
        if let Some(max_m) = item.max_memory.filter(|&v| v >= 512) {
            settings.max_memory = max_m.max(settings.min_memory);
            save_custom = true;
        }
        if !item.jvm_args.is_empty() {
            settings.jvm_arguments = item.jvm_args.clone();
            save_custom = true;
        }
        if save_custom {
            let _ = store.save_settings(&created.instance_id, &settings);
        }

        imported_ids.push(created.instance_id);
    }

    Ok(imported_ids)
}

fn copy_instance_game_content(src_mc: &Path, dest_mc: &Path) -> Result<()> {
    const GAME_DIRS: &[&str] = &[
        "mods",
        "config",
        "saves",
        "resourcepacks",
        "shaderpacks",
        "datapacks",
        "screenshots",
        "kubejs",
        "defaultconfigs",
    ];
    const GAME_FILES: &[&str] = &[
        "options.txt",
        "servers.dat",
        "optionsof.txt",
        "optionsshaders.txt",
    ];

    fs::create_dir_all(dest_mc).map_err(|e| StorageError::Io {
        path: dest_mc.display().to_string(),
        source: e,
    })?;

    for sub in GAME_DIRS {
        let src = src_mc.join(sub);
        if src.is_dir() {
            copy_tree(&src, &dest_mc.join(sub)).map_err(|e| StorageError::Io {
                path: src.display().to_string(),
                source: e,
            })?;
        }
    }
    for file in GAME_FILES {
        let src = src_mc.join(file);
        if src.is_file() {
            let dst = dest_mc.join(file);
            fs::copy(&src, &dst).map_err(|e| StorageError::Io {
                path: src.display().to_string(),
                source: e,
            })?;
        }
    }
    Ok(())
}

fn sanitize_instance_name(raw: &str) -> String {
    const INVALID: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || INVALID.contains(&c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let mut trimmed = cleaned
        .trim_matches(|c: char| c.is_whitespace() || c == '.')
        .chars()
        .take(64)
        .collect::<String>();
    while trimmed.ends_with('.') || trimmed.ends_with(' ') {
        trimmed.pop();
    }
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return "Imported Instance".to_string();
    }
    let stem = trimmed.split('.').next().unwrap_or(&trimmed).to_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return format!("Imported {trimmed}");
    }
    trimmed
}

fn count_mods_and_worlds(mc_dir: &Path) -> (usize, usize) {
    let mod_count = fs::read_dir(mc_dir.join("mods"))
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    let p = e.path();
                    if !p.is_file() {
                        return false;
                    }
                    let low = e.file_name().to_string_lossy().to_ascii_lowercase();
                    low.ends_with(".jar") || low.ends_with(".jar.disabled")
                })
                .count()
        })
        .unwrap_or(0);
    let world_count = fs::read_dir(mc_dir.join("saves"))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_dir() && e.path().join("level.dat").is_file())
                .count()
        })
        .unwrap_or(0);
    (mod_count, world_count)
}

fn inspect_prism_root(root: &Path) -> Result<Vec<DiscoveredExternalInstance>> {
    let base = if root.join("instances").is_dir()
        && !root.join("mmc-pack.json").is_file()
        && !root.join("instance.cfg").is_file()
    {
        root.join("instances")
    } else {
        root.to_path_buf()
    };

    let mut dirs = Vec::new();
    if base.join("mmc-pack.json").is_file() || base.join("instance.cfg").is_file() {
        dirs.push(base);
    } else if let Ok(rd) = fs::read_dir(&base) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                dirs.push(e.path());
            }
        }
    }

    let mut out = Vec::new();
    for dir in dirs {
        let pack_json = dir.join("mmc-pack.json");
        let cfg_path = dir.join("instance.cfg");
        if !pack_json.is_file() && !cfg_path.is_file() {
            continue;
        }
        let mc_dir = if dir.join(".minecraft").is_dir() {
            dir.join(".minecraft")
        } else if dir.join("minecraft").is_dir() {
            dir.join("minecraft")
        } else {
            continue;
        };

        let folder_name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("PrismInstance")
            .to_string();
        let (cfg_name, notes, min_m, max_m, jvm_args) = parse_prism_instance_cfg_file(&cfg_path);
        let (mc_ver, loader_name, loader_ver) = parse_mmc_pack_file(&pack_json);
        let name = cfg_name.unwrap_or_else(|| folder_name.clone());
        let (mod_count, world_count) = count_mods_and_worlds(&mc_dir);

        out.push(DiscoveredExternalInstance {
            kind: ExternalLauncherKind::PrismLauncher,
            id_key: format!("prism:{folder_name}"),
            name,
            version_id: mc_ver.unwrap_or_else(|| "1.20.1".to_string()),
            loader_name,
            loader_version: loader_ver,
            mc_dir,
            notes,
            min_memory: min_m,
            max_memory: max_m,
            jvm_args,
            mod_count,
            world_count,
        });
    }
    out.sort_by_key(|a| a.name.to_ascii_lowercase());
    Ok(out)
}

fn inspect_modrinth_app_root(root: &Path) -> Result<Vec<DiscoveredExternalInstance>> {
    let base = if root.join("profiles").is_dir() && !root.join("profile.json").is_file() {
        root.join("profiles")
    } else {
        root.to_path_buf()
    };

    let mut dirs = Vec::new();
    if base.join("profile.json").is_file() {
        dirs.push(base);
    } else if let Ok(rd) = fs::read_dir(&base) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                dirs.push(e.path());
            }
        }
    }

    let mut out = Vec::new();
    for dir in dirs {
        let pjson = dir.join("profile.json");
        if !pjson.is_file() && !dir.join("mods").is_dir() && !dir.join("saves").is_dir() {
            continue;
        }
        let folder_name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("ModrinthProfile")
            .to_string();

        let parsed: Option<Value> = fs::read_to_string(&pjson)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok());
        let name = parsed
            .as_ref()
            .and_then(|v| {
                v.get("metadata")
                    .and_then(|m| m.get("name"))
                    .or_else(|| v.get("name"))
                    .and_then(|s| s.as_str())
            })
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| folder_name.clone());
        let version_id = parsed
            .as_ref()
            .and_then(|v| {
                v.get("metadata")
                    .and_then(|m| m.get("game_version"))
                    .or_else(|| v.get("game_version"))
                    .and_then(|s| s.as_str())
            })
            .unwrap_or("1.20.1")
            .to_string();
        let loader_name = parsed
            .as_ref()
            .and_then(|v| {
                v.get("metadata")
                    .and_then(|m| m.get("loader"))
                    .or_else(|| v.get("loader"))
                    .and_then(|s| s.as_str())
            })
            .map(normalize_loader_tag)
            .unwrap_or_else(|| "vanilla".to_string());
        let loader_version = parsed
            .as_ref()
            .and_then(|v| {
                v.get("metadata")
                    .and_then(|m| m.get("loader_version"))
                    .and_then(|lv| lv.get("id").or(Some(lv)))
                    .or_else(|| v.get("loader_version"))
                    .and_then(|s| s.as_str())
            })
            .unwrap_or("")
            .to_string();

        let (mod_count, world_count) = count_mods_and_worlds(&dir);
        out.push(DiscoveredExternalInstance {
            kind: ExternalLauncherKind::ModrinthApp,
            id_key: format!("modrinth_app:{folder_name}"),
            name,
            version_id,
            loader_name,
            loader_version,
            mc_dir: dir,
            notes: String::new(),
            min_memory: None,
            max_memory: None,
            jvm_args: Vec::new(),
            mod_count,
            world_count,
        });
    }
    out.sort_by_key(|a| a.name.to_ascii_lowercase());
    Ok(out)
}

fn inspect_curseforge_app_root(root: &Path) -> Result<Vec<DiscoveredExternalInstance>> {
    let base = if root.join("minecraft").join("Instances").is_dir() {
        root.join("minecraft").join("Instances")
    } else if root.join("Instances").is_dir() && !root.join("minecraftinstance.json").is_file() {
        root.join("Instances")
    } else {
        root.to_path_buf()
    };

    let mut dirs = Vec::new();
    if base.join("minecraftinstance.json").is_file() {
        dirs.push(base);
    } else if let Ok(rd) = fs::read_dir(&base) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                dirs.push(e.path());
            }
        }
    }

    let mut out = Vec::new();
    for dir in dirs {
        let meta_path = dir.join("minecraftinstance.json");
        if !meta_path.is_file() {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&meta_path) else {
            continue;
        };
        let Ok(val) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let folder_name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("CurseForgeInstance")
            .to_string();
        let name = val
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| folder_name.clone());
        let version_id = val
            .get("gameVersion")
            .or_else(|| val.pointer("/baseModLoader/minecraftVersion"))
            .and_then(|v| v.as_str())
            .unwrap_or("1.20.1")
            .to_string();
        let loader_raw = val
            .pointer("/baseModLoader/name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let (loader_name, loader_version) = parse_cf_modloader_name(loader_raw);
        let max_memory = val
            .get("allocatedMemory")
            .and_then(|v| v.as_u64())
            .map(|m| m as u32)
            .filter(|&m| m >= 512);
        let (mod_count, world_count) = count_mods_and_worlds(&dir);

        out.push(DiscoveredExternalInstance {
            kind: ExternalLauncherKind::CurseForgeApp,
            id_key: format!("cf_app:{folder_name}"),
            name,
            version_id,
            loader_name,
            loader_version,
            mc_dir: dir,
            notes: String::new(),
            min_memory: None,
            max_memory,
            jvm_args: Vec::new(),
            mod_count,
            world_count,
        });
    }
    out.sort_by_key(|a| a.name.to_ascii_lowercase());
    Ok(out)
}

fn inspect_vanilla_root(root: &Path) -> Result<Vec<DiscoveredExternalInstance>> {
    let mc_root =
        if root.join(".minecraft").is_dir() && !root.join("launcher_profiles.json").is_file() {
            root.join(".minecraft")
        } else {
            root.to_path_buf()
        };

    let mut out = Vec::new();
    let profiles_path = mc_root.join("launcher_profiles.json");
    if let Ok(raw) = fs::read_to_string(&profiles_path) {
        if let Ok(val) = serde_json::from_str::<Value>(&raw) {
            if let Some(profiles) = val.get("profiles").and_then(|p| p.as_object()) {
                for (pid, prof) in profiles {
                    let name = prof
                        .get("name")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| pid.clone());
                    let last_ver = prof
                        .get("lastVersionId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("1.20.4");
                    if last_ver == "latest-snapshot" {
                        continue;
                    }
                    let game_dir = prof
                        .get("gameDir")
                        .and_then(|v| v.as_str())
                        .map(PathBuf::from)
                        .filter(|p| p.is_dir())
                        .unwrap_or_else(|| mc_root.clone());
                    let (version_id, loader_name, loader_version) =
                        parse_vanilla_profile_version_id(last_ver);
                    let jvm_args = prof
                        .get("javaArgs")
                        .and_then(|v| v.as_str())
                        .map(|s| s.split_whitespace().map(str::to_string).collect())
                        .unwrap_or_default();
                    let (mod_count, world_count) = count_mods_and_worlds(&game_dir);
                    out.push(DiscoveredExternalInstance {
                        kind: ExternalLauncherKind::VanillaMinecraft,
                        id_key: format!("vanilla:{pid}"),
                        name,
                        version_id,
                        loader_name,
                        loader_version,
                        mc_dir: game_dir,
                        notes: String::new(),
                        min_memory: None,
                        max_memory: None,
                        jvm_args,
                        mod_count,
                        world_count,
                    });
                }
            }
        }
    }

    if out.is_empty() && mc_root.is_dir() {
        let (mod_count, world_count) = count_mods_and_worlds(&mc_root);
        out.push(DiscoveredExternalInstance {
            kind: ExternalLauncherKind::VanillaMinecraft,
            id_key: "vanilla:default".to_string(),
            name: "Vanilla (.minecraft)".to_string(),
            version_id: "1.20.4".to_string(),
            loader_name: "vanilla".to_string(),
            loader_version: "-1".to_string(),
            mc_dir: mc_root,
            notes: String::new(),
            min_memory: None,
            max_memory: None,
            jvm_args: Vec::new(),
            mod_count,
            world_count,
        });
    }

    out.sort_by_key(|a| a.name.to_ascii_lowercase());
    Ok(out)
}

type PrismCfgParsed = (
    Option<String>,
    String,
    Option<u32>,
    Option<u32>,
    Vec<String>,
);

fn parse_prism_instance_cfg_file(path: &Path) -> PrismCfgParsed {
    let Ok(raw) = fs::read_to_string(path) else {
        return (None, String::new(), None, None, Vec::new());
    };
    let mut name: Option<String> = None;
    let mut notes = String::new();
    let mut min_mem: Option<u32> = None;
    let mut max_mem: Option<u32> = None;
    let mut jvm_args: Vec<String> = Vec::new();
    let mut override_mem = true;
    let mut override_jvm = true;

    for line in raw.lines() {
        let line = line.trim();
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let val = v.trim();
        match key {
            "name" if !val.is_empty() => name = Some(val.to_string()),
            "notes" if !val.is_empty() => {
                notes = val.replace("\\n", "\n").replace("\\t", "\t");
            }
            "OverrideMemory" => override_mem = val.eq_ignore_ascii_case("true"),
            "OverrideJavaArgs" => override_jvm = val.eq_ignore_ascii_case("true"),
            "MinMemAlloc" => min_mem = val.parse::<u32>().ok(),
            "MaxMemAlloc" => max_mem = val.parse::<u32>().ok(),
            "JvmArgs" if !val.is_empty() => {
                jvm_args = val.split_whitespace().map(str::to_string).collect();
            }
            _ => {}
        }
    }

    (
        name,
        notes,
        override_mem.then_some(min_mem).flatten(),
        override_mem.then_some(max_mem).flatten(),
        if override_jvm { jvm_args } else { Vec::new() },
    )
}

fn parse_mmc_pack_file(path: &Path) -> (Option<String>, String, String) {
    let Ok(raw) = fs::read_to_string(path) else {
        return (None, "vanilla".to_string(), "-1".to_string());
    };
    let Ok(val) = serde_json::from_str::<Value>(&raw) else {
        return (None, "vanilla".to_string(), "-1".to_string());
    };
    let mut mc_ver: Option<String> = None;
    let mut loader_name = "vanilla".to_string();
    let mut loader_ver = "-1".to_string();

    if let Some(comps) = val.get("components").and_then(|c| c.as_array()) {
        for c in comps {
            let uid = c.get("uid").and_then(|u| u.as_str()).unwrap_or("");
            let ver = c
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            match uid {
                "net.minecraft" if !ver.is_empty() => mc_ver = Some(ver),
                "net.fabricmc.fabric-loader" => {
                    loader_name = "fabric".to_string();
                    loader_ver = ver;
                }
                "org.quiltmc.quilt-loader" => {
                    loader_name = "quilt".to_string();
                    loader_ver = ver;
                }
                "net.minecraftforge" => {
                    loader_name = "forge".to_string();
                    loader_ver = ver;
                }
                "net.neoforged" => {
                    loader_name = "neoforge".to_string();
                    loader_ver = ver;
                }
                _ => {}
            }
        }
    }
    (mc_ver, loader_name, loader_ver)
}

fn normalize_loader_tag(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "fabric" => "fabric".to_string(),
        "quilt" => "quilt".to_string(),
        "forge" => "forge".to_string(),
        "neoforge" => "neoforge".to_string(),
        _ => "vanilla".to_string(),
    }
}

fn parse_cf_modloader_name(raw: &str) -> (String, String) {
    let trimmed = raw.trim();
    if let Some((prefix, ver)) = trimmed.split_once('-') {
        (normalize_loader_tag(prefix), ver.trim().to_string())
    } else if trimmed.is_empty() {
        ("vanilla".to_string(), "-1".to_string())
    } else {
        (normalize_loader_tag(trimmed), String::new())
    }
}

fn parse_vanilla_profile_version_id(last_ver: &str) -> (String, String, String) {
    let clean = last_ver.trim();
    if clean == "latest-release" || clean.is_empty() {
        return (
            "1.20.4".to_string(),
            "vanilla".to_string(),
            "-1".to_string(),
        );
    }
    if let Some(rest) = clean.strip_prefix("fabric-loader-") {
        if let Some((lv, mc)) = rest.split_once('-') {
            return (mc.to_string(), "fabric".to_string(), lv.to_string());
        }
    }
    if let Some(rest) = clean.strip_prefix("quilt-loader-") {
        if let Some((lv, mc)) = rest.split_once('-') {
            return (mc.to_string(), "quilt".to_string(), lv.to_string());
        }
    }
    if let Some((mc, fv)) = clean.split_once("-forge-") {
        return (mc.to_string(), "forge".to_string(), fv.to_string());
    }
    if let Some(nfv) = clean.strip_prefix("neoforge-") {
        return (
            "1.21.1".to_string(),
            "neoforge".to_string(),
            nfv.to_string(),
        );
    }
    (clean.to_string(), "vanilla".to_string(), "-1".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn inspects_and_imports_prism_modrinth_curseforge_and_vanilla_instances() {
        let dir = tempdir().unwrap();

        // 1. Prism Launcher
        let prism_root = dir.path().join("PrismLauncher");
        let prism_inst = prism_root.join("instances").join("FabricSMP");
        fs::create_dir_all(prism_inst.join(".minecraft/mods")).unwrap();
        fs::create_dir_all(prism_inst.join(".minecraft/saves/MyWorld")).unwrap();
        fs::write(prism_inst.join(".minecraft/mods/sodium.jar"), b"mod").unwrap();
        fs::write(
            prism_inst.join(".minecraft/saves/MyWorld/level.dat"),
            b"lvl",
        )
        .unwrap();
        fs::write(
            prism_inst.join("instance.cfg"),
            "name=Prism SMP\nnotes=Line1\\nLine2\nOverrideMemory=true\nMinMemAlloc=1024\nMaxMemAlloc=4096\n",
        )
        .unwrap();
        fs::write(
            prism_inst.join("mmc-pack.json"),
            r#"{"components":[{"uid":"net.minecraft","version":"1.20.4"},{"uid":"net.fabricmc.fabric-loader","version":"0.15.11"}]}"#,
        )
        .unwrap();

        let prism_found =
            inspect_external_launcher(ExternalLauncherKind::PrismLauncher, &prism_root).unwrap();
        assert_eq!(prism_found.len(), 1);
        assert_eq!(prism_found[0].name, "Prism SMP");
        assert_eq!(prism_found[0].version_id, "1.20.4");
        assert_eq!(prism_found[0].loader_name, "fabric");
        assert_eq!(prism_found[0].mod_count, 1);
        assert_eq!(prism_found[0].world_count, 1);

        // 2. Modrinth App
        let mr_root = dir.path().join("ModrinthApp");
        let mr_prof = mr_root.join("profiles").join("BetterVanilla");
        fs::create_dir_all(mr_prof.join("mods")).unwrap();
        fs::write(mr_prof.join("mods/iris.jar"), b"iris").unwrap();
        fs::write(
            mr_prof.join("profile.json"),
            r#"{"metadata":{"name":"Better Vanilla","game_version":"1.21.1","loader":"fabric","loader_version":{"id":"0.16.0"}}}"#,
        )
        .unwrap();
        let mr_found =
            inspect_external_launcher(ExternalLauncherKind::ModrinthApp, &mr_root).unwrap();
        assert_eq!(mr_found.len(), 1);
        assert_eq!(mr_found[0].name, "Better Vanilla");
        assert_eq!(mr_found[0].loader_name, "fabric");

        // 3. CurseForge App
        let cf_root = dir
            .path()
            .join("curseforge")
            .join("minecraft")
            .join("Instances");
        let cf_inst = cf_root.join("ATM9");
        fs::create_dir_all(cf_inst.join("mods")).unwrap();
        fs::write(
            cf_inst.join("minecraftinstance.json"),
            r#"{"name":"ATM9 Local","gameVersion":"1.20.1","allocatedMemory":6144,"baseModLoader":{"name":"forge-47.3.0"}}"#,
        )
        .unwrap();
        let cf_found =
            inspect_external_launcher(ExternalLauncherKind::CurseForgeApp, &cf_root).unwrap();
        assert_eq!(cf_found.len(), 1);
        assert_eq!(cf_found[0].name, "ATM9 Local");
        assert_eq!(cf_found[0].loader_name, "forge");
        assert_eq!(cf_found[0].loader_version, "47.3.0");
        assert_eq!(cf_found[0].max_memory, Some(6144));

        // 4. Vanilla .minecraft
        let vanilla_root = dir.path().join(".minecraft");
        fs::create_dir_all(vanilla_root.join("saves/Survival")).unwrap();
        fs::write(vanilla_root.join("saves/Survival/level.dat"), b"world").unwrap();
        fs::write(
            vanilla_root.join("launcher_profiles.json"),
            r#"{"profiles":{"default":{"name":"My Survival","lastVersionId":"1.21.4"}}}"#,
        )
        .unwrap();
        let vanilla_found =
            inspect_external_launcher(ExternalLauncherKind::VanillaMinecraft, &vanilla_root)
                .unwrap();
        assert_eq!(vanilla_found.len(), 1);
        assert_eq!(vanilla_found[0].name, "My Survival");
        assert_eq!(vanilla_found[0].version_id, "1.21.4");

        // Import all 4 into MCW Launcher storage
        let dest_paths = LauncherPaths::portable(dir.path().join("mcw_dest"));
        let mut all = Vec::new();
        all.extend(prism_found);
        all.extend(mr_found);
        all.extend(cf_found);
        all.extend(vanilla_found);
        let imported = import_external_instances(&dest_paths, &all).unwrap();
        assert_eq!(imported.len(), 4);

        let store = InstanceStore::new(&dest_paths);
        let instances = store.list_instances().unwrap();
        assert_eq!(instances.len(), 4);
        let prism_id = instances
            .iter()
            .find(|i| i.name == "Prism SMP")
            .unwrap()
            .instance_id
            .clone();
        assert!(dest_paths
            .minecraft_dir(&prism_id)
            .join("mods/sodium.jar")
            .is_file());
        assert!(dest_paths
            .minecraft_dir(&prism_id)
            .join("saves/MyWorld/level.dat")
            .is_file());
        let notes =
            fs::read_to_string(dest_paths.instance_dir(&prism_id).join("notes.txt")).unwrap();
        assert_eq!(notes, "Line1\nLine2");
    }
}
