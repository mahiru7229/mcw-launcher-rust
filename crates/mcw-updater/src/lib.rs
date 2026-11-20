pub mod installer;
pub mod version;

pub use installer::{StagedUpdate, UpdateInstaller};
pub use version::{LauncherVersion, PrereleaseKind};

use mcw_models::GITHUB_REPOSITORY;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

/// Updater is intentionally disabled for MCW Launcher: Rust Edition until
/// Rust Edition's own release pipeline replaces the legacy Python feed.
pub const UPDATES_ENABLED: bool = false;

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("Launcher updates are currently disabled for MCW Launcher: Rust Edition.")]
    Disabled,
    #[error("Invalid version string: {0}")]
    InvalidVersion(String),
    #[error("GitHub Releases update check failed: {0}")]
    GithubFailed(String),
    #[error("Update installation failed: {0}")]
    InstallFailed(String),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, UpdateError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    Stable,
    Beta,
}

impl UpdateChannel {
    pub fn parse(s: &str) -> Self {
        if s.trim().eq_ignore_ascii_case("beta") {
            Self::Beta
        } else {
            Self::Stable
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub version: String,
    pub prerelease: bool,
    pub source: String,
    pub notes: String,
    pub html_url: String,
    pub download_url: Option<String>,
    #[serde(default)]
    pub asset_name: Option<String>,
    #[serde(default)]
    pub asset_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub channel: UpdateChannel,
    pub update_available: bool,
    pub latest: Option<ReleaseInfo>,
}

pub struct UpdateChecker {
    http: reqwest::Client,
    github_repo: String,
}

impl UpdateChecker {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(mcw_models::user_agent())
            .timeout(Duration::from_secs(15))
            .build()?;
        let github_repo = std::env::var("MCW_GITHUB_REPOSITORY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| GITHUB_REPOSITORY.to_string());
        Ok(Self { http, github_repo })
    }

    /// Check GitHub Releases (`mahiru7229/mcw-launcher-rust`) on the requested channel.
    /// Returns `Err(UpdateError::Disabled)` while `UPDATES_ENABLED == false`.
    pub async fn check(
        &self,
        current_version: &str,
        channel: UpdateChannel,
    ) -> Result<UpdateCheckResult> {
        if !UPDATES_ENABLED {
            return Err(UpdateError::Disabled);
        }
        let current = LauncherVersion::parse(current_version)
            .ok_or_else(|| UpdateError::InvalidVersion(current_version.to_string()))?;

        let list = self
            .fetch_github_releases()
            .await
            .map_err(|e| UpdateError::GithubFailed(e.to_string()))?;
        let best = select_best_release(&list, channel);

        let update_available = best
            .as_ref()
            .and_then(|r| LauncherVersion::parse(&r.version))
            .map(|latest_ver| latest_ver > current)
            .unwrap_or(false);

        Ok(UpdateCheckResult {
            current_version: current_version.to_string(),
            channel,
            update_available,
            latest: best,
        })
    }

    async fn fetch_github_releases(&self) -> Result<Vec<ReleaseInfo>> {
        let url = format!(
            "https://api.github.com/repos/{}/releases?per_page=20",
            self.github_repo
        );
        let resp = self
            .http
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;
        let resp = resp.error_for_status()?;
        let items: Vec<serde_json::Value> = resp.json().await?;
        Ok(parse_release_entries(&items, "github"))
    }
}

pub fn select_platform_asset(
    assets: &[serde_json::Value],
) -> Option<(String, Option<String>, Option<u64>)> {
    if assets.is_empty() {
        return None;
    }

    let extract = |asset: &serde_json::Value| -> Option<(String, Option<String>, Option<u64>)> {
        let url = asset
            .get("browser_download_url")
            .or_else(|| asset.get("url"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())?
            .to_string();
        let name = asset
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                url.split(['/', '?'])
                    .rfind(|seg| !seg.is_empty())
                    .map(String::from)
            });
        let size = asset.get("size").and_then(|v| v.as_u64());
        Some((url, name, size))
    };

    let scored = assets.iter().filter_map(|asset| {
        let (url, name, size) = extract(asset)?;
        let lower_name = name.as_deref().unwrap_or(&url).to_ascii_lowercase();

        #[cfg(target_os = "windows")]
        let score: u32 = {
            let is_win = lower_name.contains("windows")
                || lower_name.contains("win64")
                || lower_name.contains("win-x64")
                || lower_name.contains("win_x64");
            let is_zip = lower_name.ends_with(".zip");
            let is_exe = lower_name.ends_with(".exe");
            if is_win && is_zip {
                100
            } else if is_win && is_exe {
                95
            } else if is_win {
                85
            } else if is_zip && !lower_name.contains("linux") && !lower_name.contains("mac") {
                60
            } else if is_exe {
                55
            } else {
                10
            }
        };

        #[cfg(target_os = "linux")]
        let score: u32 = {
            let is_lin = lower_name.contains("linux");
            let is_tar = lower_name.ends_with(".tar.gz") || lower_name.ends_with(".tgz");
            let is_zip = lower_name.ends_with(".zip");
            let is_appimage = lower_name.ends_with(".appimage");
            if is_lin && is_tar {
                100
            } else if is_lin && is_zip {
                95
            } else if is_lin && is_appimage {
                90
            } else if is_lin {
                80
            } else if is_tar || is_zip {
                40
            } else {
                10
            }
        };

        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        let score: u32 = 50;

        Some((score, (url, name, size)))
    });

    scored.max_by_key(|(s, _)| *s).map(|(_, tuple)| tuple)
}

fn parse_release_entries(items: &[serde_json::Value], source: &str) -> Vec<ReleaseInfo> {
    let mut out = Vec::new();
    for item in items {
        if item.get("draft").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }
        let tag = item
            .get("tag_name")
            .or_else(|| item.get("version"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_string();
        if tag.is_empty() || LauncherVersion::parse(&tag).is_none() {
            continue;
        }
        let prerelease = item
            .get("prerelease")
            .and_then(|v| v.as_bool())
            .unwrap_or_else(|| {
                LauncherVersion::parse(&tag)
                    .map(|v| v.is_prerelease())
                    .unwrap_or(false)
            });
        let notes = item
            .get("body")
            .or_else(|| item.get("notes"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let html_url = item
            .get("html_url")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let (download_url, asset_name, asset_size) = item
            .get("assets")
            .and_then(|a| a.as_array())
            .and_then(|arr| select_platform_asset(arr))
            .map(|(u, n, s)| (Some(u), n, s))
            .unwrap_or((None, None, None));

        out.push(ReleaseInfo {
            version: tag.trim_start_matches(['v', 'V']).to_string(),
            tag_name: tag,
            prerelease,
            source: source.to_string(),
            notes,
            html_url,
            download_url,
            asset_name,
            asset_size,
        });
    }
    out
}

pub fn select_best_release(
    releases: &[ReleaseInfo],
    channel: UpdateChannel,
) -> Option<ReleaseInfo> {
    releases
        .iter()
        .filter(|r| match channel {
            UpdateChannel::Stable => !r.prerelease,
            UpdateChannel::Beta => true,
        })
        .filter_map(|r| LauncherVersion::parse(&r.version).map(|v| (v, r)))
        .max_by(|(va, _), (vb, _)| va.cmp(vb))
        .map(|(_, r)| r.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn selects_highest_rust_edition_release_respecting_channel() {
        let releases = vec![
            ReleaseInfo {
                tag_name: "v0.8.0".to_string(),
                version: "0.8.0".to_string(),
                prerelease: false,
                source: "github".to_string(),
                notes: String::new(),
                html_url: String::new(),
                download_url: None,
                asset_name: None,
                asset_size: None,
            },
            ReleaseInfo {
                tag_name: "v0.9.0-beta.1".to_string(),
                version: "0.9.0-beta.1".to_string(),
                prerelease: true,
                source: "github".to_string(),
                notes: String::new(),
                html_url: String::new(),
                download_url: None,
                asset_name: None,
                asset_size: None,
            },
        ];

        let stable = select_best_release(&releases, UpdateChannel::Stable).unwrap();
        assert_eq!(stable.version, "0.8.0");

        let beta = select_best_release(&releases, UpdateChannel::Beta).unwrap();
        assert_eq!(beta.version, "0.9.0-beta.1");
    }

    #[tokio::test]
    async fn update_checker_returns_disabled_error_when_disabled() {
        const {
            assert!(!UPDATES_ENABLED);
        }
        let checker = UpdateChecker::new().unwrap();
        let err = checker
            .check("0.9.0-beta.1", UpdateChannel::Beta)
            .await
            .unwrap_err();
        assert!(matches!(err, UpdateError::Disabled));
    }

    #[test]
    fn compares_semver_build_tags_cleanly() {
        let b1 = LauncherVersion::parse("v0.9.0-beta.1").unwrap();
        let b2 = LauncherVersion::parse("v0.9.0-beta.2").unwrap();
        assert!(b2 > b1);
    }

    #[test]
    fn prefers_current_platform_archive_asset() {
        let assets = vec![
            json!({
                "name": "mcw-launcher-rust-linux-x64.tar.gz",
                "browser_download_url": "https://example.com/linux.tar.gz",
                "size": 100
            }),
            json!({
                "name": "mcw-launcher-rust-windows-x64.zip",
                "browser_download_url": "https://example.com/win.zip",
                "size": 200
            }),
        ];
        let (url, name, size) = select_platform_asset(&assets).unwrap();
        #[cfg(target_os = "windows")]
        {
            assert_eq!(url, "https://example.com/win.zip");
            assert_eq!(name.as_deref(), Some("mcw-launcher-rust-windows-x64.zip"));
            assert_eq!(size, Some(200));
        }
        #[cfg(target_os = "linux")]
        {
            assert_eq!(url, "https://example.com/linux.tar.gz");
            assert_eq!(name.as_deref(), Some("mcw-launcher-rust-linux-x64.tar.gz"));
            assert_eq!(size, Some(100));
        }
    }
}
