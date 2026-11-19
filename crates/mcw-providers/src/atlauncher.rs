use crate::cache::ApiCache;
use crate::{ProviderError, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

const ATLAUNCHER_PACKS_URL: &str =
    "https://download.nodecdn.net/containers/atl/launcher/json/packsnew.json";
const ATL_TTL_SECS: u64 = 600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtLauncherPack {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "safeName", default)]
    pub safe_name: String,
}

pub struct AtLauncherClient {
    http: reqwest::Client,
    cache: Option<Arc<ApiCache>>,
}

impl AtLauncherClient {
    pub fn new(cache: Option<Arc<ApiCache>>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(mcw_models::user_agent())
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self { http, cache })
    }

    pub async fn list_packs(&self) -> Result<Vec<AtLauncherPack>> {
        let key = "atlauncher:packsnew";
        if let Some(ref cache) = self.cache {
            if let Some(hit) = cache.get(key) {
                return Ok(serde_json::from_str(&hit)?);
            }
        }
        let resp = self.http.get(ATLAUNCHER_PACKS_URL).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(ProviderError::ApiError {
                status: status.as_u16(),
                message: text,
            });
        }
        if let Some(ref cache) = self.cache {
            let _ = cache.set(key, &text, ATL_TTL_SECS);
        }
        Ok(serde_json::from_str(&text)?)
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<AtLauncherPack>> {
        let q = query.trim().to_ascii_lowercase();
        let packs = self.list_packs().await?;
        Ok(packs
            .into_iter()
            .filter(|p| {
                q.is_empty()
                    || p.name.to_ascii_lowercase().contains(&q)
                    || p.description.to_ascii_lowercase().contains(&q)
            })
            .take(limit)
            .collect())
    }
}
