use crate::StoredAccount;
use mcw_models::account::AccountSource;
use mcw_storage::atomic_write_bytes;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_SKIN_BYTES: usize = 4 * 1024 * 1024;
const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

pub struct AccountSkinManager;

impl AccountSkinManager {
    pub fn normalize_uuid(uuid: &str) -> String {
        uuid.trim()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase()
    }

    pub fn texture_path(accounts_root: &Path, uuid: &str) -> PathBuf {
        let norm = Self::normalize_uuid(uuid);
        accounts_root.join("skins").join(format!("{norm}.png"))
    }

    /// Validate Mojang skin URL: require `textures.minecraft.net` host and enforce HTTPS.
    pub fn validate_skin_url(raw: &str) -> Option<String> {
        let mut parsed = url::Url::parse(raw.trim()).ok()?;
        let host = parsed.host_str()?.to_ascii_lowercase();
        if host != "textures.minecraft.net" {
            return None;
        }
        if parsed.scheme() == "http" {
            let _ = parsed.set_scheme("https");
        }
        if parsed.scheme() != "https" {
            return None;
        }
        Some(parsed.to_string())
    }

    pub fn cached_texture(accounts_root: &Path, account: &StoredAccount) -> Option<PathBuf> {
        if account.account_type == AccountSource::Offline {
            return None;
        }
        let path = Self::texture_path(accounts_root, &account.uuid);
        let meta = fs::metadata(&path).ok()?;
        if meta.is_file() && meta.len() > 8 {
            Some(path)
        } else {
            None
        }
    }

    pub async fn cache_texture(accounts_root: &Path, account: &StoredAccount) -> Option<PathBuf> {
        if account.account_type == AccountSource::Offline {
            return None;
        }
        if let Some(existing) = Self::cached_texture(accounts_root, account) {
            return Some(existing);
        }
        let skin_url = account
            .skin_url
            .as_deref()
            .and_then(Self::validate_skin_url)?;
        let http = reqwest::Client::builder()
            .user_agent(mcw_models::user_agent())
            .timeout(Duration::from_secs(15))
            .build()
            .ok()?;
        let resp = http
            .get(&skin_url)
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        if let Some(len) = resp.content_length() {
            if len as usize > MAX_SKIN_BYTES {
                return None;
            }
        }
        let bytes = resp.bytes().await.ok()?;
        if bytes.len() <= 8 || bytes.len() > MAX_SKIN_BYTES || !bytes.starts_with(PNG_MAGIC) {
            return None;
        }
        let target = Self::texture_path(accounts_root, &account.uuid);
        atomic_write_bytes(&target, &bytes).ok()?;
        Some(target)
    }

    pub fn remove_cached_texture(accounts_root: &Path, uuid: &str) {
        let path = Self::texture_path(accounts_root, uuid);
        let _ = fs::remove_file(path);
    }

    pub fn default_skin_variant(uuid_str: &str) -> &'static str {
        uuid::Uuid::parse_str(uuid_str.trim())
            .map(crate::OfflineAuth::default_skin_variant)
            .unwrap_or("classic")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn normalizes_uuid_and_validates_textures_host() {
        assert_eq!(
            AccountSkinManager::normalize_uuid("83A14A8A-2501-4A7A-B178-758BB1847F82"),
            "83a14a8a25014a7ab178758bb1847f82"
        );
        assert_eq!(
            AccountSkinManager::validate_skin_url("http://textures.minecraft.net/texture/abc123")
                .as_deref(),
            Some("https://textures.minecraft.net/texture/abc123")
        );
        assert!(
            AccountSkinManager::validate_skin_url("https://evil.example.com/skin.png").is_none()
        );
    }

    #[test]
    fn offline_account_never_uses_cached_network_skin() {
        let dir = tempdir().unwrap();
        let offline = StoredAccount {
            uuid: "a01e3843e5213998958af459800e4d11".to_string(),
            username: "Steve".to_string(),
            account_type: AccountSource::Offline,
            skin_url: Some("https://textures.minecraft.net/texture/fake".to_string()),
            active: true,
        };
        assert!(AccountSkinManager::cached_texture(dir.path(), &offline).is_none());

        let msa = StoredAccount {
            account_type: AccountSource::Microsoft,
            ..offline
        };
        let target = AccountSkinManager::texture_path(dir.path(), &msa.uuid);
        atomic_write_bytes(&target, b"\x89PNG\r\n\x1a\n1234").unwrap();
        assert_eq!(
            AccountSkinManager::cached_texture(dir.path(), &msa),
            Some(target)
        );
        AccountSkinManager::remove_cached_texture(dir.path(), &msa.uuid);
        assert!(AccountSkinManager::cached_texture(dir.path(), &msa).is_none());
    }
}
