use crate::pkce::PkceChallenge;
use crate::{AuthError, Result};
use mcw_models::account::Authentication;
use mcw_models::MICROSOFT_CLIENT_ID;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::{sleep, timeout};
use url::Url;

const TENANT: &str = "consumers";
const AUTHORIZE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const REDIRECT_URI: &str = "http://localhost:8400";
const CALLBACK_BIND_ADDR: &str = "127.0.0.1:8400";
const SCOPES: &str = "XboxLive.signin offline_access";

const XBL_AUTH_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeviceCodeResponse {
    pub user_code: String,
    pub device_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    #[serde(default = "default_interval")]
    pub interval: u64,
    #[serde(default)]
    pub message: String,
}

fn default_interval() -> u64 {
    5
}

#[derive(Debug, Clone, Deserialize)]
pub struct MicrosoftOAuthToken {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MinecraftProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub skins: Vec<MinecraftSkin>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MinecraftSkin {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub state: String,
}

impl MinecraftProfile {
    pub fn active_skin_url(&self) -> Option<String> {
        self.skins
            .iter()
            .find(|s| s.state.eq_ignore_ascii_case("ACTIVE"))
            .or_else(|| self.skins.first())
            .map(|s| s.url.clone())
            .filter(|u| !u.is_empty())
    }
}

#[derive(Debug, Clone)]
pub struct MicrosoftLoginResult {
    pub profile: MinecraftProfile,
    pub authentication: Authentication,
    pub refresh_token: Option<String>,
}

#[derive(Clone)]
pub struct MicrosoftAuthClient {
    http: reqwest::Client,
    client_id: String,
}

impl MicrosoftAuthClient {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(mcw_models::user_agent())
            .timeout(Duration::from_secs(30))
            .build()?;
        let _ = TENANT;
        Ok(Self {
            http,
            client_id: MICROSOFT_CLIENT_ID.to_string(),
        })
    }

    pub async fn request_device_code(&self) -> Result<DeviceCodeResponse> {
        let resp = self
            .http
            .post(DEVICE_CODE_URL)
            .form(&[("client_id", self.client_id.as_str()), ("scope", SCOPES)])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AuthError::OAuthCallback(format!(
                "Device code request failed (HTTP {status}): {text}"
            )));
        }

        Ok(resp.json::<DeviceCodeResponse>().await?)
    }

    pub async fn poll_device_code(
        &self,
        device_code: &str,
        interval_secs: u64,
        expires_in_secs: u64,
    ) -> Result<MicrosoftOAuthToken> {
        let deadline = Instant::now() + Duration::from_secs(expires_in_secs);
        let mut poll_interval = interval_secs.max(1);

        while Instant::now() < deadline {
            sleep(Duration::from_secs(poll_interval)).await;

            let resp = self
                .http
                .post(TOKEN_URL)
                .form(&[
                    ("client_id", self.client_id.as_str()),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("device_code", device_code),
                ])
                .send()
                .await;

            let Ok(resp) = resp else {
                continue;
            };

            if resp.status().is_success() {
                return Ok(resp.json::<MicrosoftOAuthToken>().await?);
            }

            let payload: serde_json::Value = resp.json().await.unwrap_or_default();
            let err_code = payload
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown_error");

            match err_code {
                "authorization_pending" => continue,
                "slow_down" => {
                    poll_interval += 5;
                    continue;
                }
                "expired_token" => {
                    return Err(AuthError::TimedOut(
                        "Device authorization code expired".to_string(),
                    ));
                }
                "access_denied" => return Err(AuthError::Cancelled),
                other => {
                    return Err(AuthError::OAuthCallback(format!(
                        "Microsoft OAuth failed: {other}"
                    )));
                }
            }
        }

        Err(AuthError::TimedOut(
            "Microsoft device authorization timed out".to_string(),
        ))
    }

    /// PKCE Browser OAuth flow.
    /// Lesson #4: Binds and listens on `127.0.0.1:8400` BEFORE invoking `on_authorize_url`.
    pub async fn authenticate_browser_pkce<F>(
        &self,
        callback_timeout: Duration,
        on_authorize_url: F,
    ) -> Result<MicrosoftOAuthToken>
    where
        F: FnOnce(&str),
    {
        let pkce = PkceChallenge::generate();
        let mut auth_url =
            Url::parse(AUTHORIZE_URL).map_err(|e| AuthError::OAuthCallback(e.to_string()))?;
        auth_url
            .query_pairs_mut()
            .append_pair("client_id", &self.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", REDIRECT_URI)
            .append_pair("scope", SCOPES)
            .append_pair("code_challenge", &pkce.code_challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &pkce.state)
            .append_pair("prompt", "select_account");

        // Bind & listen FIRST before opening the browser!
        let listener = TcpListener::bind(CALLBACK_BIND_ADDR).await?;
        on_authorize_url(auth_url.as_str());

        let (code, returned_state) = wait_for_oauth_callback(listener, callback_timeout).await?;
        if returned_state != pkce.state {
            return Err(AuthError::StateMismatch);
        }

        self.exchange_authorization_code(&code, &pkce.code_verifier)
            .await
    }

    async fn exchange_authorization_code(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<MicrosoftOAuthToken> {
        let resp = self
            .http
            .post(TOKEN_URL)
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", REDIRECT_URI),
                ("code_verifier", code_verifier),
                ("scope", SCOPES),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AuthError::OAuthCallback(format!(
                "Token exchange failed (HTTP {status}): {text}"
            )));
        }

        Ok(resp.json::<MicrosoftOAuthToken>().await?)
    }

    pub async fn refresh_microsoft_token(
        &self,
        refresh_token: &str,
    ) -> Result<MicrosoftOAuthToken> {
        let resp = self
            .http
            .post(TOKEN_URL)
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("scope", SCOPES),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AuthError::OAuthCallback(format!(
                "Token refresh failed (HTTP {status}): {text}"
            )));
        }

        Ok(resp.json::<MicrosoftOAuthToken>().await?)
    }

    /// Complete the full Xbox Live -> XSTS -> Minecraft Services -> Profile chain.
    pub async fn complete_minecraft_chain(
        &self,
        oauth: MicrosoftOAuthToken,
    ) -> Result<MicrosoftLoginResult> {
        let (xbl_token, uhs) = self.authenticate_xbox_live(&oauth.access_token).await?;
        let (xsts_token, xsts_uhs) = self.authenticate_xsts(&xbl_token).await?;
        let user_hash = if xsts_uhs.is_empty() { uhs } else { xsts_uhs };
        let mc_access_token = self
            .authenticate_minecraft_services(&user_hash, &xsts_token)
            .await?;
        let profile = self.fetch_minecraft_profile(&mc_access_token).await?;

        let authentication = Authentication {
            player_name: profile.name.clone(),
            uuid: profile.id.replace('-', ""),
            access_token: mc_access_token,
            xuid: user_hash,
            client_id: self.client_id.clone(),
            user_type: "msa".to_string(),
        };

        Ok(MicrosoftLoginResult {
            profile,
            authentication,
            refresh_token: oauth.refresh_token,
        })
    }

    async fn authenticate_xbox_live(&self, ms_access_token: &str) -> Result<(String, String)> {
        let payload = json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={ms_access_token}")
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        });

        let resp = self
            .http
            .post(XBL_AUTH_URL)
            .header("x-xbl-contract-version", "1")
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AuthError::Xsts(format!(
                "Xbox Live auth failed (HTTP {status}): {text}"
            )));
        }

        let body: serde_json::Value = resp.json().await?;
        let token = body
            .get("Token")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let uhs = body
            .pointer("/DisplayClaims/xui/0/uhs")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        if token.is_empty() || uhs.is_empty() {
            return Err(AuthError::Xsts(
                "Xbox Live returned an incomplete token response".to_string(),
            ));
        }
        Ok((token, uhs))
    }

    async fn authenticate_xsts(&self, xbl_token: &str) -> Result<(String, String)> {
        let payload = json!({
            "Properties": {
                "SandboxId": "RETAIL",
                "UserTokens": [xbl_token]
            },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        });

        let resp = self
            .http
            .post(XSTS_AUTH_URL)
            .header("x-xbl-contract-version", "1")
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let xerr = body.get("XErr").and_then(|v| v.as_u64());
            let friendly = format_xsts_error(xerr, body.get("Message").and_then(|v| v.as_str()));
            return Err(AuthError::Xsts(friendly));
        }

        let body: serde_json::Value = resp.json().await?;
        let token = body
            .get("Token")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let uhs = body
            .pointer("/DisplayClaims/xui/0/uhs")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        if token.is_empty() {
            return Err(AuthError::Xsts("XSTS returned an empty token".to_string()));
        }
        Ok((token, uhs))
    }

    async fn authenticate_minecraft_services(&self, uhs: &str, xsts_token: &str) -> Result<String> {
        let payload = json!({
            "identityToken": format!("XBL3.0 x={uhs};{xsts_token}")
        });

        let resp = self.http.post(MC_LOGIN_URL).json(&payload).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AuthError::Profile(format!(
                "Minecraft Services login failed (HTTP {status}): {text}"
            )));
        }

        let body: serde_json::Value = resp.json().await?;
        let access_token = body
            .get("access_token")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if access_token.is_empty() {
            return Err(AuthError::Profile(
                "Minecraft Services returned an empty access token".to_string(),
            ));
        }
        Ok(access_token)
    }

    async fn fetch_minecraft_profile(&self, mc_access_token: &str) -> Result<MinecraftProfile> {
        let resp = self
            .http
            .get(MC_PROFILE_URL)
            .bearer_auth(mc_access_token)
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(AuthError::Profile(
                "This Microsoft account does not own Minecraft Java Edition or has no profile."
                    .to_string(),
            ));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(AuthError::Profile(format!(
                "Unable to load Minecraft profile (HTTP {status})"
            )));
        }

        Ok(resp.json::<MinecraftProfile>().await?)
    }
}

pub fn format_xsts_error(xerr: Option<u64>, fallback: Option<&str>) -> String {
    match xerr {
        Some(2148916233) => "This Microsoft account does not have an Xbox profile.".to_string(),
        Some(2148916235) => "Xbox Live is unavailable in this account's region.".to_string(),
        Some(2148916236 | 2148916237) => {
            "This account requires adult verification on xbox.com.".to_string()
        }
        Some(2148916238) => {
            "This account is underage and must be added to a Microsoft family.".to_string()
        }
        _ => fallback
            .unwrap_or("XSTS authentication failed.")
            .to_string(),
    }
}

async fn wait_for_oauth_callback(
    listener: TcpListener,
    wait_timeout: Duration,
) -> Result<(String, String)> {
    let accept_res = timeout(wait_timeout, listener.accept())
        .await
        .map_err(|_| AuthError::TimedOut("No OAuth callback received from browser".to_string()))?;

    let (mut stream, _) = accept_res?;
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).await?;
    let request_text = String::from_utf8_lossy(&buf[..n]);
    let request_line = request_text.lines().next().unwrap_or("");
    let path_and_query = request_line.split_whitespace().nth(1).unwrap_or("/");

    let parsed = Url::parse(&format!("http://localhost{path_and_query}"))
        .map_err(|e| AuthError::OAuthCallback(e.to_string()))?;
    let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();

    let (status_line, body) = if params.contains_key("code") {
        (
            "HTTP/1.1 200 OK",
            "<html><head><meta charset='UTF-8'><title>MCW Launcher</title></head><body><h2>Microsoft login completed.</h2><p>You can close this tab and return to MCW Launcher.</p></body></html>",
        )
    } else {
        (
            "HTTP/1.1 400 Bad Request",
            "<html><head><meta charset='UTF-8'><title>MCW Launcher</title></head><body><h2>Microsoft login failed.</h2><p>You can close this tab and return to MCW Launcher.</p></body></html>",
        )
    };

    let response = format!(
        "{status_line}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nPragma: no-cache\r\nContent-Security-Policy: default-src 'none'\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;

    if let Some(err) = params.get("error") {
        if err == "access_denied" {
            return Err(AuthError::Cancelled);
        }
        let desc = params.get("error_description").cloned().unwrap_or_default();
        return Err(AuthError::OAuthCallback(format!("{err}: {desc}")));
    }

    let code = params
        .get("code")
        .cloned()
        .ok_or_else(|| AuthError::OAuthCallback("Missing authorization code".to_string()))?;
    let state = params.get("state").cloned().unwrap_or_default();

    Ok((code, state))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_xsts_error_codes() {
        assert!(format_xsts_error(Some(2148916233), None).contains("Xbox profile"));
        assert!(format_xsts_error(Some(2148916238), None).contains("Microsoft family"));
    }
}
