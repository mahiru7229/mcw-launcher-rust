use crate::cache::ApiCache;
use crate::{ProviderError, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

const FTB_API_BASE: &str = "https://api.modpacks.ch/public";
const FTB_TTL_SECS: u64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtbPackSummary {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub synopsis: String,
    #[serde(default)]
    pub installs: u64,
}

pub struct FtbClient {
    http: reqwest::Client,
    cache: Option<Arc<ApiCache>>,
}

impl FtbClient {
    pub fn new(cache: Option<Arc<ApiCache>>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(mcw_models::user_agent())
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self { http, cache })
    }

    pub async fn search_ids(&self, term: &str, limit: u32) -> Result<Vec<u64>> {
        let mut url =
            Url::parse(&format!("{FTB_API_BASE}/modpack/search/{}", limit.min(50))).unwrap();
        url.query_pairs_mut().append_pair("term", term.trim());
        let raw = self.cached_get(url.as_str()).await?;
        let val: serde_json::Value = serde_json::from_str(&raw)?;
        let ids = val
            .get("packs")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
            .unwrap_or_default();
        Ok(ids)
    }

    pub async fn get_pack(&self, pack_id: u64) -> Result<FtbPackSummary> {
        let url = format!("{FTB_API_BASE}/modpack/{pack_id}");
        let raw = self.cached_get(&url).await?;
        Ok(serde_json::from_str(&raw)?)
    }

    async fn cached_get(&self, url: &str) -> Result<String> {
        let key = format!("ftb:{url}");
        if let Some(ref cache) = self.cache {
            if let Some(hit) = cache.get(&key) {
                return Ok(hit);
            }
        }
        let resp = self.http.get(url).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(ProviderError::ApiError {
                status: status.as_u16(),
                message: text,
            });
        }
        if let Some(ref cache) = self.cache {
            let _ = cache.set(&key, &text, FTB_TTL_SECS);
        }
        Ok(text)
    }
}
