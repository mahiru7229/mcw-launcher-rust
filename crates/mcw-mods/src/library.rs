use crate::content::{ContentKind, ContentPackManager};
use crate::manager::ModManager;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstalledContentKind {
    Mod,
    ResourcePack,
    ShaderPack,
    Modpack,
}

impl InstalledContentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::ResourcePack => "resourcepack",
            Self::ShaderPack => "shaderpack",
            Self::Modpack => "modpack",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledContentItem {
    pub item_id: String,
    pub kind: InstalledContentKind,
    pub name: String,
    pub version: String,
    pub provider: String,
    pub file_name: String,
    pub target_path: String,
    pub enabled: bool,
    pub size_bytes: u64,
    pub status: String,
    pub pinned: bool,
    pub ignored_update: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContentItemFlags {
    #[serde(default)]
    pub pinned: bool,
    #[serde(default, rename = "ignoredUpdate")]
    pub ignored_update: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContentLibraryPrefs {
    #[serde(default)]
    pub items: HashMap<String, ContentItemFlags>,
}

/// Port of Python `InstalledContentLibraryManager` (`src/core/content/installed_content_library.py`)
/// and `ContentLibraryPreferences`.
pub struct InstalledContentLibrary;

impl InstalledContentLibrary {
    fn prefs_path(instance_dir: &Path) -> std::path::PathBuf {
        instance_dir.join(".mcw").join("content-library-prefs.json")
    }

    pub fn load_prefs(instance_dir: &Path) -> ContentLibraryPrefs {
        let path = Self::prefs_path(instance_dir);
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save_prefs(instance_dir: &Path, prefs: &ContentLibraryPrefs) -> Result<()> {
        let path = Self::prefs_path(instance_dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(prefs).unwrap_or_else(|_| "{}".to_string());
        let tmp = path.with_extension("json.part");
        fs::write(&tmp, format!("{json}\n"))?;
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn set_flags(
        instance_dir: &Path,
        item_id: &str,
        pinned: Option<bool>,
        ignored_update: Option<bool>,
    ) -> Result<()> {
        let mut prefs = Self::load_prefs(instance_dir);
        let entry = prefs.items.entry(item_id.to_string()).or_default();
        if let Some(p) = pinned {
            entry.pinned = p;
        }
        if let Some(ig) = ignored_update {
            entry.ignored_update = ig;
        }
        Self::save_prefs(instance_dir, &prefs)
    }

    pub fn scan(instance_dir: &Path, instance_name: &str) -> Vec<InstalledContentItem> {
        let _ = instance_name;
        let mc_dir = instance_dir.join(".minecraft");
        let prefs = Self::load_prefs(instance_dir);
        let provenance = Self::load_mod_provenance_providers(instance_dir);
        let mut items = Vec::new();

        // 1. Mods
        let mods_dir = mc_dir.join("mods");
        for m in ModManager::new(&mods_dir).list() {
            let key = m.file_name.to_ascii_lowercase();
            let provider = provenance
                .get(&key)
                .cloned()
                .unwrap_or_else(|| "local".to_string());
            let item_id = format!("mod:{provider}:{key}");
            let flags = prefs.items.get(&item_id);
            let status = if m.enabled { "ready" } else { "disabled" }.to_string();
            items.push(InstalledContentItem {
                item_id,
                kind: InstalledContentKind::Mod,
                name: if m.name.trim().is_empty() {
                    m.file_name.clone()
                } else {
                    m.name
                },
                version: m.version,
                provider,
                file_name: m.file_name.clone(),
                target_path: format!("mods/{}", m.file_name),
                enabled: m.enabled,
                size_bytes: m.size_bytes,
                status,
                pinned: flags.map(|f| f.pinned).unwrap_or(false),
                ignored_update: flags.map(|f| f.ignored_update).unwrap_or(false),
            });
        }

        // 2. Resource Packs & Shader Packs
        for (kind, out_kind, folder) in [
            (
                ContentKind::ResourcePack,
                InstalledContentKind::ResourcePack,
                "resourcepacks",
            ),
            (
                ContentKind::ShaderPack,
                InstalledContentKind::ShaderPack,
                "shaderpacks",
            ),
        ] {
            for p in ContentPackManager::list(&mc_dir, kind) {
                let item_id = format!(
                    "{}:{}:{}",
                    out_kind.as_str(),
                    p.file_name.to_ascii_lowercase(),
                    p.size_bytes
                );
                let flags = prefs.items.get(&item_id);
                let status = if p.enabled { "ready" } else { "disabled" }.to_string();
                items.push(InstalledContentItem {
                    item_id,
                    kind: out_kind,
                    name: p.display_name,
                    version: String::new(),
                    provider: "local".to_string(),
                    file_name: p.file_name.clone(),
                    target_path: format!("{folder}/{}", p.file_name),
                    enabled: p.enabled,
                    size_bytes: p.size_bytes,
                    status,
                    pinned: flags.map(|f| f.pinned).unwrap_or(false),
                    ignored_update: flags.map(|f| f.ignored_update).unwrap_or(false),
                });
            }
        }

        // 3. Modpack registries stored under `.mcw/`
        for (provider, filename) in [
            ("ftb", "ftb-pack.json"),
            ("atlauncher", "atlauncher-pack.json"),
        ] {
            let reg_path = instance_dir.join(".mcw").join(filename);
            if let Ok(raw) = fs::read_to_string(&reg_path) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                    let pack_id = val
                        .get("packId")
                        .or_else(|| val.get("safeName"))
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .or_else(|| {
                            val.get("packId")
                                .and_then(|v| v.as_u64())
                                .map(|n| n.to_string())
                        })
                        .unwrap_or_else(|| provider.to_string());
                    let name = val
                        .get("packName")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or(&pack_id)
                        .to_string();
                    let version = val
                        .get("versionName")
                        .or_else(|| val.get("versionId"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let item_id = format!("modpack:{provider}:{pack_id}");
                    let flags = prefs.items.get(&item_id);
                    items.push(InstalledContentItem {
                        item_id,
                        kind: InstalledContentKind::Modpack,
                        name,
                        version,
                        provider: provider.to_string(),
                        file_name: filename.to_string(),
                        target_path: format!(".mcw/{filename}"),
                        enabled: true,
                        size_bytes: fs::metadata(&reg_path).map(|m| m.len()).unwrap_or(0),
                        status: "ready".to_string(),
                        pinned: flags.map(|f| f.pinned).unwrap_or(false),
                        ignored_update: flags.map(|f| f.ignored_update).unwrap_or(false),
                    });
                }
            }
        }

        items.sort_by(|a, b| {
            a.kind.cmp(&b.kind).then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
        });
        items
    }

    fn load_mod_provenance_providers(instance_dir: &Path) -> HashMap<String, String> {
        let path = instance_dir.join(".mcw").join("mod-provenance.json");
        let mut map = HashMap::new();
        let Ok(raw) = fs::read_to_string(&path) else {
            return map;
        };
        let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return map;
        };
        if let Some(entries) = val.get("files").and_then(|v| v.as_array()) {
            for e in entries {
                let fname = e
                    .get("fileName")
                    .or_else(|| e.get("file_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let provider = e
                    .get("provider")
                    .and_then(|v| v.as_str())
                    .unwrap_or("local")
                    .to_string();
                if !fname.is_empty() {
                    map.insert(fname, provider);
                }
            }
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    #[test]
    fn scans_unified_library_and_persists_pin_flags() {
        let dir = tempdir().unwrap();
        let inst_dir = dir.path().join("demo");
        let mc_dir = inst_dir.join(".minecraft");
        fs::create_dir_all(mc_dir.join("mods")).unwrap();
        fs::create_dir_all(mc_dir.join("resourcepacks")).unwrap();
        fs::create_dir_all(inst_dir.join(".mcw")).unwrap();

        let mod_path = mc_dir.join("mods").join("demo-mod.jar");
        {
            let f = fs::File::create(&mod_path).unwrap();
            let mut zw = zip::ZipWriter::new(f);
            zw.start_file("fabric.mod.json", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(br#"{"id":"demo","name":"Demo Mod","version":"1.0.0"}"#)
                .unwrap();
            zw.finish().unwrap();
        }
        fs::write(mc_dir.join("resourcepacks").join("Faithful.zip"), b"zip").unwrap();
        fs::write(
            inst_dir.join(".mcw").join("ftb-pack.json"),
            r#"{"packId":"81","packName":"FTB Evolution","versionName":"1.2.0"}"#,
        )
        .unwrap();

        let items = InstalledContentLibrary::scan(&inst_dir, "demo");
        assert_eq!(items.len(), 3);
        let mod_item = items
            .iter()
            .find(|i| i.kind == InstalledContentKind::Mod)
            .unwrap();
        assert_eq!(mod_item.name, "Demo Mod");
        assert!(!mod_item.pinned);

        InstalledContentLibrary::set_flags(&inst_dir, &mod_item.item_id, Some(true), Some(true))
            .unwrap();
        let items2 = InstalledContentLibrary::scan(&inst_dir, "demo");
        let mod_item2 = items2
            .iter()
            .find(|i| i.kind == InstalledContentKind::Mod)
            .unwrap();
        assert!(mod_item2.pinned);
        assert!(mod_item2.ignored_update);
    }
}
