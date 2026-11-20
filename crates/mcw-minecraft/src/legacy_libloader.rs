use crate::{MinecraftError, Result};
use mcw_net::{DownloadClient, DownloadRequest};
use sha2::{Digest, Sha512};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use tracing::info;
use url::Url;
use zip::ZipArchive;

const ALLOWED_SOURCE_HOSTS: [&str; 3] = [
    "jcenter.bintray.com",
    "repo1.maven.org",
    "repo.maven.apache.org",
];
const TACKPROFILER_BASE: &str = "https://raw.githubusercontent.com/quat1024/TackProfiler/trunk/src/main/resources/cached-libloader-libs";
const MAX_LIBRARY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ROUNDS: usize = 8;

#[derive(Debug, Clone)]
pub struct LegacyLibLoaderDependency {
    pub group: String,
    pub name: String,
    pub version: String,
    pub sha512: String,
    pub source_jar: PathBuf,
    pub classifier: String,
    pub url: String,
    pub embedded_file: String,
    pub build_time: i64,
}

impl LegacyLibLoaderDependency {
    pub fn key(&self) -> String {
        format!("{}.{}", self.group, self.name)
    }

    pub fn display_name(&self) -> String {
        let clf = if self.classifier.is_empty() {
            String::new()
        } else {
            format!("-{}", self.classifier)
        };
        format!("{}.{}{}-{}", self.group, self.name, clf, self.version)
    }

    pub fn mirror_filename(&self) -> String {
        let clf = if self.classifier.is_empty() {
            String::new()
        } else {
            format!("-{}", self.classifier)
        };
        format!("{}.{}-{}{}.jar", self.group, self.name, self.version, clf)
    }

    pub fn relative_path(&self) -> PathBuf {
        let clf = if self.classifier.is_empty() {
            String::new()
        } else {
            format!("-{}", self.classifier)
        };
        let filename = format!("{}-{}{}.jar", self.name, self.version, clf);
        let mut p = PathBuf::new();
        for seg in self.group.split('.') {
            if !seg.is_empty() && seg != ".." {
                p.push(seg);
            }
        }
        let dir_name = if self.version.contains('-') {
            let short_hash: String = self.sha512.chars().take(16).collect();
            format!("{}-{}-{short_hash}", self.name, self.version)
        } else {
            format!("{}-{}", self.name, self.version)
        };
        p.push(dir_name);
        p.push(filename);
        p
    }
}

/// Port of Python `LegacyLibLoaderManager`
/// (`src/core/modloader/forge/legacy_libloader_manager.py`).
///
/// Recovers embedded and Maven-hosted transitive `libloader-*` dependencies declared in
/// legacy Forge (pre-1.12.2 / 1.12.2) mod JAR `META-INF/MANIFEST.MF` files, verified with SHA-512.
pub struct LegacyLibLoaderManager;

impl LegacyLibLoaderManager {
    pub async fn ensure(
        mc_dir: &Path,
        loader_name: &str,
        client: &DownloadClient,
    ) -> Result<Vec<String>> {
        if !loader_name.trim().eq_ignore_ascii_case("forge") {
            return Ok(Vec::new());
        }
        let mods_dir = mc_dir.join("mods");
        if !mods_dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut sources = Vec::new();
        if let Ok(entries) = fs::read_dir(&mods_dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_file()
                    && p.extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
                {
                    sources.push(p);
                }
            }
        }
        sources.sort();

        let mut dependencies: HashMap<String, LegacyLibLoaderDependency> =
            Self::collect_dependencies(&sources)
                .into_iter()
                .filter(|(_, d)| Self::manageable(d))
                .collect();
        if dependencies.is_empty() {
            return Ok(Vec::new());
        }

        let libraries_dir = mc_dir.join("libraries");
        let mut recovered_count = 0usize;
        let mut processed_sources: HashSet<PathBuf> = sources
            .iter()
            .map(|p| fs::canonicalize(p).unwrap_or_else(|_| p.clone()))
            .collect();

        for _round in 0..MAX_ROUNDS {
            let mut pending: Vec<LegacyLibLoaderDependency> = dependencies
                .values()
                .filter(|dep| {
                    !Self::is_valid_sha512(&libraries_dir.join(dep.relative_path()), &dep.sha512)
                })
                .cloned()
                .collect();
            if pending.is_empty() {
                break;
            }
            pending.sort_by_key(|a| a.display_name().to_ascii_lowercase());

            let mut new_sources = Vec::new();
            for dep in pending {
                let dest = libraries_dir.join(dep.relative_path());
                Self::acquire(&dep, &dest, client).await?;
                recovered_count += 1;
                let canon = fs::canonicalize(&dest).unwrap_or_else(|_| dest.clone());
                if processed_sources.insert(canon) {
                    new_sources.push(dest);
                }
            }
            if new_sources.is_empty() {
                break;
            }
            for (k, dep) in Self::collect_dependencies(&new_sources) {
                if !Self::manageable(&dep) {
                    continue;
                }
                let replace = dependencies
                    .get(&k)
                    .is_none_or(|cur| dep.build_time > cur.build_time);
                if replace {
                    dependencies.insert(k, dep);
                }
            }
        }

        let unresolved: Vec<String> = dependencies
            .values()
            .filter(|d| !Self::is_valid_sha512(&libraries_dir.join(d.relative_path()), &d.sha512))
            .map(LegacyLibLoaderDependency::display_name)
            .collect();
        if !unresolved.is_empty() {
            return Err(MinecraftError::InvalidMetadata(format!(
                "Could not prepare legacy LibLoader dependencies: {}",
                unresolved.join(", ")
            )));
        }

        if recovered_count > 0 {
            info!(
                count = recovered_count,
                "Recovered legacy Forge LibLoader dependencies"
            );
            Ok(vec![format!(
                "Recovered {recovered_count} legacy Forge mod library file(s)."
            )])
        } else {
            Ok(Vec::new())
        }
    }

    pub fn parse_manifest_attributes(payload: &str) -> HashMap<String, String> {
        let normalized = payload.replace("\r\n", "\n").replace('\r', "\n");
        let mut logical_lines: Vec<String> = Vec::new();
        for line in normalized.lines() {
            if let Some(cont) = line.strip_prefix(' ') {
                if let Some(last) = logical_lines.last_mut() {
                    last.push_str(cont);
                }
            } else {
                logical_lines.push(line.to_string());
            }
        }
        let mut map = HashMap::new();
        for line in logical_lines {
            if let Some((k, v)) = line.split_once(':') {
                map.insert(k.trim().to_ascii_lowercase(), v.trim_start().to_string());
            }
        }
        map
    }

    fn collect_dependencies(sources: &[PathBuf]) -> HashMap<String, LegacyLibLoaderDependency> {
        let mut selected: HashMap<String, LegacyLibLoaderDependency> = HashMap::new();
        for source in sources {
            let Ok(attrs) = Self::read_manifest(source) else {
                continue;
            };
            for i in 0..256usize {
                let prefix = "libloader-";
                let group = attrs
                    .get(&format!("{prefix}group{i}"))
                    .map(String::as_str)
                    .unwrap_or("")
                    .trim();
                if group.is_empty() {
                    break;
                }
                let sha512 = attrs
                    .get(&format!("{prefix}sha512hash{i}"))
                    .map(String::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase();
                if sha512.len() != 128 || !sha512.bytes().all(|b| b.is_ascii_hexdigit()) {
                    continue;
                }
                let name = attrs
                    .get(&format!("{prefix}name{i}"))
                    .map(String::as_str)
                    .unwrap_or("")
                    .trim();
                let version = attrs
                    .get(&format!("{prefix}version{i}"))
                    .map(String::as_str)
                    .unwrap_or("")
                    .trim();
                if name.is_empty() || version.is_empty() {
                    continue;
                }
                let build_time = attrs
                    .get(&format!("{prefix}buildtime{i}"))
                    .and_then(|s| s.trim().parse::<i64>().ok())
                    .unwrap_or(0);
                let dep = LegacyLibLoaderDependency {
                    group: group.to_string(),
                    name: name.to_string(),
                    version: version.to_string(),
                    sha512,
                    source_jar: source.clone(),
                    classifier: attrs
                        .get(&format!("{prefix}classifier{i}"))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default(),
                    url: attrs
                        .get(&format!("{prefix}url{i}"))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default(),
                    embedded_file: attrs
                        .get(&format!("{prefix}file{i}"))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default(),
                    build_time,
                };
                let key = dep.key();
                if selected
                    .get(&key)
                    .is_none_or(|cur| dep.build_time > cur.build_time)
                {
                    selected.insert(key, dep);
                }
            }
        }
        selected
    }

    fn read_manifest(jar: &Path) -> Result<HashMap<String, String>> {
        let f = File::open(jar)?;
        let mut zip = ZipArchive::new(f)?;
        let mut entry = zip.by_name("META-INF/MANIFEST.MF")?;
        let mut content = String::new();
        entry.read_to_string(&mut content)?;
        Ok(Self::parse_manifest_attributes(&content))
    }

    fn manageable(dep: &LegacyLibLoaderDependency) -> bool {
        if !dep.embedded_file.is_empty() {
            return true;
        }
        if let Ok(parsed) = Url::parse(&dep.url) {
            if parsed.scheme().eq_ignore_ascii_case("https") {
                if let Some(host) = parsed.host_str() {
                    if ALLOWED_SOURCE_HOSTS
                        .iter()
                        .any(|h| host.eq_ignore_ascii_case(h))
                    {
                        return true;
                    }
                }
            }
        }
        is_tackprofiler_coord(&dep.group, &dep.name, &dep.version)
    }

    async fn acquire(
        dep: &LegacyLibLoaderDependency,
        dest: &Path,
        client: &DownloadClient,
    ) -> Result<()> {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        if !dep.embedded_file.is_empty() {
            return Self::extract_embedded(dep, dest);
        }
        let urls = Self::candidate_urls(dep);
        let Some(first) = urls.first() else {
            return Err(MinecraftError::InvalidMetadata(format!(
                "No safe download source is available for legacy LibLoader dependency '{}'.",
                dep.display_name()
            )));
        };
        let mut req = DownloadRequest::new(first, dest);
        if urls.len() > 1 {
            req.mirrors = urls[1..].to_vec();
        }
        client.download_file(&req).await?;
        if !Self::is_valid_sha512(dest, &dep.sha512) {
            let _ = fs::remove_file(dest);
            return Err(MinecraftError::InvalidMetadata(format!(
                "Downloaded legacy LibLoader artifact '{}' failed SHA-512 verification.",
                dep.display_name()
            )));
        }
        Ok(())
    }

    fn extract_embedded(dep: &LegacyLibLoaderDependency, dest: &Path) -> Result<()> {
        // Validate embedded_file path stays inside archive without traversal
        let clean = dep.embedded_file.replace('\\', "/");
        if clean.starts_with('/')
            || Path::new(&clean)
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::RootDir))
        {
            return Err(MinecraftError::UnsafeArchivePath(dep.embedded_file.clone()));
        }
        let f = File::open(&dep.source_jar)?;
        let mut zip = ZipArchive::new(f)?;
        let mut entry = zip.by_name(&clean)?;
        if entry.size() > MAX_LIBRARY_BYTES {
            return Err(MinecraftError::InvalidMetadata(format!(
                "Embedded legacy Forge library '{}' exceeds safety limit.",
                dep.display_name()
            )));
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        let actual = sha512_hex(&bytes);
        if actual != dep.sha512 {
            return Err(MinecraftError::InvalidMetadata(format!(
                "Embedded legacy Forge library '{}' failed SHA-512 verification.",
                dep.display_name()
            )));
        }
        let tmp = dest.with_extension("jar.part");
        fs::write(&tmp, &bytes)?;
        if dest.exists() {
            let _ = fs::remove_file(dest);
        }
        fs::rename(&tmp, dest)?;
        Ok(())
    }

    fn candidate_urls(dep: &LegacyLibLoaderDependency) -> Vec<String> {
        let mut urls = Vec::new();
        if let Ok(parsed) = Url::parse(&dep.url) {
            if parsed.scheme().eq_ignore_ascii_case("https") {
                if let Some(host) = parsed.host_str() {
                    if ALLOWED_SOURCE_HOSTS
                        .iter()
                        .any(|h| host.eq_ignore_ascii_case(h))
                    {
                        urls.push(dep.url.clone());
                        let mut p = parsed.path().trim_start_matches('/').to_string();
                        if let Some(stripped) = p.strip_prefix("maven2/") {
                            p = stripped.to_string();
                        }
                        if !p.is_empty() {
                            urls.push(format!("https://repo1.maven.org/maven2/{p}"));
                            urls.push(format!("https://repo.maven.apache.org/maven2/{p}"));
                        }
                    }
                }
            }
        }
        if is_tackprofiler_coord(&dep.group, &dep.name, &dep.version) {
            urls.push(format!("{TACKPROFILER_BASE}/{}", dep.mirror_filename()));
        }
        urls
    }

    fn is_valid_sha512(path: &Path, expected_sha512: &str) -> bool {
        let Ok(meta) = fs::metadata(path) else {
            return false;
        };
        if !meta.is_file() || meta.len() > MAX_LIBRARY_BYTES {
            return false;
        }
        let Ok(bytes) = fs::read(path) else {
            return false;
        };
        sha512_hex(&bytes) == expected_sha512
    }
}

fn is_tackprofiler_coord(group: &str, name: &str, version: &str) -> bool {
    matches!(
        (group, name, version),
        ("com.eclipsesource.minimal-json", "minimal-json", "0.9.4")
            | ("com.github.javaparser", "javaparser-core", "3.2.4")
            | ("me.nallar.whocalled", "WhoCalled", "1.1")
            | ("org.javassist", "javassist", "3.22.0-CR1")
            | ("org.json", "json", "20090211")
            | (
                "org.minimallycorrect.javatransformer",
                "JavaTransformer",
                "1.8.3"
            )
    )
}

fn sha512_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha512::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    #[tokio::test]
    async fn extracts_and_verifies_embedded_legacy_libloader_dep() {
        let dir = tempdir().unwrap();
        let mc_dir = dir.path().join(".minecraft");
        let mods_dir = mc_dir.join("mods");
        fs::create_dir_all(&mods_dir).unwrap();

        let embedded_bytes = b"embedded-legacy-lib-jar-content";
        let hash = sha512_hex(embedded_bytes);

        let mod_jar = mods_dir.join("legacy-mod.jar");
        {
            let f = File::create(&mod_jar).unwrap();
            let mut zw = zip::ZipWriter::new(f);
            let opts = SimpleFileOptions::default();
            zw.start_file("META-INF/MANIFEST.MF", opts).unwrap();
            let manifest = format!(
                "Manifest-Version: 1.0\r\n\
                 LibLoader-group0: org.example\r\n\
                 LibLoader-name0: helper\r\n\
                 LibLoader-version0: 1.0.0\r\n\
                 LibLoader-file0: META-INF/libs/helper-1.0.0.jar\r\n\
                 LibLoader-sha512hash0: {hash}\r\n"
            );
            zw.write_all(manifest.as_bytes()).unwrap();
            zw.start_file("META-INF/libs/helper-1.0.0.jar", opts)
                .unwrap();
            zw.write_all(embedded_bytes).unwrap();
            zw.finish().unwrap();
        }

        let client = DownloadClient::new().unwrap();
        let warnings = LegacyLibLoaderManager::ensure(&mc_dir, "forge", &client)
            .await
            .unwrap();
        assert_eq!(warnings.len(), 1);
        let recovered = mc_dir.join("libraries/org/example/helper-1.0.0/helper-1.0.0.jar");
        assert!(recovered.is_file());
        assert_eq!(fs::read(recovered).unwrap(), embedded_bytes);
    }
}
