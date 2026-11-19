use crate::{MinecraftError, Result};
use mcw_models::minecraft::{
    Arguments, DownloadArtifact, LibraryDownloads, LibraryEntry, VersionMetadata,
};
use mcw_net::DownloadClient;
use mcw_storage::{atomic_write_text, LauncherPaths};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use tracing::warn;

const FABRIC_META_BASE: &str = "https://meta.fabricmc.net/v2";
const QUILT_META_BASE: &str = "https://meta.quiltmc.org/v3";
const FABRIC_MAVEN_DEFAULT: &str = "https://maven.fabricmc.net/";
const QUILT_MAVEN_DEFAULT: &str = "https://maven.quiltmc.org/repository/release/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoaderKind {
    Fabric,
    Quilt,
}

impl LoaderKind {
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "fabric" => Some(Self::Fabric),
            "quilt" => Some(Self::Quilt),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fabric => "fabric",
            Self::Quilt => "quilt",
        }
    }

    fn meta_base(&self) -> &'static str {
        match self {
            Self::Fabric => FABRIC_META_BASE,
            Self::Quilt => QUILT_META_BASE,
        }
    }

    fn default_maven(&self) -> &'static str {
        match self {
            Self::Fabric => FABRIC_MAVEN_DEFAULT,
            Self::Quilt => QUILT_MAVEN_DEFAULT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoaderVersionEntry {
    pub version: String,
    pub stable: bool,
}

#[derive(Debug, Deserialize)]
struct LoaderProfileJson {
    id: String,
    #[serde(rename = "mainClass")]
    main_class: String,
    #[serde(default)]
    arguments: Option<Arguments>,
    #[serde(default)]
    libraries: Vec<MavenProfileLibrary>,
}

#[derive(Debug, Deserialize)]
struct MavenProfileLibrary {
    name: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    sha1: Option<String>,
    #[serde(default)]
    size: u64,
}

pub struct ModLoaderService<'a> {
    paths: &'a LauncherPaths,
    client: &'a DownloadClient,
}

impl<'a> ModLoaderService<'a> {
    pub fn new(paths: &'a LauncherPaths, client: &'a DownloadClient) -> Self {
        Self { paths, client }
    }

    pub async fn list_versions(
        &self,
        kind: LoaderKind,
        game_version: &str,
    ) -> Result<Vec<LoaderVersionEntry>> {
        let url = format!("{}/versions/loader/{game_version}", kind.meta_base());
        let raw = self.client.get_text(&url).await?;
        let arr: Vec<serde_json::Value> = serde_json::from_str(&raw)?;

        let mut out = Vec::new();
        for item in arr {
            let loader = item.get("loader").unwrap_or(&item);
            let Some(version) = loader.get("version").and_then(|v| v.as_str()) else {
                continue;
            };
            let version = version.trim();
            if version.is_empty() {
                continue;
            }
            let stable = loader
                .get("stable")
                .and_then(|v| v.as_bool())
                .unwrap_or_else(|| !version.contains('-'));
            out.push(LoaderVersionEntry {
                version: version.to_string(),
                stable,
            });
        }
        Ok(out)
    }

    pub async fn resolve_version(
        &self,
        kind: LoaderKind,
        game_version: &str,
        requested: &str,
    ) -> Result<String> {
        let trimmed = requested.trim();
        if !trimmed.is_empty() && trimmed != "-1" && !trimmed.eq_ignore_ascii_case("latest") {
            return Ok(trimmed.to_string());
        }
        let versions = self.list_versions(kind, game_version).await?;
        versions
            .iter()
            .find(|v| v.stable)
            .or_else(|| versions.first())
            .map(|v| v.version.clone())
            .ok_or_else(|| {
                MinecraftError::InvalidMetadata(format!(
                    "No {} loader versions available for Minecraft {game_version}",
                    kind.as_str()
                ))
            })
    }

    pub async fn install_and_merge(
        &self,
        kind: LoaderKind,
        base: &VersionMetadata,
        loader_version: &str,
    ) -> Result<VersionMetadata> {
        let resolved_loader = self
            .resolve_version(kind, &base.id, loader_version)
            .await?;
        let cache_id = format!("{}-{}-{}", kind.as_str(), resolved_loader, base.id);
        let cache_path = self.paths.version_json(&cache_id);

        let url = format!(
            "{}/versions/loader/{}/{}/profile/json",
            kind.meta_base(),
            base.id,
            resolved_loader
        );

        let raw = match self.client.get_text(&url).await {
            Ok(text) => {
                let _ = atomic_write_text(&cache_path, &text);
                text
            }
            Err(net_err) => {
                if cache_path.is_file() {
                    warn!(error = %net_err, "Using cached {} profile JSON", kind.as_str());
                    fs::read_to_string(&cache_path)?
                } else {
                    return Err(net_err.into());
                }
            }
        };

        let profile: LoaderProfileJson = serde_json::from_str(&raw)?;
        Ok(merge_loader_profile(base, &profile, kind.default_maven()))
    }
}

fn merge_loader_profile(
    base: &VersionMetadata,
    profile: &LoaderProfileJson,
    default_maven: &str,
) -> VersionMetadata {
    let mut merged = base.clone();
    merged.id = profile.id.clone();
    merged.inherits_from = Some(base.id.clone());
    merged.main_class = profile.main_class.clone();

    // Append loader Maven libraries so ClasspathBuilder deduplication prefers them over base.
    for lib in &profile.libraries {
        if let Some(entry) = maven_library_to_entry(lib, default_maven) {
            merged.libraries.push(entry);
        }
    }

    if let Some(ref loader_args) = profile.arguments {
        match merged.arguments {
            Some(ref mut base_args) => {
                base_args.jvm.extend(loader_args.jvm.iter().cloned());
                base_args.game.extend(loader_args.game.iter().cloned());
            }
            None => {
                merged.arguments = Some(loader_args.clone());
            }
        }
    }

    merged
}

fn maven_library_to_entry(
    lib: &MavenProfileLibrary,
    default_maven: &str,
) -> Option<LibraryEntry> {
    let rel_path = maven_coordinate_to_path(&lib.name)?;
    let base_url = lib
        .url
        .as_deref()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or(default_maven);
    let url = format!("{}/{}", base_url.trim_end_matches('/'), rel_path);

    Some(LibraryEntry {
        name: lib.name.clone(),
        url: lib.url.clone(),
        downloads: Some(LibraryDownloads {
            artifact: Some(DownloadArtifact {
                path: Some(rel_path),
                sha1: lib.sha1.clone(),
                size: lib.size,
                url,
            }),
            classifiers: HashMap::new(),
        }),
        rules: Vec::new(),
        natives: None,
        clientreq: None,
    })
}

pub fn maven_coordinate_to_path(coordinate: &str) -> Option<String> {
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
    let group_path = group.replace('.', "/");
    let filename = if classifier.is_empty() {
        format!("{artifact}-{version}.{ext}")
    } else {
        format!("{artifact}-{version}-{classifier}.{ext}")
    };
    Some(format!("{group_path}/{artifact}/{version}/{filename}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_maven_coordinates_to_paths() {
        assert_eq!(
            maven_coordinate_to_path("net.fabricmc:fabric-loader:0.16.10"),
            Some("net/fabricmc/fabric-loader/0.16.10/fabric-loader-0.16.10.jar".to_string())
        );
        assert_eq!(
            maven_coordinate_to_path("org.ow2.asm:asm:9.7.1:sources@jar"),
            Some("org/ow2/asm/asm/9.7.1/asm-9.7.1-sources.jar".to_string())
        );
    }

    #[test]
    fn merges_fabric_profile_onto_vanilla_metadata() {
        let base = VersionMetadata {
            id: "1.21.4".to_string(),
            version_type: "release".to_string(),
            main_class: "net.minecraft.client.main.Main".to_string(),
            inherits_from: None,
            asset_index: None,
            assets: Some("19".to_string()),
            downloads: None,
            libraries: vec![],
            java_version: None,
            arguments: None,
            minecraft_arguments: None,
        };
        let profile = LoaderProfileJson {
            id: "fabric-loader-0.16.10-1.21.4".to_string(),
            main_class: "net.fabricmc.loader.impl.launch.knot.KnotClient".to_string(),
            arguments: None,
            libraries: vec![MavenProfileLibrary {
                name: "net.fabricmc:fabric-loader:0.16.10".to_string(),
                url: Some("https://maven.fabricmc.net/".to_string()),
                sha1: None,
                size: 0,
            }],
        };

        let merged = merge_loader_profile(&base, &profile, FABRIC_MAVEN_DEFAULT);
        assert_eq!(merged.inherits_from.as_deref(), Some("1.21.4"));
        assert_eq!(
            merged.main_class,
            "net.fabricmc.loader.impl.launch.knot.KnotClient"
        );
        assert_eq!(merged.libraries.len(), 1);
        let art = merged.libraries[0]
            .downloads
            .as_ref()
            .unwrap()
            .artifact
            .as_ref()
            .unwrap();
        assert_eq!(
            art.url,
            "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.16.10/fabric-loader-0.16.10.jar"
        );
    }
}
