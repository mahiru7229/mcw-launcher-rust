use crate::{Result, RuntimeError};
use regex::Regex;
use serde::Deserialize;
use std::time::Duration;

const MCLOGS_API_URL: &str = "https://api.mclo.gs/1/log";
const MAX_LOG_SIZE_BYTES: usize = 5 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct McLogsUploadResult {
    pub id: String,
    pub url: String,
    pub raw: String,
}

#[derive(Debug, Deserialize)]
struct McLogsApiResponse {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    raw: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Redacts sensitive tokens, JWTs, emails, and OS user home directories before uploading logs.
pub fn sanitize_log_text(raw: &str) -> String {
    let mut out = raw.to_string();

    let patterns: &[(&str, &str)] = &[
        // Minecraft --accessToken argument
        (r"(--accessToken\s+)\S+", "${1}<REDACTED>"),
        // Authorization / Bearer tokens
        (r"(?i)(Bearer\s+)[A-Za-z0-9\-._~+/]+=*", "${1}<REDACTED>"),
        // JWT tokens
        (
            r"eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
            "<REDACTED_JWT>",
        ),
        // Windows user profile path
        (r"(?i)([A-Z]:\\Users\\)[^\\/\s]+", "${1}<USER>"),
        // Linux & macOS home paths
        (r"(/home/|/Users/)[^/\s]+", "${1}<USER>"),
        // Email addresses
        (
            r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}",
            "<REDACTED_EMAIL>",
        ),
    ];

    for (pat, rep) in patterns {
        if let Ok(re) = Regex::new(pat) {
            out = re.replace_all(&out, *rep).into_owned();
        }
    }

    out
}

pub struct McLogsClient;

impl McLogsClient {
    pub async fn upload(content: &str) -> Result<McLogsUploadResult> {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Err(RuntimeError::Process(
                "Log content is empty — nothing to upload.".to_string(),
            ));
        }

        let mut sanitized = sanitize_log_text(trimmed);
        if sanitized.len() > MAX_LOG_SIZE_BYTES {
            let mut kept = Vec::new();
            let mut total = 0usize;
            for line in sanitized.lines().rev() {
                let sz = line.len() + 1;
                if total + sz > MAX_LOG_SIZE_BYTES - 1024 {
                    break;
                }
                kept.push(line);
                total += sz;
            }
            kept.reverse();
            sanitized = format!("[... truncated earlier log lines ...]\n{}", kept.join("\n"));
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent("MCW-Launcher-Rust/0.5")
            .build()
            .map_err(|e| RuntimeError::Process(format!("HTTP client error: {e}")))?;

        let resp = client
            .post(MCLOGS_API_URL)
            .form(&[("content", sanitized.as_str())])
            .send()
            .await
            .map_err(|e| RuntimeError::Process(format!("Failed to upload to mclo.gs: {e}")))?;

        let status = resp.status();
        let body: McLogsApiResponse = resp.json().await.map_err(|e| {
            RuntimeError::Process(format!("Invalid mclo.gs response (HTTP {status}): {e}"))
        })?;

        if !status.is_success() || !body.success {
            return Err(RuntimeError::Process(format!(
                "mclo.gs error: {}",
                body.error.unwrap_or_else(|| format!("HTTP {status}"))
            )));
        }

        Ok(McLogsUploadResult {
            id: body.id.unwrap_or_default(),
            url: body.url.unwrap_or_default(),
            raw: body.raw.unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_tokens_paths_and_emails() {
        let raw = r"Launching with --accessToken eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abc123def456
Path: C:\Users\alice\AppData\Roaming\.minecraft and /home/bob/mcw
Contact: alice@example.com";
        let clean = sanitize_log_text(raw);
        assert!(clean.contains("--accessToken <REDACTED>"));
        assert!(clean.contains(r"C:\Users\<USER>\AppData"));
        assert!(clean.contains("/home/<USER>/mcw"));
        assert!(clean.contains("<REDACTED_EMAIL>"));
        assert!(!clean.contains("alice"));
        assert!(!clean.contains("bob"));
    }
}
