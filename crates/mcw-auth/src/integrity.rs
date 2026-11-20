use crate::{AuthError, Result};
use mcw_storage::atomic_write_bytes;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const HMAC_BLOCK_SIZE: usize = 64;

pub struct AccountIntegrity;

impl AccountIntegrity {
    pub fn key_file_path(accounts_root: &Path) -> PathBuf {
        accounts_root.join("tokens").join(".integrity.key")
    }

    pub fn load_or_create_key(accounts_root: &Path) -> Result<Vec<u8>> {
        let path = Self::key_file_path(accounts_root);
        if path.is_file() {
            let raw = fs::read(&path)?;
            #[cfg(windows)]
            let key = crate::account_store::dpapi_unprotect(&raw)?;
            #[cfg(not(windows))]
            let key = raw;
            if key.len() >= 32 {
                return Ok(key);
            }
        }

        let mut key = vec![0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);

        #[cfg(windows)]
        let stored = crate::account_store::dpapi_protect(&key)?;
        #[cfg(not(windows))]
        let stored = key.clone();

        atomic_write_bytes(&path, &stored)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::metadata(&path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o600);
                let _ = fs::set_permissions(&path, perms);
            }
        }
        Ok(key)
    }

    pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
        let mut normalized_key = [0u8; HMAC_BLOCK_SIZE];
        if key.len() > HMAC_BLOCK_SIZE {
            let hashed = Sha256::digest(key);
            normalized_key[..32].copy_from_slice(&hashed);
        } else {
            normalized_key[..key.len()].copy_from_slice(key);
        }

        let mut i_key_pad = [0x36u8; HMAC_BLOCK_SIZE];
        let mut o_key_pad = [0x5cu8; HMAC_BLOCK_SIZE];
        for i in 0..HMAC_BLOCK_SIZE {
            i_key_pad[i] ^= normalized_key[i];
            o_key_pad[i] ^= normalized_key[i];
        }

        let mut inner = Sha256::new();
        inner.update(i_key_pad);
        inner.update(data);
        let inner_digest = inner.finalize();

        let mut outer = Sha256::new();
        outer.update(o_key_pad);
        outer.update(inner_digest);
        let outer_digest = outer.finalize();

        let mut out = [0u8; 32];
        out.copy_from_slice(&outer_digest);
        out
    }

    pub fn sign_bytes(key: &[u8], data: &[u8]) -> String {
        let mac = Self::hmac_sha256(key, data);
        let mut hex = String::with_capacity(64);
        for b in mac {
            hex.push_str(&format!("{b:02x}"));
        }
        hex
    }

    pub fn verify_bytes(key: &[u8], data: &[u8], expected_hex: &str) -> bool {
        let trimmed = expected_hex.trim();
        if trimmed.is_empty() {
            return false;
        }
        let computed = Self::sign_bytes(key, data);
        let a = computed.as_bytes();
        let b = trimmed.to_ascii_lowercase();
        let b_bytes = b.as_bytes();
        if a.len() != b_bytes.len() {
            return false;
        }
        let mut diff = 0u8;
        for (&x, &y) in a.iter().zip(b_bytes.iter()) {
            diff |= x ^ y;
        }
        diff == 0
    }

    pub fn reset_key(accounts_root: &Path) -> Result<Vec<u8>> {
        let path = Self::key_file_path(accounts_root);
        let _ = fs::remove_file(&path);
        Self::load_or_create_key(accounts_root)
            .map_err(|e| AuthError::Keyring(format!("Failed to reset account integrity key: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_rfc_2104_hmac_sha256_and_detects_tamper() {
        let mac =
            AccountIntegrity::sign_bytes(b"key", b"The quick brown fox jumps over the lazy dog");
        assert_eq!(
            mac,
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
        assert!(AccountIntegrity::verify_bytes(
            b"key",
            b"The quick brown fox jumps over the lazy dog",
            &mac
        ));
        assert!(!AccountIntegrity::verify_bytes(
            b"key",
            b"The quick brown fox jumps over the lazy cat",
            &mac
        ));
    }
}
