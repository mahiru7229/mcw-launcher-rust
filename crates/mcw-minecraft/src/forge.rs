use crate::{MinecraftError, Result};
use mcw_models::minecraft::{DownloadArtifact, LibraryDownloads, LibraryEntry, VersionMetadata};
use mcw_net::{DownloadClient, DownloadRequest};
use mcw_storage::{atomic_write_text, LauncherPaths};
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::info;
use zip::ZipArchive;

const FORGE_MAVEN_ROOT: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";
const FORGE_METADATA_URL: &str =
    "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";
const FORGE_MAVEN_BASE: &str = "https://maven.minecraftforge.net/";
const MOJANG_LIBS_BASE: &str = "https://libraries.minecraft.net/";

const NEOFORGE_MODERN_ROOT: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";
const NEOFORGE_LEGACY_ROOT: &str = "https://maven.neoforged.net/releases/net/neoforged/forge";
const NEOFORGE_MODERN_METADATA: &str =
    "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";
const NEOFORGE_LEGACY_METADATA: &str =
    "https://maven.neoforged.net/releases/net/neoforged/forge/maven-metadata.xml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeFamily {
    Forge,
    NeoForge,
}

impl ForgeFamily {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "forge" => Some(Self::Forge),
            "neoforge" => Some(Self::NeoForge),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Forge => "forge",
            Self::NeoForge => "neoforge",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ForgeLoaderVersion {
    pub family: ForgeFamily,
    pub game_version: String,
    pub loader_version: String,
    pub installer_url: String,
}

#[derive(Debug, Deserialize)]
struct ForgeVersionJson {
    id: String,
    #[serde(rename = "mainClass")]
    main_class: String,
    #[serde(default)]
    arguments: Option<mcw_models::minecraft::Arguments>,
    #[serde(rename = "minecraftArguments", default)]
    minecraft_arguments: Option<String>,
    #[serde(default)]
    libraries: Vec<LibraryEntry>,
}

#[derive(Debug, Deserialize)]
struct LegacyInstallProfile {
    install: LegacyInstallSection,
    #[serde(rename = "versionInfo")]
    version_info: ForgeVersionJson,
}

#[derive(Debug, Deserialize)]
struct LegacyInstallSection {
    #[serde(rename = "filePath")]
    file_path: String,
    path: String,
}

pub struct ForgeLoaderService<'a> {
    paths: &'a LauncherPaths,
    client: &'a DownloadClient,
}

impl<'a> ForgeLoaderService<'a> {
    pub fn new(paths: &'a LauncherPaths, client: &'a DownloadClient) -> Self {
        Self { paths, client }
    }

    pub async fn list_versions(
        &self,
        family: ForgeFamily,
        game_version: &str,
    ) -> Result<Vec<ForgeLoaderVersion>> {
        let gv = game_version.trim();
        match family {
            ForgeFamily::Forge => {
                let xml = self.client.get_text(FORGE_METADATA_URL).await?;
                let all = extract_xml_versions(&xml);
                let prefix = format!("{gv}-");
                let mut out: Vec<ForgeLoaderVersion> = all
                    .into_iter()
                    .filter_map(|coord| {
                        let ver = coord.strip_prefix(&prefix)?;
                        let url = format!("{FORGE_MAVEN_ROOT}/{coord}/forge-{coord}-installer.jar");
                        Some(ForgeLoaderVersion {
                            family: ForgeFamily::Forge,
                            game_version: gv.to_string(),
                            loader_version: ver.to_string(),
                            installer_url: url,
                        })
                    })
                    .collect();
                out.reverse();
                Ok(out)
            }
            ForgeFamily::NeoForge => {
                if gv == "1.20.1" {
                    let xml = self.client.get_text(NEOFORGE_LEGACY_METADATA).await?;
                    let all = extract_xml_versions(&xml);
                    let prefix = "1.20.1-";
                    let mut out: Vec<ForgeLoaderVersion> = all
                        .into_iter()
                        .filter_map(|coord| {
                            let ver = coord.strip_prefix(prefix)?;
                            let url = format!(
                                "{NEOFORGE_LEGACY_ROOT}/{coord}/forge-{coord}-installer.jar"
                            );
                            Some(ForgeLoaderVersion {
                                family: ForgeFamily::NeoForge,
                                game_version: gv.to_string(),
                                loader_version: ver.to_string(),
                                installer_url: url,
                            })
                        })
                        .collect();
                    out.reverse();
                    Ok(out)
                } else {
                    let xml = self.client.get_text(NEOFORGE_MODERN_METADATA).await?;
                    let all = extract_xml_versions(&xml);
                    let mut out: Vec<ForgeLoaderVersion> = all
                        .into_iter()
                        .filter(|ver| neoforge_matches_mc(gv, ver))
                        .map(|ver| {
                            let url = format!(
                                "{NEOFORGE_MODERN_ROOT}/{ver}/neoforge-{ver}-installer.jar"
                            );
                            ForgeLoaderVersion {
                                family: ForgeFamily::NeoForge,
                                game_version: gv.to_string(),
                                loader_version: ver,
                                installer_url: url,
                            }
                        })
                        .collect();
                    out.reverse();
                    Ok(out)
                }
            }
        }
    }

    pub async fn resolve_version(
        &self,
        family: ForgeFamily,
        game_version: &str,
        requested: &str,
    ) -> Result<ForgeLoaderVersion> {
        let clean = requested
            .trim()
            .strip_prefix(&format!("{game_version}-"))
            .unwrap_or(requested.trim());
        let versions = self.list_versions(family, game_version).await?;
        if clean.is_empty() || clean == "-1" || clean.eq_ignore_ascii_case("latest") {
            return versions.into_iter().next().ok_or_else(|| {
                MinecraftError::InvalidMetadata(format!(
                    "No {} versions found for Minecraft {game_version}",
                    family.as_str()
                ))
            });
        }
        if let Some(found) = versions.iter().find(|v| v.loader_version == clean) {
            return Ok(found.clone());
        }
        // Fallback direct URL construction if exact version wasn't in trimmed list.
        let url = match family {
            ForgeFamily::Forge => {
                let coord = format!("{game_version}-{clean}");
                format!("{FORGE_MAVEN_ROOT}/{coord}/forge-{coord}-installer.jar")
            }
            ForgeFamily::NeoForge => {
                if game_version == "1.20.1" {
                    let coord = format!("{game_version}-{clean}");
                    format!("{NEOFORGE_LEGACY_ROOT}/{coord}/forge-{coord}-installer.jar")
                } else {
                    format!("{NEOFORGE_MODERN_ROOT}/{clean}/neoforge-{clean}-installer.jar")
                }
            }
        };
        Ok(ForgeLoaderVersion {
            family,
            game_version: game_version.to_string(),
            loader_version: clean.to_string(),
            installer_url: url,
        })
    }

    pub async fn download_installer(&self, entry: &ForgeLoaderVersion) -> Result<PathBuf> {
        self.download_installer_with_progress(entry, |_, _| {})
            .await
    }

    pub async fn download_installer_with_progress<F>(
        &self,
        entry: &ForgeLoaderVersion,
        on_bytes: F,
    ) -> Result<PathBuf>
    where
        F: Fn(u64, u64) + Send + Sync + 'static,
    {
        let dest = self.paths.cache_root.join("installers").join(format!(
            "{}-{}-{}-installer.jar",
            entry.family.as_str(),
            entry.game_version,
            entry.loader_version
        ));
        let req = DownloadRequest::new(&entry.installer_url, &dest);
        self.client
            .download_file_with_progress(&req, on_bytes)
            .await?;
        Ok(dest)
    }

    /// Install & merge Forge or NeoForge (supports BOTH legacy pre-1.13 installers like 1.12.2
    /// RLCraft and modern 1.13+ `--installClient` installers).
    pub async fn install_and_merge(
        &self,
        family: ForgeFamily,
        base: &VersionMetadata,
        requested_version: &str,
        installer_java: Option<&Path>,
    ) -> Result<VersionMetadata> {
        self.install_and_merge_with_progress(
            family,
            base,
            requested_version,
            installer_java,
            |_, _| {},
        )
        .await
    }

    /// Install & merge Forge or NeoForge while reporting installer JAR download bytes `(done, total)`.
    pub async fn install_and_merge_with_progress<F>(
        &self,
        family: ForgeFamily,
        base: &VersionMetadata,
        requested_version: &str,
        installer_java: Option<&Path>,
        on_bytes: F,
    ) -> Result<VersionMetadata>
    where
        F: Fn(u64, u64) + Send + Sync + 'static,
    {
        let entry = self
            .resolve_version(family, &base.id, requested_version)
            .await?;
        let cache_id = format!(
            "{}-{}-{}",
            family.as_str(),
            entry.game_version,
            entry.loader_version
        );
        let cache_path = self.paths.version_json(&cache_id);

        if cache_path.is_file() {
            if let Ok(raw) = fs::read_to_string(&cache_path) {
                if let Ok(cached) = serde_json::from_str::<VersionMetadata>(&raw) {
                    return Ok(cached);
                }
            }
        }

        info!(
            family = family.as_str(),
            game = %entry.game_version,
            loader = %entry.loader_version,
            "Downloading Forge/NeoForge installer"
        );
        let installer_jar = self
            .download_installer_with_progress(&entry, on_bytes)
            .await?;

        // Check if this is a legacy (pre-1.13, e.g. 1.12.2) Forge installer with `versionInfo`
        // embedded in `install_profile.json`.
        let merged = if let Some(legacy) = try_read_legacy_profile(&installer_jar)? {
            info!("Installing legacy Forge profile directly (no Swing UI)");
            self.extract_legacy_universal_jar(&installer_jar, &legacy.install)?;
            merge_forge_profile(base, &legacy.version_info, &self.paths.libraries_dir())
        } else {
            info!("Installing modern Forge/NeoForge profile via --installClient");
            let profile = self.run_modern_installer(&installer_jar, base, installer_java)?;
            merge_forge_profile(base, &profile, &self.paths.libraries_dir())
        };

        if let Ok(serialized) = serde_json::to_string_pretty(&merged) {
            let _ = atomic_write_text(&cache_path, &serialized);
        }
        Ok(merged)
    }

    fn extract_legacy_universal_jar(
        &self,
        installer_jar: &Path,
        install: &LegacyInstallSection,
    ) -> Result<()> {
        let rel = crate::modloader::maven_coordinate_to_path(&install.path).ok_or_else(|| {
            MinecraftError::InvalidMetadata(format!(
                "Invalid legacy Forge Maven coordinate: {}",
                install.path
            ))
        })?;
        let dest = LauncherPaths::safe_join_relative(&self.paths.libraries_dir(), &rel)?;
        if dest.is_file() {
            return Ok(());
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }

        let file = File::open(installer_jar)?;
        let mut archive = ZipArchive::new(file)?;
        let member_name = install.file_path.trim_start_matches('/');
        let mut entry = archive.by_name(member_name).map_err(|_| {
            MinecraftError::InvalidMetadata(format!(
                "Legacy Forge installer is missing embedded universal JAR '{member_name}'"
            ))
        })?;
        let mut out = File::create(&dest)?;
        io::copy(&mut entry, &mut out)?;
        Ok(())
    }

    fn run_modern_installer(
        &self,
        installer_jar: &Path,
        base: &VersionMetadata,
        installer_java: Option<&Path>,
    ) -> Result<ForgeVersionJson> {
        let staging = self
            .paths
            .cache_root
            .join("forge-staging")
            .join(format!("stage-{}", base.id));
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        fs::create_dir_all(staging.join("versions").join(&base.id))?;
        fs::create_dir_all(staging.join("libraries"))?;
        fs::write(staging.join("launcher_profiles.json"), r#"{"profiles":{}}"#)?;

        // Copy base client.jar & version.json into staging so Forge/NeoForge processors find them.
        let base_jar = self.paths.client_jar(&base.id);
        let base_json = self.paths.version_json(&base.id);
        if base_jar.is_file() {
            let _ = fs::copy(
                &base_jar,
                staging
                    .join("versions")
                    .join(&base.id)
                    .join(format!("{}.jar", base.id)),
            );
        }
        if base_json.is_file() {
            let _ = fs::copy(
                &base_json,
                staging
                    .join("versions")
                    .join(&base.id)
                    .join(format!("{}.json", base.id)),
            );
        }

        if let Some(java_exe) = installer_java {
            let resolved_java = mcw_runtime::prefer_javaw_on_windows(java_exe);
            let mut cmd = Command::new(&resolved_java);
            cmd.arg("-jar")
                .arg(installer_jar)
                .arg("--installClient")
                .arg(&staging);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            let output = cmd.output()?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                return Err(MinecraftError::InvalidMetadata(format!(
                    "Forge/NeoForge installer failed: {stderr}\n{stdout}"
                )));
            }
            // Copy generated libraries from staging into shared cache/libraries/
            copy_dir_merge(&staging.join("libraries"), &self.paths.libraries_dir())?;
        }

        // Extract version.json from staging/versions or directly from installer JAR
        let staged_profile = find_generated_version_json(&staging.join("versions"), &base.id);
        let raw = if let Some(path) = staged_profile {
            fs::read_to_string(path)?
        } else {
            let file = File::open(installer_jar)?;
            let mut archive = ZipArchive::new(file)?;
            let mut buf = String::new();
            archive
                .by_name("version.json")
                .map_err(|_| {
                    MinecraftError::InvalidMetadata(
                        "Forge/NeoForge installer is missing version.json".to_string(),
                    )
                })?
                .read_to_string(&mut buf)?;
            buf
        };

        let _ = fs::remove_dir_all(&staging);
        Ok(serde_json::from_str(&raw)?)
    }
}

fn try_read_legacy_profile(installer_jar: &Path) -> Result<Option<LegacyInstallProfile>> {
    let file = File::open(installer_jar)?;
    let mut archive = ZipArchive::new(file)?;
    let Ok(mut entry) = archive.by_name("install_profile.json") else {
        return Ok(None);
    };
    let mut raw = String::new();
    entry.read_to_string(&mut raw)?;
    match serde_json::from_str::<LegacyInstallProfile>(&raw) {
        Ok(legacy) => Ok(Some(legacy)),
        Err(_) => Ok(None),
    }
}

fn find_generated_version_json(versions_dir: &Path, base_id: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(versions_dir).ok()?;
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == base_id {
            continue;
        }
        let candidate = dir.join(format!("{name}.json"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn copy_dir_merge(from: &Path, to: &Path) -> io::Result<()> {
    if !from.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_dir_merge(&src, &dst)?;
        } else if src.is_file() && !dst.is_file() {
            fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

fn merge_forge_profile(
    base: &VersionMetadata,
    profile: &ForgeVersionJson,
    libraries_dir: &Path,
) -> VersionMetadata {
    let mut merged = base.clone();
    merged.id = profile.id.clone();
    merged.inherits_from = Some(base.id.clone());
    merged.main_class = profile.main_class.clone();

    for lib in &profile.libraries {
        if lib.clientreq == Some(false) {
            continue;
        }
        let mut entry = lib.clone();
        if entry.downloads.is_none() {
            if let Some(rel) = crate::modloader::maven_coordinate_to_path(&entry.name) {
                let already_local = libraries_dir.join(&rel).is_file();
                let url = if already_local {
                    String::new()
                } else {
                    let repo = entry
                        .url
                        .as_deref()
                        .filter(|u| !u.trim().is_empty())
                        .unwrap_or_else(|| {
                            if entry.name.starts_with("net.minecraftforge:") {
                                FORGE_MAVEN_BASE
                            } else {
                                MOJANG_LIBS_BASE
                            }
                        });
                    format!("{}/{rel}", repo.trim_end_matches('/'))
                };
                entry.downloads = Some(LibraryDownloads {
                    artifact: Some(DownloadArtifact {
                        path: Some(rel),
                        sha1: None,
                        size: 0,
                        url,
                    }),
                    classifiers: HashMap::new(),
                });
            }
        }
        merged.libraries.push(entry);
    }

    if let Some(ref forge_args) = profile.arguments {
        match merged.arguments {
            Some(ref mut base_args) => {
                base_args.jvm.extend(forge_args.jvm.iter().cloned());
                base_args.game.extend(forge_args.game.iter().cloned());
            }
            None => merged.arguments = Some(forge_args.clone()),
        }
    }
    if profile.minecraft_arguments.is_some() {
        merged.minecraft_arguments = profile.minecraft_arguments.clone();
    }

    merged
}

pub fn extract_xml_versions(xml: &str) -> Vec<String> {
    let re = Regex::new(r"<version>([^<]+)</version>").unwrap();
    re.captures_iter(xml)
        .filter_map(|c| c.get(1).map(|m| m.as_str().trim().to_string()))
        .filter(|s| !s.is_empty())
        .collect()
}

/// NeoForge versioning convention: MC `1.21.4` -> NeoForge `21.4.x`, MC `1.20.4` -> `20.4.x`,
/// MC `1.21` -> `21.0.x`.
pub fn neoforge_matches_mc(game_version: &str, neoforge_ver: &str) -> bool {
    let stripped = game_version.strip_prefix("1.").unwrap_or(game_version);
    let mut parts = stripped.split('.');
    let Some(minor) = parts.next() else {
        return false;
    };
    let patch = parts.next().unwrap_or("0");
    let expected_prefix = format!("{minor}.{patch}.");
    neoforge_ver.starts_with(&expected_prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_maven_metadata_xml_and_matches_neoforge_versions() {
        let xml = r#"<metadata><versioning><versions><version>21.4.100-beta</version><version>21.4.120</version><version>21.1.50</version></versions></versioning></metadata>"#;
        let vers = extract_xml_versions(xml);
        assert_eq!(vers.len(), 3);
        assert!(neoforge_matches_mc("1.21.4", "21.4.120"));
        assert!(!neoforge_matches_mc("1.21.4", "21.1.50"));
        assert!(neoforge_matches_mc("1.21", "21.0.14"));
    }
}
