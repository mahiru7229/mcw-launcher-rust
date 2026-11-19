use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountSource {
    Offline,
    Microsoft,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub username: String,
    pub uuid: String,
    pub account_type: AccountSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Authentication {
    pub player_name: String,
    /// Undashed 32-char hex UUID expected by Minecraft launch arguments.
    pub uuid: String,
    pub access_token: String,
    pub xuid: String,
    pub client_id: String,
    pub user_type: String,
}
