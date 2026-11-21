pub mod installer;
pub mod version;

pub use installer::{candidate_download_urls, StagedUpdate, UpdateInstaller};
pub use version::{LauncherVersion, PrereleaseKind};

use mcw_models::GITHUB_REPOSITORY;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

/// Updater enabled for MCW Launcher: Rust Edition (v1.0.0+).
pub const UPDATES_ENABLED: bool = true;

/// Primary Cloudflare Edge CDN manifest for MCW Launcher: Rust Edition.
pub const CLOUDFLARE_RUST_MANIFEST_URL: &str =
    "https://mcw-download.pages.dev/releases/rust/manifest.json";

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("Launcher updates are currently disabled for MCW Launcher: Rust Edition.")]
    Disabled,
    #[error("Invalid version string: {0}")]
    InvalidVersion(String),
    #[error("Release update check failed: {0}")]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedAsset {
    pub url: String,
    pub mirror_urls: Vec<String>,
    pub name: Option<String>,
    pub size: Option<u64>,
    pub sha256: Option<String>,
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
    pub mirror_urls: Vec<String>,
    #[serde(default)]
    pub sha256: Option<String>,
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
    cdn_manifest_url: String,
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
        let cdn_manifest_url = std::env::var("MCW_UPDATE_CDN_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| CLOUDFLARE_RUST_MANIFEST_URL.to_string());
        Ok(Self {
            http,
            github_repo,
            cdn_manifest_url,
        })
    }

    /// Check Cloudflare Edge CDN (`mcw-download.pages.dev/releases/rust/manifest.json`)
    /// with automatic failover to GitHub Releases (`mahiru7229/mcw-launcher-rust`).
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

        let mut best = None;
        if let Ok(cdn_list) = self.fetch_cloudflare_releases().await {
            if let Some(candidate) = select_best_release(&cdn_list, channel) {
                if candidate.download_url.is_some() {
                    best = Some(candidate);
                }
            }
        }

        if best.is_none() {
            let gh_list = self
                .fetch_github_releases()
                .await
                .map_err(|e| UpdateError::GithubFailed(e.to_string()))?;
            best = select_best_release(&gh_list, channel);
        }

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

    async fn fetch_cloudflare_releases(&self) -> Result<Vec<ReleaseInfo>> {
        let resp = self
            .http
            .get(&self.cdn_manifest_url)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(8))
            .send()
            .await?
            .error_for_status()?;
        let val: serde_json::Value = resp.json().await?;
        let items = parse_cdn_manifest_items(&val);
        Ok(parse_release_entries(&items, "cloudflare-cdn"))
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

pub(crate) fn parse_cdn_manifest_items(val: &serde_json::Value) -> Vec<serde_json::Value> {
    if let Some(arr) = val.as_array() {
        return arr.clone();
    }
    if let Some(arr) = val.get("releases").and_then(|v| v.as_array()) {
        return arr.clone();
    }
    if let Some(arr) = val
        .get("editions")
        .and_then(|e| e.get("rust"))
        .and_then(|r| r.get("releases"))
        .and_then(|v| v.as_array())
    {
        return arr.clone();
    }
    Vec::new()
}

pub(crate) fn normalize_sha256(raw: Option<&str>) -> Option<String> {
    let clean = raw?.trim().to_ascii_lowercase();
    let hex = clean.strip_prefix("sha256:").unwrap_or(&clean);
    if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(hex.to_string())
    } else {
        None
    }
}

fn is_non_binary_asset(lower_name: &str) -> bool {
    lower_name.ends_with(".sha256")
        || lower_name.ends_with(".sha256sum")
        || lower_name.ends_with(".sha1")
        || lower_name.ends_with(".md5")
        || lower_name.ends_with(".txt")
        || lower_name.ends_with(".sig")
        || lower_name.ends_with(".asc")
        || lower_name.contains("legacy")
        || lower_name.contains("bridge")
}

pub fn select_platform_asset(assets: &[serde_json::Value]) -> Option<SelectedAsset> {
    if assets.is_empty() {
        return None;
    }

    let extract = |asset: &serde_json::Value| -> Option<SelectedAsset> {
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
        let lower_name = name.as_deref().unwrap_or(&url).to_ascii_lowercase();
        if is_non_binary_asset(&lower_name) {
            return None;
        }

        let mut mirror_urls = Vec::new();
        if let Some(arr) = asset.get("mirror_urls").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(m) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                    if m != url && !mirror_urls.iter().any(|x| x == m) {
                        mirror_urls.push(m.to_string());
                    }
                }
            }
        }
        let sha256 = normalize_sha256(
            asset
                .get("sha256")
                .or_else(|| asset.get("digest"))
                .and_then(|v| v.as_str()),
        );
        let size = asset.get("size").and_then(|v| v.as_u64());
        Some(SelectedAsset {
            url,
            mirror_urls,
            name,
            size,
            sha256,
        })
    };

    let scored = assets.iter().filter_map(|asset| {
        let selected = extract(asset)?;
        let lower_name = selected
            .name
            .as_deref()
            .unwrap_or(&selected.url)
            .to_ascii_lowercase();

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

        Some((score, selected))
    });

    scored.max_by_key(|(s, _)| *s).map(|(_, sel)| sel)
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
        let picked = item
            .get("assets")
            .and_then(|a| a.as_array())
            .and_then(|arr| select_platform_asset(arr));

        let (download_url, mirror_urls, asset_name, asset_size, sha256) = match picked {
            Some(sel) => (
                Some(sel.url),
                sel.mirror_urls,
                sel.name,
                sel.size,
                sel.sha256,
            ),
            None => (None, Vec::new(), None, None, None),
        };

        out.push(ReleaseInfo {
            version: tag.trim_start_matches(['v', 'V']).to_string(),
            tag_name: tag,
            prerelease,
            source: source.to_string(),
            notes,
            html_url,
            download_url,
            mirror_urls,
            sha256,
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
    let eligible: Vec<&ReleaseInfo> = releases
        .iter()
        .filter(|r| match channel {
            UpdateChannel::Stable => !r.prerelease,
            UpdateChannel::Beta => true,
        })
        .collect();

    let with_assets = eligible
        .iter()
        .copied()
        .filter(|r| r.download_url.is_some())
        .filter_map(|r| LauncherVersion::parse(&r.version).map(|v| (v, r)))
        .max_by(|(va, _), (vb, _)| va.cmp(vb))
        .map(|(_, r)| r.clone());

    with_assets.or_else(|| {
        eligible
            .into_iter()
            .filter_map(|r| LauncherVersion::parse(&r.version).map(|v| (v, r)))
            .max_by(|(va, _), (vb, _)| va.cmp(vb))
            .map(|(_, r)| r.clone())
    })
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
                mirror_urls: Vec::new(),
                sha256: None,
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
                mirror_urls: Vec::new(),
                sha256: None,
                asset_name: None,
                asset_size: None,
            },
        ];

        let stable = select_best_release(&releases, UpdateChannel::Stable).unwrap();
        assert_eq!(stable.version, "0.8.0");

        let beta = select_best_release(&releases, UpdateChannel::Beta).unwrap();
        assert_eq!(beta.version, "0.9.0-beta.1");
    }

    #[test]
    fn updates_enabled_is_true_for_v1_release() {
        const {
            assert!(UPDATES_ENABLED);
        }
        assert!(CLOUDFLARE_RUST_MANIFEST_URL.contains("mcw-download.pages.dev/releases/rust/"));
    }

    #[test]
    fn parses_cloudflare_cdn_rust_manifest_with_mirrors_and_sha256() {
        let manifest = json!({
            "schema_version": 1,
            "releases": [
                {
                    "tag_name": "v1.0.0",
                    "name": "MCW Launcher: Rust Edition v1.0.0",
                    "prerelease": false,
                    "draft": false,
                    "assets": [
                        {
                            "name": "SHA256SUMS.txt",
                            "size": 192,
                            "browser_download_url": "https://mcw-download.pages.dev/releases/rust/v1.0.0/SHA256SUMS.txt",
                            "sha256": "08d7a5cc01745d68c298af04a1d217726ead9ce4d593519d839c95dc892f8344"
                        },
                        {
                            "name": "mcw-launcher-windows-x64.zip",
                            "size": 10193004,
                            "browser_download_url": "https://mcw-download.pages.dev/releases/rust/v1.0.0/mcw-launcher-windows-x64.zip",
                            "mirror_urls": [
                                "https://github.com/mahiru7229/mcw-launcher-rust/releases/download/v1.0.0/mcw-launcher-windows-x64.zip"
                            ],
                            "sha256": "8a02bb9a19cf05898aa4bc222e4d3dd40e4cd2abd48e5e66089870cbbcc23593"
                        },
                        {
                            "name": "mcw-launcher-linux-x64.tar.gz",
                            "size": 13134698,
                            "browser_download_url": "https://mcw-download.pages.dev/releases/rust/v1.0.0/mcw-launcher-linux-x64.tar.gz",
                            "mirror_urls": [
                                "https://github.com/mahiru7229/mcw-launcher-rust/releases/download/v1.0.0/mcw-launcher-linux-x64.tar.gz"
                            ],
                            "digest": "sha256:64b5d4988749046cd9995bea4ba6ba298df0abc1c8440c3091399aaad8e2f250"
                        }
                    ]
                }
            ]
        });

        let items = parse_cdn_manifest_items(&manifest);
        let parsed = parse_release_entries(&items, "cloudflare-cdn");
        assert_eq!(parsed.len(), 1);
        let rel = select_best_release(&parsed, UpdateChannel::Stable).unwrap();
        assert_eq!(rel.source, "cloudflare-cdn");
        #[cfg(target_os = "windows")]
        {
            assert_eq!(
                rel.download_url.as_deref(),
                Some("https://mcw-download.pages.dev/releases/rust/v1.0.0/mcw-launcher-windows-x64.zip")
            );
            assert_eq!(
                rel.mirror_urls,
                vec!["https://github.com/mahiru7229/mcw-launcher-rust/releases/download/v1.0.0/mcw-launcher-windows-x64.zip"]
            );
            assert_eq!(
                rel.sha256.as_deref(),
                Some("8a02bb9a19cf05898aa4bc222e4d3dd40e4cd2abd48e5e66089870cbbcc23593")
            );
        }
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                rel.download_url.as_deref(),
                Some("https://mcw-download.pages.dev/releases/rust/v1.0.0/mcw-launcher-linux-x64.tar.gz")
            );
            assert_eq!(
                rel.sha256.as_deref(),
                Some("64b5d4988749046cd9995bea4ba6ba298df0abc1c8440c3091399aaad8e2f250")
            );
        }
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
            json!({
                "name": "SHA256SUMS.txt",
                "browser_download_url": "https://example.com/SHA256SUMS.txt",
                "size": 64
            }),
        ];
        let sel = select_platform_asset(&assets).unwrap();
        #[cfg(target_os = "windows")]
        {
            assert_eq!(sel.url, "https://example.com/win.zip");
            assert_eq!(
                sel.name.as_deref(),
                Some("mcw-launcher-rust-windows-x64.zip")
            );
            assert_eq!(sel.size, Some(200));
        }
        #[cfg(target_os = "linux")]
        {
            assert_eq!(sel.url, "https://example.com/linux.tar.gz");
            assert_eq!(
                sel.name.as_deref(),
                Some("mcw-launcher-rust-linux-x64.tar.gz")
            );
            assert_eq!(sel.size, Some(100));
        }
    }
}
