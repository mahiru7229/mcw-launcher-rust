use mcw_runtime::detect_jar_required_java;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
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
    }
}
