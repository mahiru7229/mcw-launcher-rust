use crate::{MinecraftError, Result};
use mcw_models::minecraft::{DownloadArtifact, LibraryDownloads, LibraryEntry, VersionMetadata};
use mcw_net::{DownloadClient, DownloadRequest};
use mcw_storage::LauncherPaths;
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

const FORGE_MAVEN_ROOT: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";
const FORGE_METADATA_URL: &str =
    "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";

const NEOFORGE_MODERN_ROOT: &str =
    "https://maven.neoforged.net/releases/net/neoforged/neoforge";
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
                        let url = format!(
                            "{FORGE_MAVEN_ROOT}/{coord}/forge-{coord}-installer.jar"
                        );
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

    pub async fn download_installer(
        &self,
        entry: &ForgeLoaderVersion,
    ) -> Result<std::path::PathBuf> {
        let dest = self
            .paths
            .cache_root
            .join("installers")
            .join(format!(
                "{}-{}-{}-installer.jar",
                entry.family.as_str(),
                entry.game_version,
                entry.loader_version
            ));
        let req = DownloadRequest::new(&entry.installer_url, &dest);
        self.client.download_file(&req).await?;
        Ok(dest)
    }

    /// Extract `version.json` from a downloaded Forge/NeoForge installer JAR and merge
    /// onto base Vanilla `VersionMetadata`.
    pub fn extract_and_merge_profile(
        installer_jar: &Path,
        base: &VersionMetadata,
    ) -> Result<VersionMetadata> {
        let file = File::open(installer_jar)?;
        let mut archive = ZipArchive::new(file)?;
        let mut raw = String::new();
        archive
            .by_name("version.json")
            .map_err(|_| {
                MinecraftError::InvalidMetadata(
                    "Forge/NeoForge installer archive is missing version.json".to_string(),
                )
            })?
            .read_to_string(&mut raw)?;

        let profile: ForgeVersionJson = serde_json::from_str(&raw)?;
        Ok(merge_forge_profile(base, &profile))
    }
}

fn merge_forge_profile(base: &VersionMetadata, profile: &ForgeVersionJson) -> VersionMetadata {
    let mut merged = base.clone();
    merged.id = profile.id.clone();
    merged.inherits_from = Some(base.id.clone());
    merged.main_class = profile.main_class.clone();

    for lib in &profile.libraries {
        let mut entry = lib.clone();
        // Ensure Maven coordinate-only entries have a fallback artifact path.
        if entry.downloads.is_none() {
            if let Some(rel) = crate::modloader::maven_coordinate_to_path(&entry.name) {
                entry.downloads = Some(LibraryDownloads {
                    artifact: Some(DownloadArtifact {
                        path: Some(rel.clone()),
                        sha1: None,
                        size: 0,
                        url: format!("https://maven.minecraftforge.net/{rel}"),
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
