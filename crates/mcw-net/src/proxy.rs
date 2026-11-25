use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Proxy mode: `"none"`, `"http"`, or `"socks5"`.
    pub mode: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            mode: "none".to_string(),
            host: String::new(),
            port: 8080,
            username: String::new(),
            password: String::new(),
        }
    }
}

impl ProxyConfig {
    pub fn normalized_mode(&self) -> &'static str {
        match self.mode.trim().to_ascii_lowercase().as_str() {
            "http" | "https" => "http",
            "socks5" | "socks" => "socks5",
            _ => "none",
        }
    }

    pub fn build_proxy_url(&self) -> Option<String> {
        let scheme = match self.normalized_mode() {
            "http" => "http",
            "socks5" => "socks5",
            _ => return None,
        };
        let mut clean = self.host.trim();
        for prefix in ["https://", "http://", "socks5://", "socks://"] {
            if clean.len() >= prefix.len() && clean[..prefix.len()].eq_ignore_ascii_case(prefix) {
                clean = &clean[prefix.len()..];
                break;
            }
        }
        let clean = clean.trim_end_matches('/').trim();
        if clean.is_empty() {
            return None;
        }
        let port = self.port.max(1);
        if clean.contains(':') && !clean.starts_with('[') && !clean.ends_with(']') {
            Some(format!("{scheme}://[{clean}]:{port}"))
        } else {
            Some(format!("{scheme}://{clean}:{port}"))
        }
    }
}

fn proxy_state() -> &'static Mutex<ProxyConfig> {
    static STATE: OnceLock<Mutex<ProxyConfig>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(ProxyConfig::default()))
}

pub fn set_global_proxy_config(cfg: ProxyConfig) {
    if let Ok(mut guard) = proxy_state().lock() {
        *guard = cfg;
    }
}

pub fn global_proxy_config() -> ProxyConfig {
    proxy_state().lock().map(|g| g.clone()).unwrap_or_default()
}

/// Applies the active global HTTP or SOCKS5 proxy configuration (if enabled) to a `reqwest::ClientBuilder`.
pub fn apply_global_proxy(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    let cfg = global_proxy_config();
    let Some(url) = cfg.build_proxy_url() else {
        return builder;
    };
    match reqwest::Proxy::all(&url) {
        Ok(mut proxy) => {
            let user = cfg.username.trim();
            if !user.is_empty() {
                proxy = proxy.basic_auth(user, &cfg.password);
            }
            builder.proxy(proxy)
        }
        Err(err) => {
            tracing::warn!(error = %err, url = %url, "Ignoring invalid proxy URL");
            builder
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_http_and_socks5_proxy_urls_and_ignores_disabled_or_empty_host() {
        let disabled = ProxyConfig {
            mode: "none".to_string(),
            host: "127.0.0.1".to_string(),
            port: 8080,
            ..Default::default()
        };
        assert_eq!(disabled.build_proxy_url(), None);

        let empty_host = ProxyConfig {
            mode: "http".to_string(),
            host: "   ".to_string(),
            port: 8080,
            ..Default::default()
        };
        assert_eq!(empty_host.build_proxy_url(), None);

        let http_cfg = ProxyConfig {
            mode: "http".to_string(),
            host: "http://127.0.0.1/".to_string(),
            port: 8888,
            ..Default::default()
        };
        assert_eq!(
            http_cfg.build_proxy_url().as_deref(),
            Some("http://127.0.0.1:8888")
        );

        let socks_cfg = ProxyConfig {
            mode: "socks5".to_string(),
            host: "127.0.0.1".to_string(),
            port: 1080,
            username: "alice".to_string(),
            password: "secret".to_string(),
        };
        assert_eq!(
            socks_cfg.build_proxy_url().as_deref(),
            Some("socks5://127.0.0.1:1080")
        );

        let ipv6_cfg = ProxyConfig {
            mode: "socks5".to_string(),
            host: "::1".to_string(),
            port: 1080,
            ..Default::default()
        };
        assert_eq!(
            ipv6_cfg.build_proxy_url().as_deref(),
            Some("socks5://[::1]:1080")
        );
    }
}
