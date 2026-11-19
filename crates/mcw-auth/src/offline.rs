use crate::{AuthError, Result};
use mcw_models::account::Authentication;
use md5::{Digest, Md5};
use uuid::Uuid;

pub struct OfflineAuth;

impl OfflineAuth {
    pub const ACCESS_TOKEN: &'static str = "0";
    pub const USER_TYPE: &'static str = "legacy";

    pub fn authenticate(username: &str) -> Result<Authentication> {
        let player_name = username.trim();
        if player_name.is_empty() {
            return Err(AuthError::EmptyUsername);
        }

        let launch_uuid = Self::uuid_undashed(player_name);

        Ok(Authentication {
            player_name: player_name.to_string(),
            uuid: launch_uuid,
            access_token: Self::ACCESS_TOKEN.to_string(),
            xuid: String::new(),
            client_id: String::new(),
            user_type: Self::USER_TYPE.to_string(),
        })
    }

    /// Generate Minecraft's canonical offline UUID (UUIDv3 from `OfflinePlayer:<name>`).
    pub fn uuid_v3(player_name: &str) -> Uuid {
        let mut hasher = Md5::new();
        hasher.update(format!("OfflinePlayer:{player_name}").as_bytes());
        let mut bytes: [u8; 16] = hasher.finalize().into();

        // UUID version 3
        bytes[6] = (bytes[6] & 0x0F) | 0x30;
        // IETF variant
        bytes[8] = (bytes[8] & 0x3F) | 0x80;

        Uuid::from_bytes(bytes)
    }

    pub fn uuid_dashed(player_name: &str) -> String {
        Self::uuid_v3(player_name).hyphenated().to_string()
    }

    pub fn uuid_undashed(player_name: &str) -> String {
        Self::uuid_v3(player_name).simple().to_string()
    }

    /// Match Minecraft's DefaultPlayerSkin algorithm (classic vs slim).
    pub fn default_skin_variant(player_uuid: Uuid) -> &'static str {
        let int_val = player_uuid.as_u128();
        let most = (int_val >> 64) as u64;
        let least = int_val as u64;
        let hilo = most ^ least;
        let hash_code = ((hilo >> 32) ^ hilo) as u32;
        if (hash_code & 1) == 1 {
            "slim"
        } else {
            "classic"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_minecraft_spec() {
        let dashed = OfflineAuth::uuid_dashed("Player");
        let auth = OfflineAuth::authenticate("Player").unwrap();
        assert_eq!(dashed.len(), 36);
        assert_eq!(auth.uuid.len(), 32);
        assert_eq!(auth.uuid, dashed.replace('-', ""));
        assert_eq!(auth.access_token, "0");
        assert_eq!(auth.user_type, "legacy");
        assert_eq!(auth.player_name, "Player");
        let parsed = Uuid::parse_str(&dashed).unwrap();
        assert_eq!(parsed.get_version_num(), 3);
    }

    #[test]
    fn empty_username_fails() {
        assert!(OfflineAuth::authenticate("   ").is_err());
    }
}
