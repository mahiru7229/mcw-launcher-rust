use futures::stream::{self, StreamExt};
use reqwest::header::{RANGE, USER_AGENT};
use reqwest::StatusCode;
use sha1::{Digest, Sha1};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

static GLOBAL_DOWNLOADED_BYTES: AtomicU64 = AtomicU64::new(0);

/// Returns cumulative bytes downloaded by all `DownloadClient` instances in this process.
/// Used by the desktop UI to compute real-time network download speed (B/s, KB/s, MB/s).
pub fn total_downloaded_bytes() -> u64 {
    GLOBAL_DOWNLOADED_BYTES.load(Ordering::Relaxed)
}
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::time::sleep;
use tracing::debug;

#[derive(Debug, Error)]
pub enum NetError {
    #[error("HTTP client initialization failed: {0}")]
    ClientInit(#[from] reqwest::Error),
    #[error("Download failed for {url} after retries: {reason}")]
    DownloadFailed { url: String, reason: String },
    #[error("Hash verification failed for {path}: expected {expected}, got {actual}")]
    HashMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("Size verification failed for {path}: expected {expected} bytes, got {actual} bytes")]
    SizeMismatch {
        path: String,
        expected: u64,
        actual: u64,
    },
    #[error("I/O error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: io::Error,
    },
}

pub type Result<T> = std::result::Result<T, NetError>;

#[derive(Debug, Clone)]
pub struct DownloadRequest {
    pub url: String,
    pub mirrors: Vec<String>,
    pub destination: PathBuf,
    pub expected_sha1: Option<String>,
    /// 0 means unknown/unchecked (Maven servers frequently omit Content-Length).
    pub expected_size: u64,
}

impl DownloadRequest {
    pub fn new(url: impl Into<String>, destination: impl Into<PathBuf>) -> Self {
        Self {
            url: url.into(),
            mirrors: Vec::new(),
            destination: destination.into(),
            expected_sha1: None,
            expected_size: 0,
        }
    }

    pub fn with_sha1(mut self, sha1: Option<String>) -> Self {
        self.expected_sha1 = sha1.map(|s| s.trim().to_ascii_lowercase());
        self
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.expected_size = size;
        self
    }

    fn candidate_urls(&self) -> Vec<&str> {
        let mut urls = Vec::with_capacity(1 + self.mirrors.len());
        urls.push(self.url.as_str());
        for m in &self.mirrors {
            if !m.trim().is_empty() {
                urls.push(m.as_str());
            }
        }
        urls
    }
}

#[derive(Debug, Clone, Default)]
pub struct DownloadOutcome {
    pub destination: PathBuf,
    pub skipped: bool,
    pub bytes_written: u64,
}

#[derive(Clone)]
pub struct DownloadClient {
    http: reqwest::Client,
    max_retries: usize,
}

impl DownloadClient {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(mcw_models::user_agent())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .pool_max_idle_per_host(20)
            .build()?;
        Ok(Self {
            http,
            max_retries: 4,
        })
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub async fn get_text(&self, url: &str) -> Result<String> {
        let mut last_err = String::new();
        for attempt in 0..=self.max_retries {
            match self
                .http
                .get(url)
                .header(USER_AGENT, mcw_models::user_agent())
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    return resp.text().await.map_err(|e| NetError::DownloadFailed {
                        url: url.to_string(),
                        reason: e.to_string(),
                    });
                }
                Ok(resp) => {
                    last_err = format!("HTTP {}", resp.status());
                }
                Err(e) => {
                    last_err = e.to_string();
                }
            }
            if attempt < self.max_retries {
                sleep(Duration::from_millis(300 * (1 << attempt.min(4)))).await;
            }
        }
        Err(NetError::DownloadFailed {
            url: url.to_string(),
            reason: last_err,
        })
    }

    pub async fn download_file(&self, req: &DownloadRequest) -> Result<DownloadOutcome> {
        if req.destination.is_file()
            && verify_file(
                &req.destination,
                req.expected_sha1.as_deref(),
                req.expected_size,
            )
            .is_ok()
        {
            return Ok(DownloadOutcome {
                destination: req.destination.clone(),
                skipped: true,
                bytes_written: 0,
            });
        }

        if let Some(parent) = req.destination.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|source| NetError::Io {
                    path: parent.display().to_string(),
                    source,
                })?;
        }

        let part_path = part_path_for(&req.destination);
        let urls = req.candidate_urls();
        let mut last_err = String::from("no candidate URLs");

        for url in urls {
            for attempt in 0..=self.max_retries {
                match self.download_once(url, &part_path, req).await {
                    Ok(bytes_written) => {
                        verify_file(&part_path, req.expected_sha1.as_deref(), req.expected_size)?;
                        rename_with_retry(&part_path, &req.destination)?;
                        return Ok(DownloadOutcome {
                            destination: req.destination.clone(),
                            skipped: false,
                            bytes_written,
                        });
                    }
                    Err(err) => {
                        last_err = err.to_string();
                        debug!(
                            url = %url,
                            attempt = attempt + 1,
                            error = %last_err,
                            "download attempt failed"
                        );
                        // If hash/size mismatch, delete corrupted .part file before retry.
                        if matches!(
                            err,
                            NetError::HashMismatch { .. } | NetError::SizeMismatch { .. }
                        ) {
                            let _ = tokio::fs::remove_file(&part_path).await;
                        }
                        if attempt < self.max_retries {
                            sleep(Duration::from_millis(250 * (1 << attempt.min(4)))).await;
                        }
                    }
                }
            }
        }

        let _ = tokio::fs::remove_file(&part_path).await;
        Err(NetError::DownloadFailed {
            url: req.url.clone(),
            reason: last_err,
        })
    }

    async fn download_once(
        &self,
        url: &str,
        part_path: &Path,
        req: &DownloadRequest,
    ) -> Result<u64> {
        let existing_len = tokio::fs::metadata(part_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        let mut request_builder = self
            .http
            .get(url)
            .header(USER_AGENT, mcw_models::user_agent());

        let resume_from =
            if existing_len > 0 && (req.expected_size == 0 || existing_len < req.expected_size) {
                request_builder = request_builder.header(RANGE, format!("bytes={existing_len}-"));
                existing_len
            } else {
                0
            };

        let response = request_builder
            .send()
            .await
            .map_err(|e| NetError::DownloadFailed {
                url: url.to_string(),
                reason: e.to_string(),
            })?;

        let status = response.status();
        if status == StatusCode::RANGE_NOT_SATISFIABLE {
            let _ = tokio::fs::remove_file(part_path).await;
            return Err(NetError::DownloadFailed {
                url: url.to_string(),
                reason: "HTTP 416 Range Not Satisfiable (resetting partial file)".to_string(),
            });
        }
        if !status.is_success() {
            return Err(NetError::DownloadFailed {
                url: url.to_string(),
                reason: format!("HTTP {status}"),
            });
        }

        let append_mode = resume_from > 0 && status == StatusCode::PARTIAL_CONTENT;
        let mut file = if append_mode {
            tokio::fs::OpenOptions::new()
                .append(true)
                .open(part_path)
                .await
                .map_err(|source| NetError::Io {
                    path: part_path.display().to_string(),
                    source,
                })?
        } else {
            tokio::fs::File::create(part_path)
                .await
                .map_err(|source| NetError::Io {
                    path: part_path.display().to_string(),
                    source,
                })?
        };

        let mut written = if append_mode { resume_from } else { 0 };
        let mut stream = response.bytes_stream();
        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res.map_err(|e| NetError::DownloadFailed {
                url: url.to_string(),
                reason: e.to_string(),
            })?;
            file.write_all(&chunk)
                .await
                .map_err(|source| NetError::Io {
                    path: part_path.display().to_string(),
                    source,
                })?;
            written += chunk.len() as u64;
            GLOBAL_DOWNLOADED_BYTES.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        }
        file.flush().await.map_err(|source| NetError::Io {
            path: part_path.display().to_string(),
            source,
        })?;

        Ok(written)
    }

    pub async fn download_batch<F>(
        &self,
        requests: Vec<DownloadRequest>,
        concurrency: usize,
        on_progress: F,
    ) -> Result<Vec<DownloadOutcome>>
    where
        F: Fn(u64, u64) + Send + Sync + 'static,
    {
        let mut seen_dests = HashSet::with_capacity(requests.len());
        let unique_requests: Vec<DownloadRequest> = requests
            .into_iter()
            .filter(|req| seen_dests.insert(req.destination.clone()))
            .collect();

        let total = unique_requests.len() as u64;
        if total == 0 {
            return Ok(Vec::new());
        }

        let completed = Arc::new(AtomicU64::new(0));
        let callback = Arc::new(on_progress);
        let client = self.clone();
        let limit = concurrency.max(1);

        let results: Vec<Result<DownloadOutcome>> = stream::iter(unique_requests)
            .map(move |req| {
                let client = client.clone();
                let completed = Arc::clone(&completed);
                let callback = Arc::clone(&callback);
                async move {
                    let outcome = client.download_file(&req).await?;
                    let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    callback(done, total);
                    Ok(outcome)
                }
            })
            .buffer_unordered(limit)
            .collect()
            .await;

        let mut outcomes = Vec::with_capacity(results.len());
        for res in results {
            outcomes.push(res?);
        }
        Ok(outcomes)
    }
}

pub fn compute_sha1(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(|source| NetError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let mut hasher = Sha1::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|source| NetError::Io {
            path: path.display().to_string(),
            source,
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    Ok(hex_encode(&digest))
}

/// Verify file size (only when `expected_size > 0`) and SHA-1 (when provided).
pub fn verify_file(path: &Path, expected_sha1: Option<&str>, expected_size: u64) -> Result<()> {
    let meta = fs::metadata(path).map_err(|source| NetError::Io {
        path: path.display().to_string(),
        source,
    })?;

    // Lesson #1 from 1.8.x: Maven servers may report size == 0; only check size when > 0.
    if expected_size > 0 && meta.len() != expected_size {
        return Err(NetError::SizeMismatch {
            path: path.display().to_string(),
            expected: expected_size,
            actual: meta.len(),
        });
    }

    if let Some(expected) = expected_sha1 {
        let expected_clean = expected.trim().to_ascii_lowercase();
        if !expected_clean.is_empty() {
            let actual = compute_sha1(path)?;
            if actual != expected_clean {
                return Err(NetError::HashMismatch {
                    path: path.display().to_string(),
                    expected: expected_clean,
                    actual,
                });
            }
        }
    }

    Ok(())
}

static PART_COUNTER: AtomicU64 = AtomicU64::new(1);

fn part_path_for(dest: &Path) -> PathBuf {
    let seq = PART_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let mut s = dest.as_os_str().to_os_string();
    s.push(format!(".{pid}-{seq}.part"));
    PathBuf::from(s)
}

fn rename_with_retry(from: &Path, to: &Path) -> Result<()> {
    for attempt in 0..=5 {
        if to.exists() {
            let _ = fs::remove_file(to);
        }
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(err) if attempt < 5 && err.kind() == io::ErrorKind::PermissionDenied => {
                std::thread::sleep(Duration::from_millis(25 * (attempt as u64 + 1)));
            }
            Err(source) => {
                return Err(NetError::Io {
                    path: to.display().to_string(),
                    source,
                });
            }
        }
    }
    unreachable!()
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn verify_file_allows_zero_expected_size_and_checks_sha1() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sample.jar");
        fs::write(&path, b"hello world").unwrap();

        // SHA-1("hello world") = 2aae6c35c94fcfb415dbe95f408b9ce91ee846ed
        let sha1 = "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed";
        assert!(verify_file(&path, Some(sha1), 0).is_ok());
        assert!(verify_file(&path, Some(sha1), 11).is_ok());
        assert!(verify_file(&path, Some(sha1), 999).is_err());
        assert!(verify_file(&path, Some("0000000000000000000000000000000000000000"), 0).is_err());
    }
}
