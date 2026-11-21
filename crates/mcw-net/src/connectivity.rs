use crate::DownloadClient;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectivityStatus {
    #[default]
    Unknown,
    Online,
    Offline,
}

impl ConnectivityStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Online => "online",
            Self::Offline => "offline",
        }
    }
}

/// Lightweight network reachability monitor mirroring Python `ConnectivityMonitor`.
pub struct ConnectivityMonitor;

impl ConnectivityMonitor {
    const PROBE_URLS: [&'static str; 2] = [
        "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
        "https://api.modrinth.com/v2/tag/loader",
    ];

    pub async fn probe(client: &DownloadClient) -> ConnectivityStatus {
        Self::probe_urls(client, &Self::PROBE_URLS, Duration::from_secs(3)).await
    }

    pub async fn probe_urls(
        client: &DownloadClient,
        urls: &[&str],
        timeout: Duration,
    ) -> ConnectivityStatus {
        for &url in urls {
            let req = client.http().head(url).timeout(timeout);
            if let Ok(resp) = req.send().await {
                if resp.status().is_success() || resp.status().is_redirection() {
                    return ConnectivityStatus::Online;
                }
            }
        }
        ConnectivityStatus::Offline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_offline_for_unreachable_local_endpoint() {
        let client = DownloadClient::new().unwrap();
        let status = ConnectivityMonitor::probe_urls(
            &client,
            &["http://127.0.0.1:9/unreachable"],
            Duration::from_millis(250),
        )
        .await;
        assert_eq!(status, ConnectivityStatus::Offline);
    }
}
