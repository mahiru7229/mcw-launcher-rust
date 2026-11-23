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
    const PROBE_URLS: [&'static str; 4] = [
        "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
        "https://api.modrinth.com/v2/tag/loader",
        "https://mcw-download.pages.dev/releases/rust/manifest.json",
        "https://api.github.com/zen",
    ];

    pub async fn probe(client: &DownloadClient) -> ConnectivityStatus {
        Self::probe_urls(client, &Self::PROBE_URLS, Duration::from_millis(3500)).await
    }

    pub async fn probe_urls(
        client: &DownloadClient,
        urls: &[&str],
        timeout: Duration,
    ) -> ConnectivityStatus {
        if urls.is_empty() {
            return ConnectivityStatus::Offline;
        }
        let checks = urls.iter().map(|&url| {
            let req = client.http().head(url).timeout(timeout);
            Box::pin(async move {
                match req.send().await {
                    Ok(resp) if resp.status().is_success() || resp.status().is_redirection() => {
                        Ok(())
                    }
                    _ => Err(()),
                }
            })
        });
        match futures::future::select_ok(checks).await {
            Ok(((), _)) => ConnectivityStatus::Online,
            Err(_) => ConnectivityStatus::Offline,
        }
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
