use crate::StoredAccount;
use mcw_models::account::AccountSource;
use mcw_storage::atomic_write_bytes;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_SKIN_BYTES: usize = 4 * 1024 * 1024;
const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSkinEntry {
    pub file_name: String,
    pub display_name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

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

    /// Shared local Skin Library directory at `<project_root>/skins/`.
    pub fn library_dir(project_root: &Path) -> PathBuf {
        project_root.join("skins")
    }

    /// List PNG skin files saved inside `<project_root>/skins/`.
    pub fn list_library_skins(project_root: &Path) -> Vec<LocalSkinEntry> {
        let dir = Self::library_dir(project_root);
        let Ok(rd) = fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut entries: Vec<LocalSkinEntry> = rd
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                if !path.is_file() {
                    return None;
                }
                let file_name = e.file_name().to_str()?.to_string();
                if !file_name.to_ascii_lowercase().ends_with(".png") {
                    return None;
                }
                let size_bytes = e.metadata().ok()?.len();
                if size_bytes <= 8 {
                    return None;
                }
                let display_name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&file_name)
                    .to_string();
                Some(LocalSkinEntry {
                    file_name,
                    display_name,
                    path,
                    size_bytes,
                })
            })
            .collect();
        entries.sort_by(|a, b| {
            a.display_name
                .to_ascii_lowercase()
                .cmp(&b.display_name.to_ascii_lowercase())
                .then_with(|| a.file_name.cmp(&b.file_name))
        });
        entries
    }

    /// Import a local `.png` skin file into `<project_root>/skins/`, validating PNG header and size.
    pub fn import_library_skin(project_root: &Path, src: &Path) -> crate::Result<LocalSkinEntry> {
        let bytes = fs::read(src)?;
        if bytes.len() <= 8 || bytes.len() > MAX_SKIN_BYTES || !bytes.starts_with(PNG_MAGIC) {
            return Err(crate::AuthError::Profile(
                "Selected skin file is not a valid PNG (max 4 MB).".to_string(),
            ));
        }
        let dir = Self::library_dir(project_root);
        fs::create_dir_all(&dir)?;

        let stem = src
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("skin");
        let mut file_name = format!("{stem}.png");
        let mut dest = dir.join(&file_name);
        let mut idx = 2usize;
        while dest.exists() {
            file_name = format!("{stem} ({idx}).png");
            dest = dir.join(&file_name);
            idx += 1;
        }
        atomic_write_bytes(&dest, &bytes)?;
        let display_name = dest
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&file_name)
            .to_string();
        Ok(LocalSkinEntry {
            file_name,
            display_name,
            path: dest,
            size_bytes: bytes.len() as u64,
        })
    }

    /// Delete a skin PNG inside `<project_root>/skins/` (with strict path-traversal check).
    pub fn delete_library_skin(project_root: &Path, file_name: &str) -> crate::Result<()> {
        if file_name.is_empty()
            || file_name.contains('/')
            || file_name.contains('\\')
            || file_name.contains("..")
            || !file_name.to_ascii_lowercase().ends_with(".png")
        {
            return Err(crate::AuthError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid skin filename",
            )));
        }
        let target = Self::library_dir(project_root).join(file_name);
        if target.is_file() {
            fs::remove_file(&target)?;
        }
        Ok(())
    }

    /// Download PNG texture bytes (skin or cape) from `textures.minecraft.net`.
    pub async fn download_texture_bytes(url: &str) -> Option<Vec<u8>> {
        let valid_url = Self::validate_skin_url(url)?;
        let http = reqwest::Client::builder()
            .user_agent(mcw_models::user_agent())
            .timeout(Duration::from_secs(15))
            .build()
            .ok()?;
        let resp = http
            .get(&valid_url)
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
        Some(bytes.to_vec())
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

    #[test]
    fn manages_local_skin_library_import_list_and_delete() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("cool_skin.png");
        fs::write(&src, b"\x89PNG\r\n\x1a\nfake_png_bytes").unwrap();

        let imported = AccountSkinManager::import_library_skin(dir.path(), &src).unwrap();
        assert_eq!(imported.file_name, "cool_skin.png");
        assert_eq!(imported.display_name, "cool_skin");

        let dup = AccountSkinManager::import_library_skin(dir.path(), &src).unwrap();
        assert_eq!(dup.file_name, "cool_skin (2).png");

        let list = AccountSkinManager::list_library_skins(dir.path());
        assert_eq!(list.len(), 2);

        AccountSkinManager::delete_library_skin(dir.path(), "cool_skin.png").unwrap();
        assert_eq!(AccountSkinManager::list_library_skins(dir.path()).len(), 1);
        assert!(AccountSkinManager::delete_library_skin(dir.path(), "../evil.png").is_err());

        let invalid = dir.path().join("bad.png");
        fs::write(&invalid, b"not_a_png").unwrap();
        assert!(AccountSkinManager::import_library_skin(dir.path(), &invalid).is_err());
    }
}
