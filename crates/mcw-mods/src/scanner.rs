use mcw_runtime::detect_jar_required_java;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use zip::ZipArchive;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModLoaderTag {
    Fabric,
    Quilt,
    Forge,
    NeoForge,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModInfo {
    pub file_name: String,
    pub path: PathBuf,
    pub enabled: bool,
    pub size_bytes: u64,
    pub mod_id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    pub loader: ModLoaderTag,
    pub required_java_major: u32,
    pub embedded_mod_ids: Vec<String>,
    #[serde(default)]
    pub depends: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedModEntry {
    size_bytes: u64,
    mtime_ns: u128,
    info: ModInfo,
}

pub struct ModScanner;

impl ModScanner {
    /// Scan all `.jar` and `.jar.disabled` files in `mods_dir` in parallel using `rayon`.
    pub fn scan_dir(mods_dir: &Path) -> Vec<ModInfo> {
        let Ok(entries) = fs::read_dir(mods_dir) else {
            return Vec::new();
        };

        let jar_paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                if !p.is_file() {
                    return false;
                }
                let name = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                name.ends_with(".jar") || name.ends_with(".jar.disabled")
            })
            .collect();

        let mut mods: Vec<ModInfo> = jar_paths
            .par_iter()
            .map(|path| Self::inspect_jar(path))
            .collect();

        mods.sort_by(|a, b| {
            b.enabled
                .cmp(&a.enabled)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        mods
    }

    /// Scan `mods_dir` using an on-disk `(size_bytes, mtime_ns)` metadata cache so unchanged JARs
    /// are returned in milliseconds without re-opening ZIP archives.
    pub fn scan_dir_cached(mods_dir: &Path, cache_file: &Path) -> Vec<ModInfo> {
        let Ok(entries) = fs::read_dir(mods_dir) else {
            return Vec::new();
        };

        let prev_cache: HashMap<String, CachedModEntry> = fs::read_to_string(cache_file)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();

        let mut reused: Vec<ModInfo> = Vec::new();
        let mut to_inspect: Vec<(String, u64, u128, PathBuf)> = Vec::new();
        let mut next_cache: HashMap<String, CachedModEntry> = HashMap::new();
        let mut current_count = 0usize;

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(file_name) = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            let lower = file_name.to_ascii_lowercase();
            if !(lower.ends_with(".jar") || lower.ends_with(".jar.disabled")) {
                continue;
            }
            current_count += 1;

            let meta = entry.metadata().ok();
            let size_bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let mtime_ns = meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);

            if mtime_ns > 0 {
                if let Some(hit) = prev_cache.get(&file_name) {
                    if hit.size_bytes == size_bytes
                        && hit.mtime_ns == mtime_ns
                        && hit.info.path == path
                    {
                        next_cache.insert(file_name, hit.clone());
                        reused.push(hit.info.clone());
                        continue;
                    }
                }
            }

            to_inspect.push((file_name, size_bytes, mtime_ns, path));
        }

        let dirty = !to_inspect.is_empty() || current_count != prev_cache.len();
        if !to_inspect.is_empty() {
            let scanned: Vec<(String, u64, u128, ModInfo)> = to_inspect
                .par_iter()
                .map(|(fname, size, mtime, path)| {
                    (fname.clone(), *size, *mtime, Self::inspect_jar(path))
                })
                .collect();
            for (fname, size_bytes, mtime_ns, info) in scanned {
                next_cache.insert(
                    fname,
                    CachedModEntry {
                        size_bytes,
                        mtime_ns,
                        info: info.clone(),
                    },
                );
                reused.push(info);
            }
        }

        if dirty {
            if let Some(parent) = cache_file.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string(&next_cache) {
                let tmp = cache_file.with_extension("json.tmp");
                if fs::write(&tmp, json.as_bytes()).is_ok() {
                    let _ = fs::remove_file(cache_file);
                    let _ = fs::rename(&tmp, cache_file);
                }
            }
        }

        reused.sort_by(|a, b| {
            b.enabled
                .cmp(&a.enabled)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        reused
    }

    pub fn inspect_jar(path: &Path) -> ModInfo {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.jar")
            .to_string();
        let enabled = !file_name.to_ascii_lowercase().ends_with(".disabled");
        let size_bytes = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let fallback_name = file_name
            .trim_end_matches(".disabled")
            .trim_end_matches(".jar")
            .to_string();

        let required_java_major = detect_jar_required_java(path);

        let mut info = ModInfo {
            file_name,
            path: path.to_path_buf(),
            enabled,
            size_bytes,
            mod_id: fallback_name.to_ascii_lowercase(),
            name: fallback_name,
            version: String::new(),
            description: String::new(),
            authors: Vec::new(),
            loader: ModLoaderTag::Unknown,
            required_java_major,
            embedded_mod_ids: Vec::new(),
            depends: Vec::new(),
        };

        let Ok(file) = File::open(path) else {
            return info;
        };
        let Ok(mut archive) = ZipArchive::new(file) else {
            return info;
        };

        // 1. Parse primary mod metadata in priority order: Quilt -> Fabric -> NeoForge -> Forge -> Legacy mcmod.info
        if let Some(raw) = read_zip_text(&mut archive, "quilt.mod.json") {
            apply_quilt_metadata(&mut info, &raw);
        } else if let Some(raw) = read_zip_text(&mut archive, "fabric.mod.json") {
            apply_fabric_metadata(&mut info, &raw);
        } else if let Some(raw) = read_zip_text(&mut archive, "META-INF/neoforge.mods.toml") {
            apply_toml_metadata(&mut info, &raw, ModLoaderTag::NeoForge);
        } else if let Some(raw) = read_zip_text(&mut archive, "META-INF/mods.toml") {
            apply_toml_metadata(&mut info, &raw, ModLoaderTag::Forge);
        } else if let Some(raw) = read_zip_text(&mut archive, "mcmod.info") {
            apply_mcmod_info(&mut info, &raw);
        }

        // 2. Inspect Jar-in-Jar (`META-INF/jars/*.jar` and `META-INF/jarjar/*.jar`)
        info.embedded_mod_ids = extract_embedded_mod_ids(&mut archive);

        info
    }
}

/// Check if a mod's loader tag is compatible with an instance's active loader.
/// Notably, Quilt supports Fabric mods (Fabric-on-Quilt).
pub fn is_loader_compatible(mod_loader: ModLoaderTag, instance_loader: &str) -> bool {
    let target = instance_loader.trim().to_ascii_lowercase();
    matches!(
        (mod_loader, target.as_str()),
        (ModLoaderTag::Unknown, _)
            | (ModLoaderTag::Fabric, "fabric" | "quilt")
            | (ModLoaderTag::Quilt, "quilt")
            | (ModLoaderTag::Forge, "forge")
            | (ModLoaderTag::NeoForge, "neoforge")
    )
}

fn read_zip_text<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Option<String> {
    let mut entry = archive.by_name(name).ok()?;
    let mut buf = String::new();
    entry.read_to_string(&mut buf).ok()?;
    Some(buf)
}

fn apply_fabric_metadata(info: &mut ModInfo, raw: &str) {
    info.loader = ModLoaderTag::Fabric;
    let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) else {
        return;
    };
    if let Some(id) = val.get("id").and_then(|v| v.as_str()) {
        if !id.trim().is_empty() {
            info.mod_id = id.trim().to_string();
        }
    }
    if let Some(name) = val.get("name").and_then(|v| v.as_str()) {
        if !name.trim().is_empty() {
            info.name = name.trim().to_string();
        }
    }
    if let Some(ver) = val.get("version").and_then(|v| v.as_str()) {
        info.version = ver.trim().to_string();
    }
    if let Some(desc) = val.get("description").and_then(|v| v.as_str()) {
        info.description = desc.trim().to_string();
    }
    if let Some(authors) = val.get("authors").and_then(|v| v.as_array()) {
        info.authors = authors
            .iter()
            .filter_map(|a| {
                a.as_str()
                    .map(String::from)
                    .or_else(|| a.get("name").and_then(|n| n.as_str()).map(String::from))
            })
            .collect();
    }
    if let Some(deps) = val.get("depends").and_then(|v| v.as_object()) {
        info.depends = deps
            .keys()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .collect();
    }
}

fn apply_quilt_metadata(info: &mut ModInfo, raw: &str) {
    info.loader = ModLoaderTag::Quilt;
    let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) else {
        return;
    };
    let Some(ql) = val.get("quilt_loader") else {
        return;
    };
    if let Some(id) = ql.get("id").and_then(|v| v.as_str()) {
        if !id.trim().is_empty() {
            info.mod_id = id.trim().to_string();
        }
    }
    if let Some(ver) = ql.get("version").and_then(|v| v.as_str()) {
        info.version = ver.trim().to_string();
    }
    if let Some(meta) = ql.get("metadata") {
        if let Some(name) = meta.get("name").and_then(|v| v.as_str()) {
            if !name.trim().is_empty() {
                info.name = name.trim().to_string();
            }
        }
        if let Some(desc) = meta.get("description").and_then(|v| v.as_str()) {
            info.description = desc.trim().to_string();
        }
        if let Some(contribs) = meta.get("contributors").and_then(|v| v.as_object()) {
            info.authors = contribs.keys().cloned().collect();
        }
    }
    if let Some(deps) = ql.get("depends").and_then(|v| v.as_array()) {
        for dep in deps {
            if let Some(id) = dep.as_str() {
                if !id.trim().is_empty() {
                    info.depends.push(id.trim().to_string());
                }
            } else if let Some(obj) = dep.as_object() {
                if obj.get("optional").and_then(|v| v.as_bool()) == Some(true) {
                    continue;
                }
                if let Some(id) = obj.get("id").and_then(|v| v.as_str()) {
                    if !id.trim().is_empty() {
                        info.depends.push(id.trim().to_string());
                    }
                }
            }
        }
    }
}

fn apply_toml_metadata(info: &mut ModInfo, raw: &str, loader: ModLoaderTag) {
    info.loader = loader;
    let Ok(val) = toml::from_str::<toml::Value>(raw) else {
        return;
    };
    let Some(first_mod) = val
        .get("mods")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
    else {
        return;
    };
    if let Some(id) = first_mod.get("modId").and_then(|v| v.as_str()) {
        if !id.trim().is_empty() {
            info.mod_id = id.trim().to_string();
        }
    }
    if let Some(name) = first_mod.get("displayName").and_then(|v| v.as_str()) {
        if !name.trim().is_empty() {
            info.name = name.trim().to_string();
        }
    }
    if let Some(ver) = first_mod.get("version").and_then(|v| v.as_str()) {
        info.version = ver.trim().to_string();
    }
    if let Some(desc) = first_mod.get("description").and_then(|v| v.as_str()) {
        info.description = desc.trim().to_string();
    }
    if let Some(authors) = first_mod.get("authors").and_then(|v| v.as_str()) {
        info.authors = authors
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(deps_table) = val.get("dependencies").and_then(|d| d.as_table()) {
        for dep_list in deps_table.values() {
            let Some(arr) = dep_list.as_array() else {
                continue;
            };
            for dep in arr {
                let is_required = dep.get("mandatory").and_then(|v| v.as_bool()) == Some(true)
                    || dep
                        .get("type")
                        .and_then(|v| v.as_str())
                        .is_some_and(|t| t.eq_ignore_ascii_case("required"));
                if !is_required {
                    continue;
                }
                if let Some(id) = dep.get("modId").and_then(|v| v.as_str()) {
                    let clean = id.trim();
                    if !clean.is_empty() && !info.depends.iter().any(|d| d == clean) {
                        info.depends.push(clean.to_string());
                    }
                }
            }
        }
    }
}

fn apply_mcmod_info(info: &mut ModInfo, raw: &str) {
    info.loader = ModLoaderTag::Forge;
    let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) else {
        return;
    };
    let entry = val.as_array().and_then(|a| a.first()).or_else(|| {
        val.get("modList")
            .and_then(|m| m.as_array())
            .and_then(|a| a.first())
    });
    let Some(first) = entry else {
        return;
    };
    if let Some(id) = first.get("modid").and_then(|v| v.as_str()) {
        if !id.trim().is_empty() {
            info.mod_id = id.trim().to_string();
        }
    }
    if let Some(name) = first.get("name").and_then(|v| v.as_str()) {
        if !name.trim().is_empty() {
            info.name = name.trim().to_string();
        }
    }
    if let Some(ver) = first.get("version").and_then(|v| v.as_str()) {
        info.version = ver.trim().to_string();
    }
    if let Some(desc) = first.get("description").and_then(|v| v.as_str()) {
        info.description = desc.trim().to_string();
    }
}

fn extract_embedded_mod_ids<R: Read + std::io::Seek>(archive: &mut ZipArchive<R>) -> Vec<String> {
    let nested_names: Vec<String> = (0..archive.len())
        .filter_map(|i| {
            let entry = archive.by_index(i).ok()?;
            let name = entry.name().to_string();
            let lower = name.to_ascii_lowercase();
            if (lower.starts_with("meta-inf/jars/") || lower.starts_with("meta-inf/jarjar/"))
                && lower.ends_with(".jar")
            {
                Some(name)
            } else {
                None
            }
        })
        .collect();

    let mut embedded = Vec::new();
    for nested in nested_names {
        let Ok(mut entry) = archive.by_name(&nested) else {
            continue;
        };
        let mut bytes = Vec::new();
        if entry.read_to_end(&mut bytes).is_err() {
            continue;
        }
        let Ok(mut inner) = ZipArchive::new(Cursor::new(bytes)) else {
            continue;
        };
        if let Some(raw) = read_zip_text(&mut inner, "fabric.mod.json") {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(id) = val.get("id").and_then(|v| v.as_str()) {
                    embedded.push(id.to_string());
                }
            }
        } else if let Some(raw) = read_zip_text(&mut inner, "quilt.mod.json") {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(id) = val.pointer("/quilt_loader/id").and_then(|v| v.as_str()) {
                    embedded.push(id.to_string());
                }
            }
        }
    }
    embedded.sort();
    embedded.dedup();
    embedded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    #[test]
    fn scans_fabric_and_jij_and_forge_mods_in_parallel() {
        let dir = tempdir().unwrap();

        // 1. Fabric mod with embedded Jar-in-Jar
        let mut inner_buf = Cursor::new(Vec::new());
        {
            let mut inner_zip = ZipWriter::new(&mut inner_buf);
            inner_zip
                .start_file("fabric.mod.json", SimpleFileOptions::default())
                .unwrap();
            inner_zip
                .write_all(br#"{"id":"cloth-config","name":"Cloth Config","version":"15.0"}"#)
                .unwrap();
            inner_zip.finish().unwrap();
        }

        let fabric_jar = dir.path().join("sodium.jar");
        {
            let mut zip = ZipWriter::new(File::create(&fabric_jar).unwrap());
            zip.start_file("fabric.mod.json", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(
                br#"{"id":"sodium","name":"Sodium","version":"0.6.0","authors":["jellysquid3"]}"#,
            )
            .unwrap();
            zip.start_file(
                "META-INF/jars/cloth-config.jar",
                SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(&inner_buf.into_inner()).unwrap();
            zip.finish().unwrap();
        }

        // 2. Disabled Forge mod
        let forge_jar = dir.path().join("jei.jar.disabled");
        {
            let mut zip = ZipWriter::new(File::create(&forge_jar).unwrap());
            zip.start_file("META-INF/mods.toml", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(
                b"[[mods]]\nmodId=\"jei\"\ndisplayName=\"Just Enough Items\"\nversion=\"19.0\"\n",
            )
            .unwrap();
            zip.finish().unwrap();
        }

        let mods = ModScanner::scan_dir(dir.path());
        assert_eq!(mods.len(), 2);
        assert_eq!(mods[0].mod_id, "sodium");
        assert!(mods[0].enabled);
        assert_eq!(mods[0].loader, ModLoaderTag::Fabric);
        assert_eq!(mods[0].embedded_mod_ids, vec!["cloth-config".to_string()]);
        assert!(is_loader_compatible(mods[0].loader, "quilt"));

        assert_eq!(mods[1].mod_id, "jei");
        assert!(!mods[1].enabled);
        assert_eq!(mods[1].loader, ModLoaderTag::Forge);
        assert!(!is_loader_compatible(mods[1].loader, "fabric"));

        // Verify persistent cache hit preserves parsed ModInfo even without re-reading zip
        let cache_file = dir.path().join("mods_cache.json");
        let cached_1 = ModScanner::scan_dir_cached(dir.path(), &cache_file);
        assert_eq!(cached_1.len(), 2);
        assert!(cache_file.is_file());
        let cached_2 = ModScanner::scan_dir_cached(dir.path(), &cache_file);
        assert_eq!(cached_2.len(), 2);
        assert_eq!(cached_2[0].mod_id, "sodium");
    }
}
