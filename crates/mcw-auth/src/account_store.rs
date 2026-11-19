use crate::{AuthError, Result};
use mcw_models::account::AccountSource;
use mcw_storage::{atomic_write_json, LauncherPaths};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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
            let _ = Self::delete_refresh_token(&removed.uuid);
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

    /// Store Microsoft refresh token in OS Keyring (Windows Credential Manager / Linux Secret Service).
    /// Security rule: never store short-lived Minecraft access tokens on disk.
    pub fn store_refresh_token(uuid: &str, refresh_token: &str) -> Result<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, uuid)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        entry
            .set_password(refresh_token)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        Ok(())
    }

    pub fn load_refresh_token(uuid: &str) -> Result<String> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, uuid)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        entry
            .get_password()
            .map_err(|_| AuthError::MissingRefreshToken(uuid.to_string()))
    }

    pub fn delete_refresh_token(uuid: &str) -> Result<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, uuid)
            .map_err(|e| AuthError::Keyring(e.to_string()))?;
        let _ = entry.delete_credential();
        Ok(())
    }
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
}
