pub mod account_store;
pub mod microsoft;
pub mod offline;
pub mod pkce;
pub mod skin;

pub use account_store::{AccountStore, StoredAccount};
pub use microsoft::{
    DeviceCodeResponse, MicrosoftAuthClient, MicrosoftLoginResult, MicrosoftOAuthToken,
    MinecraftProfile,
};
pub use offline::OfflineAuth;
pub use pkce::PkceChallenge;
pub use skin::AccountSkinManager;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Offline account username cannot be empty")]
    EmptyUsername,
    #[error("Microsoft authentication was cancelled")]
    Cancelled,
    #[error("Microsoft authentication timed out: {0}")]
    TimedOut(String),
    #[error("OAuth state mismatch (possible CSRF)")]
    StateMismatch,
    #[error("OAuth callback error: {0}")]
    OAuthCallback(String),
    #[error("XSTS authentication failed: {0}")]
    Xsts(String),
    #[error("Minecraft profile error: {0}")]
    Profile(String),
    #[error("Account '{0}' not found")]
    AccountNotFound(String),
    #[error("No refresh token stored for account '{0}' (please log in again)")]
    MissingRefreshToken(String),
    #[error("OS keyring error: {0}")]
    Keyring(String),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Storage error: {0}")]
    Storage(#[from] mcw_storage::StorageError),
}

pub type Result<T> = std::result::Result<T, AuthError>;
