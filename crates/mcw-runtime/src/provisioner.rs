use crate::java::{JavaDiscovery, JavaInstallation, JavaMajorPolicy};
use crate::java_diagnostics::JavaRecoveryDiagnostics;
use crate::{Result, RuntimeError};
use mcw_net::{DownloadClient, DownloadRequest};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use tracing::info;
use zip::ZipArchive;

const ADOPTIUM_ASSETS_URL: &str = "https://api.adoptium.net/v3/assets/latest";
const ADOPTIUM_AVAILABLE_RELEASES_URL: &str = "https://api.adoptium.net/v3/info/available_releases";

#[derive(Debug, Clone)]
pub struct AdoptiumRelease {
    pub major: u32,
    pub release_name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub filename: String,
}

pub struct JavaProvisioner;

impl JavaProvisioner {
    pub fn managed_dir(runtimes_root: &Path, major: u32) -> PathBuf {
        runtimes_root.join(format!("java-{major}"))
    }

    pub fn managed_executable(runtimes_root: &Path, major: u32) -> PathBuf {
        let exe = if cfg!(windows) { "java.exe" } else { "java" };
        Self::managed_dir(runtimes_root, major)
            .join("bin")
            .join(exe)
    }

    pub fn parse_latest_feature_release(raw: &str) -> Result<u32> {
        let payload: serde_json::Value = serde_json::from_str(raw).map_err(|e| {
            RuntimeError::ProvisionFailed(format!(
                "Invalid Adoptium available-releases response: {e}"
            ))
        })?;
        if let Some(major) = payload
            .get("most_recent_feature_release")
            .and_then(|v| v.as_u64())
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| n >= 8)
        {
            return Ok(major);
        }
        if let Some(max_major) = payload
            .get("available_releases")
            .and_then(|v| v.as_array())
            .and_then(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_u64().and_then(|n| u32::try_from(n).ok()))
                    .filter(|&n| n >= 8)
                    .max()
            })
        {
            return Ok(max_major);
        }
        Err(RuntimeError::ProvisionFailed(
            "Adoptium did not report a latest GA Java feature release.".to_string(),
        ))
    }

    pub async fn fetch_latest_feature_release(client: &DownloadClient) -> Result<u32> {
        let raw = client.get_text(ADOPTIUM_AVAILABLE_RELEASES_URL).await?;
        Self::parse_latest_feature_release(&raw)
    }

    pub async fn fetch_latest_adoptium(
        client: &DownloadClient,
        major: u32,
    ) -> Result<AdoptiumRelease> {
        let managed_major = JavaMajorPolicy::resolve(major)?;
        Self::fetch_latest_adoptium_exact(client, managed_major).await
    }

    pub async fn fetch_latest_adoptium_exact(
        client: &DownloadClient,
        exact_major: u32,
    ) -> Result<AdoptiumRelease> {
        if exact_major < 8 {
            return Err(RuntimeError::InvalidJavaMajor(exact_major.to_string()));
        }
        let os_name = if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else {
            return Err(RuntimeError::ProvisionFailed(
                "Managed Adoptium JDK provisioning currently supports Windows and Linux"
                    .to_string(),
            ));
        };
        let arch = if cfg!(target_arch = "x86_64") {
            "x64"
        } else if cfg!(target_arch = "aarch64") {
            "aarch64"
        } else {
            return Err(RuntimeError::ProvisionFailed(format!(
                "Unsupported CPU architecture: {}",
                std::env::consts::ARCH
            )));
        };

        let url = format!(
            "{ADOPTIUM_ASSETS_URL}/{exact_major}/hotspot?heap_size=normal&image_type=jdk&jvm_impl=hotspot&project=jdk&vendor=eclipse&os={os_name}&architecture={arch}"
        );
        let raw = client.get_text(&url).await?;
        let payload: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
            RuntimeError::ProvisionFailed(format!("Invalid Adoptium response: {e}"))
        })?;

        let arr = payload.as_array().ok_or_else(|| {
            RuntimeError::ProvisionFailed("Adoptium response is not an array".to_string())
        })?;
        let asset = arr.first().ok_or_else(|| {
            RuntimeError::ProvisionFailed(format!(
                "No Adoptium Temurin JDK {exact_major} release found for {os_name}/{arch}"
            ))
        })?;

        let pkg = asset.pointer("/binary/package").ok_or_else(|| {
            RuntimeError::ProvisionFailed("Adoptium asset is missing binary.package".to_string())
        })?;

        let download_url = pkg
            .get("link")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let sha256 = pkg
            .get("checksum")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let size = pkg.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
        let filename = pkg
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("temurin-jdk.zip")
            .to_string();
        let release_name = asset
            .get("release_name")
            .and_then(|v| v.as_str())
            .unwrap_or(&filename)
            .to_string();

        if download_url.is_empty() || sha256.len() != 64 {
            return Err(RuntimeError::ProvisionFailed(
                "Adoptium package is missing download link or SHA-256 checksum".to_string(),
            ));
        }

        Ok(AdoptiumRelease {
            major: exact_major,
            release_name,
            url: download_url,
            sha256,
            size,
            filename,
        })
    }

    pub async fn ensure_managed(
        runtimes_root: &Path,
        required_major: u32,
        client: &DownloadClient,
    ) -> Result<JavaInstallation> {
        Self::ensure_managed_with_progress(runtimes_root, required_major, client, |_, _| {}).await
    }

    pub async fn ensure_managed_with_progress<F>(
        runtimes_root: &Path,
        required_major: u32,
        client: &DownloadClient,
        on_bytes: F,
    ) -> Result<JavaInstallation>
    where
        F: Fn(u64, u64) + Send + Sync + 'static,
    {
        let managed_major = JavaMajorPolicy::resolve(required_major)?;
        Self::install_managed_exact_with_progress(
            runtimes_root,
            managed_major,
            false,
            client,
            on_bytes,
        )
        .await
    }

    pub async fn install_managed_exact(
        runtimes_root: &Path,
        exact_major: u32,
        force: bool,
        client: &DownloadClient,
    ) -> Result<JavaInstallation> {
        Self::install_managed_exact_with_progress(
            runtimes_root,
            exact_major,
            force,
            client,
            |_, _| {},
        )
        .await
    }

    pub async fn install_managed_exact_with_progress<F>(
        runtimes_root: &Path,
        exact_major: u32,
        force: bool,
        client: &DownloadClient,
        on_bytes: F,
    ) -> Result<JavaInstallation>
    where
        F: Fn(u64, u64) + Send + Sync + 'static,
    {
        if exact_major < 8 {
            return Err(RuntimeError::InvalidJavaMajor(exact_major.to_string()));
        }
        if !force {
            let existing_exe = Self::managed_executable(runtimes_root, exact_major);
            if existing_exe.is_file() {
                if let Some(info) = JavaDiscovery::probe(&existing_exe) {
                    return Ok(info);
                }
            }
        }

        let release = Self::fetch_latest_adoptium_exact(client, exact_major).await?;
        info!(
            major = exact_major,
            release = %release.release_name,
            "Downloading managed Eclipse Temurin JDK"
        );

        let archives_dir = runtimes_root.join(".archives");
        fs::create_dir_all(&archives_dir)?;
        let archive_path = archives_dir.join(&release.filename);

        let req = DownloadRequest::new(&release.url, &archive_path).with_size(release.size);
        client.download_file_with_progress(&req, on_bytes).await?;

        let actual_sha256 = compute_sha256(&archive_path)?;
        if actual_sha256 != release.sha256 {
            let _ = fs::remove_file(&archive_path);
            return Err(RuntimeError::ProvisionFailed(format!(
                "SHA-256 mismatch for {}: expected {}, got {}",
                release.filename, release.sha256, actual_sha256
            )));
        }

        let target_dir = Self::managed_dir(runtimes_root, exact_major);
        if target_dir.exists() {
            fs::remove_dir_all(&target_dir)?;
        }
        extract_jdk_zip(&archive_path, &target_dir)?;
        let _ = fs::remove_file(&archive_path);

        let exe = Self::managed_executable(runtimes_root, exact_major);
        let installed = JavaDiscovery::probe(&exe).ok_or_else(|| {
            RuntimeError::ProvisionFailed(format!(
                "Extracted JDK at {} did not produce a working java executable",
                target_dir.display()
            ))
        })?;
        JavaRecoveryDiagnostics::record(
            "java.provision.installed",
            format!(
                "Installed managed Java {} ({}) at {}",
                installed.major,
                installed.version_string,
                installed.path.display()
            ),
        );
        Ok(installed)
    }
}

fn compute_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    Ok(out)
}

fn extract_jdk_zip(archive_path: &Path, target_dir: &Path) -> Result<()> {
    fs::create_dir_all(target_dir)?;
    let file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let normalized = entry.name().replace('\\', "/");
        let parts: Vec<&str> = Path::new(&normalized)
            .components()
            .filter_map(|c| match c {
                Component::Normal(p) => p.to_str(),
                _ => None,
            })
            .collect();
        // Adoptium zips wrap everything inside a single top-level `jdk-17.x.x+y/` directory.
        if parts.len() <= 1 {
            continue;
        }
        let mut dest = target_dir.to_path_buf();
        for p in &parts[1..] {
            dest.push(p);
        }
        if entry.is_dir() {
            fs::create_dir_all(&dest)?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&dest)?;
        io::copy(&mut entry, &mut out)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_adoptium_latest_feature_release() {
        let json = r#"{"available_releases":[8,11,17,21,25,26],"most_recent_feature_release":26}"#;
        assert_eq!(
            JavaProvisioner::parse_latest_feature_release(json).unwrap(),
            26
        );
        let fallback = r#"{"available_releases":[8,17,21,25]}"#;
        assert_eq!(
            JavaProvisioner::parse_latest_feature_release(fallback).unwrap(),
            25
        );
        assert!(JavaProvisioner::parse_latest_feature_release("{}").is_err());
    }
}
