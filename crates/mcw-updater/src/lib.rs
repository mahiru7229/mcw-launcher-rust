pub mod version;

pub use version::{LauncherVersion, PrereleaseKind};

use mcw_models::GITHUB_REPOSITORY;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;
use tracing::warn;

pub const DEFAULT_CLOUDFLARE_MANIFEST_URL: &str =
    "https://mcw-download.pages.dev/releases/manifest.json";

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("Invalid version string: {0}")]
    InvalidVersion(String),
    #[error("Both Cloudflare CDN and GitHub Releases failed: cloudflare=({cloudflare}), github=({github})")]
    AllSourcesFailed { cloudflare: String, github: String },
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
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
    cloudflare_url: String,
    github_repo: String,
}

impl UpdateChecker {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(mcw_models::user_agent())
            .timeout(Duration::from_secs(15))
            .build()?;
        let cloudflare_url = std::env::var("MCW_CLOUDFLARE_RELEASES_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_CLOUDFLARE_MANIFEST_URL.to_string());
        Ok(Self {
            http,
            cloudflare_url,
            github_repo: GITHUB_REPOSITORY.to_string(),
        })
    }

    /// Check for updates on the requested channel.
    /// Lesson #3: If Cloudflare CDN fails or returns empty, BOTH `stable` and `beta`
    /// channels automatically fall back to GitHub Releases API.
    pub async fn check(
        &self,
        current_version: &str,
        channel: UpdateChannel,
    ) -> Result<UpdateCheckResult> {
        let current = LauncherVersion::parse(current_version)
            .ok_or_else(|| UpdateError::InvalidVersion(current_version.to_string()))?;

        let mut cf_err: Option<String> = None;
        let cf_releases = match self.fetch_cloudflare_releases().await {
            Ok(list) if !list.is_empty() => Some(list),
            Ok(_) => {
                warn!("Cloudflare CDN release manifest was empty; falling back to GitHub Releases");
                cf_err = Some("empty manifest".to_string());
                None
            }
            Err(err) => {
                warn!(error = %err, "Cloudflare CDN check failed; falling back to GitHub Releases");
                cf_err = Some(err.to_string());
                None
            }
        };

        let mut candidates = cf_releases.unwrap_or_default();

        // Lesson #3: Fallback to GitHub Releases for BOTH stable and beta if Cloudflare
        // yielded no eligible release for this channel.
        let mut best = select_best_release(&candidates, channel);
        if best.is_none() {
            match self.fetch_github_releases().await {
                Ok(gh_list) => {
                    candidates = gh_list;
                    best = select_best_release(&candidates, channel);
                }
                Err(gh_err) => {
                    if let Some(cf) = cf_err {
                        return Err(UpdateError::AllSourcesFailed {
                            cloudflare: cf,
                            github: gh_err.to_string(),
                        });
                    }
                }
            }
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
            .get(&self.cloudflare_url)
            .header("Accept", "application/json")
            .send()
            .await?;
        let resp = resp.error_for_status()?;
        let val: serde_json::Value = resp.json().await?;
        let items = val
            .as_array()
            .cloned()
            .or_else(|| val.get("releases").and_then(|r| r.as_array()).cloned())
            .unwrap_or_default();
        Ok(parse_release_entries(&items, "cloudflare"))
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
        let download_url = item
            .get("assets")
            .and_then(|a| a.as_array())
            .and_then(|arr| arr.first())
            .and_then(|asset| asset.get("browser_download_url").or_else(|| asset.get("url")))
            .and_then(|v| v.as_str())
            .map(String::from);

        out.push(ReleaseInfo {
            version: tag.trim_start_matches(['v', 'V']).to_string(),
            tag_name: tag,
            prerelease,
            source: source.to_string(),
            notes,
            html_url,
            download_url,
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

    #[test]
    fn selects_highest_release_respecting_channel() {
        let releases = vec![
            ReleaseInfo {
                tag_name: "v1.8.7".to_string(),
                version: "1.8.7".to_string(),
                prerelease: false,
                source: "cloudflare".to_string(),
                notes: String::new(),
                html_url: String::new(),
                download_url: None,
            },
            ReleaseInfo {
                tag_name: "v2.0.0-beta.1".to_string(),
                version: "2.0.0-beta.1".to_string(),
                prerelease: true,
                source: "github".to_string(),
                notes: String::new(),
                html_url: String::new(),
                download_url: None,
            },
        ];

        let stable = select_best_release(&releases, UpdateChannel::Stable).unwrap();
        assert_eq!(stable.version, "1.8.7");

        let beta = select_best_release(&releases, UpdateChannel::Beta).unwrap();
        assert_eq!(beta.version, "2.0.0-beta.1");
    }
}
