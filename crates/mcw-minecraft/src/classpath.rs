use crate::rules::LibraryRules;
use mcw_models::minecraft::{LibraryEntry, VersionMetadata};
use mcw_storage::LauncherPaths;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct ClasspathBuilder;

impl ClasspathBuilder {
    /// Build platform-joined classpath, deduplicating libraries by Maven `(group, artifact, classifier)`
    /// (keeping the latest entry) and appending `client_jar` at the end.
    pub fn build(
        version: &VersionMetadata,
        client_jar: &Path,
        libraries_dir: &Path,
    ) -> String {
        let mut entries: Vec<String> = Vec::new();
        let mut key_indices: HashMap<(String, String, String), usize> = HashMap::new();

        for lib in &version.libraries {
            if !LibraryRules::is_allowed(&lib.rules) || lib.clientreq == Some(false) {
                continue;
            }
            let Some(rel_path) = Self::artifact_path(lib, libraries_dir) else {
                continue;
            };
            let full_path = libraries_dir.join(rel_path).to_string_lossy().to_string();

            if let Some(key) = Self::maven_key(&lib.name) {
                if let Some(&idx) = key_indices.get(&key) {
                    entries[idx] = full_path;
                } else {
                    key_indices.insert(key, entries.len());
                    entries.push(full_path);
                }
            } else {
                entries.push(full_path);
            }
        }

        entries.push(client_jar.to_string_lossy().to_string());
        let sep = if cfg!(windows) { ";" } else { ":" };
        entries.join(sep)
    }

    fn maven_key(name: &str) -> Option<(String, String, String)> {
        let raw = name.trim();
        if raw.is_empty() {
            return None;
        }
        let without_ext = raw.rsplit_once('@').map(|(left, _)| left).unwrap_or(raw);
        let parts: Vec<&str> = without_ext.split(':').map(str::trim).collect();
        if parts.len() < 3 || parts[0].is_empty() || parts[1].is_empty() {
            return None;
        }
        let classifier = parts.get(3).copied().unwrap_or("").to_string();
        Some((parts[0].to_string(), parts[1].to_string(), classifier))
    }

    fn artifact_path(lib: &LibraryEntry, libraries_dir: &Path) -> Option<PathBuf> {
        if let Some(ref downloads) = lib.downloads {
            if let Some(ref artifact) = downloads.artifact {
                if let Some(ref configured) = artifact.path {
                    let trimmed = configured.trim();
                    if !trimmed.is_empty() {
                        return LauncherPaths::safe_join_relative(Path::new(""), trimmed).ok();
                    }
                }
            }
        }
        let legacy = Self::legacy_maven_path(&lib.name)?;
        if libraries_dir.join(&legacy).is_file() {
            Some(legacy)
        } else {
            None
        }
    }

    fn legacy_maven_path(coordinate: &str) -> Option<PathBuf> {
        let raw = coordinate.trim();
        let (coord, ext) = raw
            .rsplit_once('@')
            .map(|(l, r)| (l, if r.trim().is_empty() { "jar" } else { r.trim() }))
            .unwrap_or((raw, "jar"));
        let parts: Vec<&str> = coord.split(':').map(str::trim).collect();
        if parts.len() < 3 {
            return None;
        }
        let (group, artifact, version) = (parts[0], parts[1], parts[2]);
        let classifier = parts.get(3).copied().unwrap_or("");
        if group.is_empty() || artifact.is_empty() || version.is_empty() {
            return None;
        }
        let filename = if classifier.is_empty() {
            format!("{artifact}-{version}.{ext}")
        } else {
            format!("{artifact}-{version}-{classifier}.{ext}")
        };
        let mut rel = PathBuf::new();
        for seg in group.split('.') {
            rel.push(seg);
        }
        rel.push(artifact);
        rel.push(version);
        rel.push(filename);
        Some(rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcw_models::minecraft::{DownloadArtifact, LibraryDownloads};

    #[test]
    fn deduplicates_maven_libraries_keeping_latest() {
        let meta = VersionMetadata {
            id: "1.21.4".to_string(),
            version_type: "release".to_string(),
            main_class: "net.minecraft.client.main.Main".to_string(),
            inherits_from: None,
            asset_index: None,
            assets: None,
            downloads: None,
            java_version: None,
            arguments: None,
            minecraft_arguments: None,
            libraries: vec![
                LibraryEntry {
                    name: "org.ow2.asm:asm:9.6".to_string(),
                    downloads: Some(LibraryDownloads {
                        artifact: Some(DownloadArtifact {
                            path: Some("org/ow2/asm/asm/9.6/asm-9.6.jar".to_string()),
                            sha1: None,
                            size: 0,
                            url: String::new(),
                        }),
                        classifiers: HashMap::new(),
                    }),
                    rules: vec![],
                    natives: None,
                    clientreq: None,
                },
                LibraryEntry {
                    name: "org.ow2.asm:asm:9.7.1".to_string(),
                    downloads: Some(LibraryDownloads {
                        artifact: Some(DownloadArtifact {
                            path: Some("org/ow2/asm/asm/9.7.1/asm-9.7.1.jar".to_string()),
                            sha1: None,
                            size: 0,
                            url: String::new(),
                        }),
                        classifiers: HashMap::new(),
                    }),
                    rules: vec![],
                    natives: None,
                    clientreq: None,
                },
            ],
        };

        let cp = ClasspathBuilder::build(&meta, Path::new("/mc/client.jar"), Path::new("/mc/libs"));
        assert!(!cp.contains("asm-9.6.jar"));
        assert!(cp.contains("asm-9.7.1.jar"));
        assert!(cp.ends_with("client.jar"));
    }
}
