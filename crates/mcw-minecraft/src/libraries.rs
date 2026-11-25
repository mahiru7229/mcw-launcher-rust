use crate::rules::LibraryRules;
use crate::{MinecraftError, Result};
use mcw_models::minecraft::{DownloadArtifact, VersionMetadata};
use mcw_models::progress::{ProgressEvent, ProgressSink, ProgressStage};
use mcw_net::{DownloadClient, DownloadRequest};
use mcw_storage::LauncherPaths;
use sha1::{Digest, Sha1};
use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use zip::ZipArchive;

const DEFAULT_CONCURRENCY: usize = 20;

struct PlannedLibrary {
    request: DownloadRequest,
    is_native: bool,
    sha1_marker: String,
}

pub struct LibraryManager<'a> {
    paths: &'a LauncherPaths,
    client: &'a DownloadClient,
}

impl<'a> LibraryManager<'a> {
    pub fn new(paths: &'a LauncherPaths, client: &'a DownloadClient) -> Self {
        Self { paths, client }
    }

    pub async fn ensure_libraries(
        &self,
        version: &VersionMetadata,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<()> {
        let planned = self.collect_libraries(version)?;
        let total = planned.len() as u64;

        progress.on_progress(ProgressEvent {
            stage: ProgressStage::DownloadingLibraries,
            message: format!("Preparing {total} libraries..."),
            current: 0,
            total,
        });

        let requests: Vec<DownloadRequest> = planned.iter().map(|p| p.request.clone()).collect();
        let prog_dl = Arc::clone(&progress);
        self.client
            .download_batch(requests, DEFAULT_CONCURRENCY, move |done, tot| {
                prog_dl.on_progress(ProgressEvent {
                    stage: ProgressStage::DownloadingLibraries,
                    message: format!("Libraries {done}/{tot}"),
                    current: done,
                    total: tot,
                });
            })
            .await?;

        let natives: Vec<&PlannedLibrary> = planned.iter().filter(|p| p.is_native).collect();
        if !natives.is_empty() {
            let natives_dir = self.paths.natives_dir(&version.id);
            fs::create_dir_all(&natives_dir)?;
            let total_natives = natives.len() as u64;
            for (idx, native) in natives.iter().enumerate() {
                Self::extract_native(
                    &native.request.destination,
                    &natives_dir,
                    &native.sha1_marker,
                )?;
                progress.on_progress(ProgressEvent {
                    stage: ProgressStage::ExtractingNatives,
                    message: format!("Extracted native {}/{}", idx + 1, total_natives),
                    current: (idx + 1) as u64,
                    total: total_natives,
                });
            }
        }

        Ok(())
    }

    fn collect_libraries(&self, version: &VersionMetadata) -> Result<Vec<PlannedLibrary>> {
        let libs_dir = self.paths.libraries_dir();
        let current_os = LibraryRules::current_os();
        let arch_bits = LibraryRules::current_arch_bits();
        let mut planned = Vec::new();
        let mut seen_dests = std::collections::HashSet::new();

        for lib in &version.libraries {
            if !LibraryRules::is_allowed(&lib.rules) || lib.clientreq == Some(false) {
                continue;
            }
            let Some(ref downloads) = lib.downloads else {
                continue;
            };

            if let Some(ref artifact) = downloads.artifact {
                if let Some(item) = Self::plan_artifact(&libs_dir, artifact, false)? {
                    if seen_dests.insert((item.request.destination.clone(), false)) {
                        planned.push(item);
                    }
                }
            }

            if let Some(ref natives_map) = lib.natives {
                if let Some(template) = natives_map.get(current_os) {
                    let classifier = template.replace("${arch}", arch_bits);
                    if let Some(native_art) = downloads.classifiers.get(&classifier) {
                        if let Some(item) = Self::plan_artifact(&libs_dir, native_art, true)? {
                            if seen_dests.insert((item.request.destination.clone(), true)) {
                                planned.push(item);
                            }
                        }
                    }
                }
            }
        }

        Ok(planned)
    }

    fn plan_artifact(
        libs_dir: &Path,
        artifact: &DownloadArtifact,
        is_native: bool,
    ) -> Result<Option<PlannedLibrary>> {
        let Some(ref rel) = artifact.path else {
            return Ok(None);
        };
        if artifact.url.trim().is_empty() {
            return Ok(None);
        }
        let dest = LauncherPaths::safe_join_relative(libs_dir, rel)?;
        let sha1_marker = artifact
            .sha1
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| rel.replace(['/', '\\'], "_"));
        let req = DownloadRequest::new(&artifact.url, dest)
            .with_sha1(artifact.sha1.clone())
            .with_size(artifact.size);

        Ok(Some(PlannedLibrary {
            request: req,
            is_native,
            sha1_marker,
        }))
    }

    pub fn extract_native(native_jar: &Path, destination: &Path, sha1_marker: &str) -> Result<()> {
        let marker_dir = destination.join(".extracted");
        let marker_path = marker_dir.join(sha1_marker);
        if marker_path.is_file() {
            return Ok(());
        }

        fs::create_dir_all(&marker_dir)?;

        let file = File::open(native_jar)?;
        let mut archive = ZipArchive::new(file)?;

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let Some(rel) = safe_native_entry_path(entry.name(), entry.unix_mode())? else {
                continue;
            };
            let target = destination.join(rel);
            if entry.is_dir() {
                fs::create_dir_all(&target)?;
                continue;
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out = File::create(&target)?;
            io::copy(&mut entry, &mut out)?;
        }

        fs::write(&marker_path, b"ok")?;
        Ok(())
    }

    /// Removes stale FreeType native libraries (`*freetype*`) and their extraction markers from
    /// `natives/<version>/` before extracting an updated `lwjgl-freetype` native JAR.
    pub fn clean_freetype_natives(natives_dir: &Path) -> Result<usize> {
        if !natives_dir.is_dir() {
            return Ok(0);
        }
        let mut removed = 0usize;
        for entry in fs::read_dir(natives_dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if path.is_file() && name.contains("freetype") {
                fs::remove_file(&path)?;
                removed += 1;
            }
        }
        let marker_dir = natives_dir.join(".extracted");
        if marker_dir.is_dir() {
            for entry in fs::read_dir(&marker_dir)? {
                let entry = entry?;
                let p = entry.path();
                if p.is_file() {
                    let _ = fs::remove_file(&p);
                }
            }
        }
        Ok(removed)
    }

    /// Verifies that a [`DownloadArtifact`] exists on disk inside `libraries/` and matches its SHA-1
    /// (when a SHA-1 checksum is present in metadata).
    pub fn verify_artifact_on_disk(&self, artifact: &DownloadArtifact) -> Result<bool> {
        Self::verify_artifact_on_disk_at(&self.paths.libraries_dir(), artifact)
    }

    /// Static variant of [`Self::verify_artifact_on_disk`] for offline security assessments.
    pub fn verify_artifact_on_disk_at(
        libs_dir: &Path,
        artifact: &DownloadArtifact,
    ) -> Result<bool> {
        let Some(ref rel) = artifact.path else {
            return Ok(false);
        };
        let dest = LauncherPaths::safe_join_relative(libs_dir, rel)?;
        if !dest.is_file() {
            return Ok(false);
        }
        if let Some(ref expected_sha1) = artifact.sha1 {
            let trimmed = expected_sha1.trim().to_ascii_lowercase();
            if !trimmed.is_empty() {
                let bytes = fs::read(&dest)?;
                let mut hasher = Sha1::new();
                hasher.update(&bytes);
                let digest = hasher.finalize();
                let mut hex = String::with_capacity(40);
                for b in digest {
                    hex.push_str(&format!("{b:02x}"));
                }
                return Ok(hex == trimmed);
            }
        }
        Ok(fs::metadata(&dest).map(|m| m.len() > 0).unwrap_or(false))
    }
}

fn safe_native_entry_path(name: &str, unix_mode: Option<u32>) -> Result<Option<PathBuf>> {
    // Reject symlinks (POSIX S_IFLNK = 0o120000)
    if let Some(mode) = unix_mode {
        if (mode & 0o170000) == 0o120000 {
            return Err(MinecraftError::UnsafeArchivePath(format!(
                "symbolic link in archive: {name}"
            )));
        }
    }

    let normalized = name.replace('\\', "/");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let path = Path::new(trimmed);
    let mut parts = Vec::new();
    for comp in path.components() {
        match comp {
            Component::Normal(p) => {
                let s = p.to_string_lossy();
                if s.contains(':') {
                    return Err(MinecraftError::UnsafeArchivePath(name.to_string()));
                }
                parts.push(p);
            }
            _ => return Err(MinecraftError::UnsafeArchivePath(name.to_string())),
        }
    }

    if parts.is_empty() {
        return Ok(None);
    }

    let first = parts[0].to_string_lossy();
    if first.eq_ignore_ascii_case("META-INF") {
        return Ok(None);
    }
    if first.eq_ignore_ascii_case(".extracted") {
        return Err(MinecraftError::UnsafeArchivePath(name.to_string()));
    }

    let mut out = PathBuf::new();
    for p in parts {
        out.push(p);
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zip_slip_and_skips_meta_inf() {
        assert!(safe_native_entry_path("../evil.dll", None).is_err());
        assert!(safe_native_entry_path("/abs/evil.dll", None).is_err());
        assert!(safe_native_entry_path(".extracted/fake", None).is_err());
        assert!(safe_native_entry_path("META-INF/MANIFEST.MF", None)
            .unwrap()
            .is_none());
        assert_eq!(
            safe_native_entry_path("lwjgl.dll", None).unwrap(),
            Some(PathBuf::from("lwjgl.dll"))
        );
    }
}
