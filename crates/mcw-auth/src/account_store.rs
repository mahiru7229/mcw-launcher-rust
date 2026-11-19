use crate::{AuthError, Result};
use mcw_models::account::AccountSource;
use mcw_storage::{atomic_write_bytes, atomic_write_json, LauncherPaths};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[cfg(not(windows))]
const KEYRING_SERVICE: &str = "mcw-launcher-rust";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAccount {
    pub uuid: String,
    pub username: String,
    pub account_type: AccountSource,
    #[serde(default)]
    pub skin_url: Option<String>,
    #[serde(default)]
    pub active: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AccountsFile {
    schema_version: u32,
    accounts: Vec<StoredAccount>,
}

pub struct AccountStore<'a> {
    paths: &'a LauncherPaths,
}

impl<'a> AccountStore<'a> {
    pub fn new(paths: &'a LauncherPaths) -> Self {
        Self { paths }
    }

    fn accounts_file_path(&self) -> PathBuf {
        self.paths.accounts_root.join("accounts.json")
    }

    #[cfg(windows)]
    fn token_file_path(&self, uuid: &str) -> PathBuf {
        let clean: String = uuid
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        self.paths
            .accounts_root
            .join("tokens")
            .join(format!("{clean}.dpapi"))
    }

    pub fn list(&self) -> Result<Vec<StoredAccount>> {
        let path = self.accounts_file_path();
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(&path)?;
        let parsed: AccountsFile = serde_json::from_str(&raw).unwrap_or_default();
        Ok(parsed.accounts)
    }

    pub fn upsert(&self, mut account: StoredAccount, make_active: bool) -> Result<StoredAccount> {
        let mut accounts = self.list()?;
        if make_active {
            for existing in &mut accounts {
                existing.active = false;
            }
            account.active = true;
        }

        if let Some(slot) = accounts
            .iter_mut()
            .find(|a| a.uuid.eq_ignore_ascii_case(&account.uuid))
        {
            *slot = account.clone();
        } else {
            if accounts.is_empty() {
                account.active = true;
            }
            accounts.push(account.clone());
        }

        self.save_all(&accounts)?;
        Ok(account)
    }

    pub fn find(&self, username_or_uuid: &str) -> Result<StoredAccount> {
        let query = username_or_uuid.trim();
        let accounts = self.list()?;
        accounts
            .into_iter()
            .find(|a| {
                a.username.eq_ignore_ascii_case(query) || a.uuid.eq_ignore_ascii_case(query)
            })
            .ok_or_else(|| AuthError::AccountNotFound(query.to_string()))
    }

    pub fn active_account(&self) -> Result<Option<StoredAccount>> {
        let accounts = self.list()?;
        Ok(accounts
            .iter()
            .find(|a| a.active)
            .cloned()
            .or_else(|| accounts.into_iter().next()))
    }

    pub fn remove(&self, username_or_uuid: &str) -> Result<StoredAccount> {
        let mut accounts = self.list()?;
        let idx = accounts
            .iter()
            .position(|a| {
                a.username.eq_ignore_ascii_case(username_or_uuid)
                    || a.uuid.eq_ignore_ascii_case(username_or_uuid)
            })
            .ok_or_else(|| AuthError::AccountNotFound(username_or_uuid.to_string()))?;

        let removed = accounts.remove(idx);
        if removed.active {
            if let Some(first) = accounts.first_mut() {
                first.active = true;
            }
        }
        self.save_all(&accounts)?;

        if removed.account_type == AccountSource::Microsoft {
            let _ = self.delete_refresh_token(&removed.uuid);
        }
        Ok(removed)
    }

    fn save_all(&self, accounts: &[StoredAccount]) -> Result<()> {
        let file = AccountsFile {
            schema_version: 1,
            accounts: accounts.to_vec(),
        };
        atomic_write_json(&self.accounts_file_path(), &file)?;
        Ok(())
    }

    /// Store Microsoft refresh token encrypted with Windows DPAPI (no 2560-byte Credential Manager
    /// limit) or OS Keyring on Linux/macOS.
    /// Security rule: never persist short-lived Minecraft access tokens on disk.
    pub fn store_refresh_token(&self, uuid: &str, refresh_token: &str) -> Result<()> {
        #[cfg(windows)]
        {
            let encrypted = dpapi_protect(refresh_token.as_bytes())?;
            let path = self.token_file_path(uuid);
            atomic_write_bytes(&path, &encrypted)?;
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let entry = keyring::Entry::new(KEYRING_SERVICE, uuid)
                .map_err(|e| AuthError::Keyring(e.to_string()))?;
            entry
                .set_password(refresh_token)
                .map_err(|e| AuthError::Keyring(e.to_string()))?;
            Ok(())
        }
    }

    pub fn load_refresh_token(&self, uuid: &str) -> Result<String> {
        #[cfg(windows)]
        {
            let path = self.token_file_path(uuid);
            let encrypted = fs::read(&path)
                .map_err(|_| AuthError::MissingRefreshToken(uuid.to_string()))?;
            let decrypted = dpapi_unprotect(&encrypted)
                .map_err(|_| AuthError::MissingRefreshToken(uuid.to_string()))?;
            String::from_utf8(decrypted)
                .map_err(|_| AuthError::MissingRefreshToken(uuid.to_string()))
        }
        #[cfg(not(windows))]
        {
            let entry = keyring::Entry::new(KEYRING_SERVICE, uuid)
                .map_err(|e| AuthError::Keyring(e.to_string()))?;
            entry
                .get_password()
                .map_err(|_| AuthError::MissingRefreshToken(uuid.to_string()))
        }
    }

    pub fn delete_refresh_token(&self, uuid: &str) -> Result<()> {
        #[cfg(windows)]
        {
            let path = self.token_file_path(uuid);
            let _ = fs::remove_file(path);
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let entry = keyring::Entry::new(KEYRING_SERVICE, uuid)
                .map_err(|e| AuthError::Keyring(e.to_string()))?;
            let _ = entry.delete_credential();
            Ok(())
        }
    }
}

#[cfg(windows)]
fn dpapi_protect(plaintext: &[u8]) -> Result<Vec<u8>> {
    use std::ptr;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    use windows_sys::Win32::Foundation::LocalFree;

    let input = CRYPT_INTEGER_BLOB {
        cbData: plaintext.len() as u32,
        pbData: plaintext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };

    let ok = unsafe {
        CryptProtectData(
            &input,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 || output.pbData.is_null() {
        return Err(AuthError::Keyring(
            "Windows DPAPI CryptProtectData failed".to_string(),
        ));
    }

    let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as *mut _);
    }
    Ok(bytes)
}

#[cfg(windows)]
fn dpapi_unprotect(ciphertext: &[u8]) -> Result<Vec<u8>> {
    use std::ptr;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    use windows_sys::Win32::Foundation::LocalFree;

    let input = CRYPT_INTEGER_BLOB {
        cbData: ciphertext.len() as u32,
        pbData: ciphertext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };

    let ok = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 || output.pbData.is_null() {
        return Err(AuthError::Keyring(
            "Windows DPAPI CryptUnprotectData failed".to_string(),
        ));
    }

    let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as *mut _);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn account_store_upsert_find_remove() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();
        let store = AccountStore::new(&paths);

        let acc = StoredAccount {
            uuid: "a01e3843e5213998958af459800e4d11".to_string(),
            username: "Player".to_string(),
            account_type: AccountSource::Offline,
            skin_url: None,
            active: true,
        };
        store.upsert(acc.clone(), true).unwrap();

        let found = store.find("player").unwrap();
        assert_eq!(found.uuid, acc.uuid);
        assert!(found.active);

        store.remove("Player").unwrap();
        assert!(store.list().unwrap().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_encrypts_and_roundtrips_long_refresh_token() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();
        let store = AccountStore::new(&paths);

        // Realistic long Microsoft OAuth refresh token (>2000 bytes)
        let long_token = format!("M.C512_BAY.{}", "A1b2C3d4E5".repeat(220));
        let uuid = "83a14a8a25014a7ab178758bb1847f82";

        store.store_refresh_token(uuid, &long_token).unwrap();

        // Raw file on disk must NOT contain plaintext token
        let raw_on_disk = fs::read(store.token_file_path(uuid)).unwrap();
        assert!(!String::from_utf8_lossy(&raw_on_disk).contains("M.C512_BAY"));

        // Round-trip decrypt matches exactly
        let loaded = store.load_refresh_token(uuid).unwrap();
        assert_eq!(loaded, long_token);

        store.delete_refresh_token(uuid).unwrap();
        assert!(store.load_refresh_token(uuid).is_err());
    }
}
