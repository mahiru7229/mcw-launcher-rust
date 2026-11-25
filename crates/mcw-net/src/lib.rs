pub mod bandwidth;
pub mod connectivity;
pub mod journal;
pub mod proxy;

pub use bandwidth::{global_bandwidth_limit_mbps, set_global_bandwidth_limit_mbps};
pub use connectivity::{ConnectivityMonitor, ConnectivityStatus};
pub use journal::{DownloadJournal, DownloadJournalEntry};
pub use proxy::{apply_global_proxy, global_proxy_config, set_global_proxy_config, ProxyConfig};

use futures::stream::{self, StreamExt};
use reqwest::header::{
    HeaderMap, ACCEPT_RANGES, CONTENT_LENGTH, ETAG, IF_NONE_MATCH, RANGE, RETRY_AFTER, USER_AGENT,
};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, UNIX_EPOCH};

type VerificationKey = (PathBuf, u64, u128);

fn verification_cache() -> &'static Mutex<HashMap<VerificationKey, String>> {
    static CACHE: OnceLock<Mutex<HashMap<VerificationKey, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn verification_cache_file() -> &'static Mutex<Option<PathBuf>> {
    static PATH: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    PATH.get_or_init(|| Mutex::new(None))
}

static VERIFICATION_CACHE_DIRTY: AtomicUsize = AtomicUsize::new(0);
static GLOBAL_SEGMENTS_PER_FILE: AtomicUsize = AtomicUsize::new(6);
const SEGMENT_MIN_BYTES: u64 = 4 * 1024 * 1024;
const PROGRESS_THROTTLE_BYTES: u64 = 128 * 1024;
const DISK_BUF_BYTES: usize = 256 * 1024;
const MAX_PERSISTED_VERIFY_ENTRIES: usize = 50_000;

/// Configure how many concurrent HTTP Range segments are used for large downloads (`>= 4 MiB`).
/// `0` or `1` disables segmented downloads (single connection); typical values are `4`, `6`, `8`.
pub fn set_global_segments_per_file(segments: usize) {
    let clamped = if segments <= 1 {
        1
    } else {
        segments.clamp(2, 16)
    };
    GLOBAL_SEGMENTS_PER_FILE.store(clamped, Ordering::Relaxed);
}

pub fn global_segments_per_file() -> usize {
    GLOBAL_SEGMENTS_PER_FILE.load(Ordering::Relaxed)
}
use tokio::sync::Notify;

static GLOBAL_DOWNLOADED_BYTES: AtomicU64 = AtomicU64::new(0);

/// Cooperative pause / resume / cancel controller for background downloads and tasks
/// (mirroring MCW Launcher Python's `DownloadPauseController`).
#[derive(Debug, Default)]
pub struct TaskControl {
    paused: AtomicBool,
    cancelled: AtomicBool,
    notify: Notify,
}

impl TaskControl {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn toggle_pause(&self) -> bool {
        if self.is_paused() {
            self.resume();
            false
        } else {
            self.pause();
            true
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.paused.store(false, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Blocks asynchronously while paused; returns `Err(NetError::Cancelled)` when cancelled.
    pub async fn check(&self) -> Result<()> {
        loop {
            if self.is_cancelled() {
                return Err(NetError::Cancelled);
            }
            if !self.is_paused() {
                return Ok(());
            }
            tokio::select! {
                _ = self.notify.notified() => {}
                _ = sleep(Duration::from_millis(200)) => {}
            }
        }
    }
}

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
    #[error("Task cancelled by user")]
    Cancelled,
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
    control: Option<Arc<TaskControl>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedHttpMeta {
    etag: String,
}

#[derive(Debug)]
enum SegmentAttemptError {
    FallbackSingleStream(String),
    Retryable(String),
    Permanent(String),
    Cancelled,
    Io { path: String, source: io::Error },
}

pub fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let raw = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    let secs = raw.parse::<u64>().ok()?;
    Some(Duration::from_secs(secs.clamp(1, 15)))
}

pub fn compute_byte_ranges(total: u64, segments: usize) -> Vec<(u64, u64)> {
    if total == 0 || segments <= 1 {
        return vec![(0, total.saturating_sub(1))];
    }
    let count = (segments as u64).min(total).max(1);
    let base = total / count;
    let rem = total % count;
    let mut out = Vec::with_capacity(count as usize);
    let mut start = 0u64;
    for i in 0..count {
        let len = base + if i < rem { 1 } else { 0 };
        let end = start + len - 1;
        out.push((start, end));
        start = end + 1;
    }
    out
}

impl DownloadClient {
    pub fn new() -> Result<Self> {
        let builder = reqwest::Client::builder()
            .user_agent(mcw_models::user_agent())
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(30))
            .tcp_nodelay(true)
            .http2_adaptive_window(true)
            .http2_initial_stream_window_size(Some(2 * 1024 * 1024))
            .http2_initial_connection_window_size(Some(4 * 1024 * 1024))
            .pool_max_idle_per_host(32);
        let http = apply_global_proxy(builder).build()?;
        Ok(Self {
            http,
            max_retries: 4,
            control: None,
        })
    }

    pub fn with_task_control(&self, control: Arc<TaskControl>) -> Self {
        let mut cloned = self.clone();
        cloned.control = Some(control);
        cloned
    }

    async fn check_control(&self) -> Result<()> {
        if let Some(ref ctrl) = self.control {
            ctrl.check().await?;
        }
        Ok(())
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub async fn get_text(&self, url: &str) -> Result<String> {
        let mut last_err = String::new();
        for attempt in 0..=self.max_retries {
            self.check_control().await?;
            let mut retry_delay: Option<Duration> = None;
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
                    let status = resp.status();
                    last_err = format!("HTTP {status}");
                    if !is_retryable_status(status) {
                        break;
                    }
                    retry_delay = parse_retry_after(resp.headers());
                }
                Err(e) => {
                    last_err = e.to_string();
                }
            }
            if attempt < self.max_retries {
                let wait = retry_delay
                    .unwrap_or_else(|| Duration::from_millis(300 * (1 << attempt.min(4))));
                sleep(wait).await;
            }
        }
        Err(NetError::DownloadFailed {
            url: url.to_string(),
            reason: last_err,
        })
    }

    /// Fetch text with `ETag` / `If-None-Match` conditional caching (`304 Not Modified`).
    pub async fn get_text_cached(&self, url: &str, cache_path: &Path) -> Result<String> {
        let etag_path = etag_sidecar_path(cache_path);
        let cached_body = fs::read_to_string(cache_path)
            .ok()
            .filter(|s| !s.trim().is_empty());
        let cached_etag = if cached_body.is_some() {
            fs::read_to_string(&etag_path)
                .ok()
                .and_then(|s| serde_json::from_str::<CachedHttpMeta>(&s).ok())
                .map(|m| m.etag)
                .filter(|e| !e.trim().is_empty())
        } else {
            None
        };

        let mut last_err = String::new();
        for attempt in 0..=self.max_retries {
            self.check_control().await?;
            let mut retry_delay: Option<Duration> = None;
            let mut req_builder = self
                .http
                .get(url)
                .header(USER_AGENT, mcw_models::user_agent());
            if let Some(ref etag) = cached_etag {
                req_builder = req_builder.header(IF_NONE_MATCH, etag.as_str());
            }

            match req_builder.send().await {
                Ok(resp) if resp.status() == StatusCode::NOT_MODIFIED => {
                    if let Some(body) = cached_body {
                        return Ok(body);
                    }
                    last_err = "HTTP 304 Not Modified without local cache body".to_string();
                }
                Ok(resp) if resp.status().is_success() => {
                    let new_etag = resp
                        .headers()
                        .get(ETAG)
                        .and_then(|v| v.to_str().ok())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                    let text = resp.text().await.map_err(|e| NetError::DownloadFailed {
                        url: url.to_string(),
                        reason: e.to_string(),
                    })?;
                    let _ = write_text_atomic(cache_path, &text);
                    if let Some(etag) = new_etag {
                        if let Ok(meta_raw) = serde_json::to_string(&CachedHttpMeta { etag }) {
                            let _ = write_text_atomic(&etag_path, &meta_raw);
                        }
                    }
                    return Ok(text);
                }
                Ok(resp) => {
                    let status = resp.status();
                    last_err = format!("HTTP {status}");
                    if !is_retryable_status(status) {
                        break;
                    }
                    retry_delay = parse_retry_after(resp.headers());
                }
                Err(e) => {
                    last_err = e.to_string();
                }
            }
            if attempt < self.max_retries {
                let wait = retry_delay
                    .unwrap_or_else(|| Duration::from_millis(300 * (1 << attempt.min(4))));
                sleep(wait).await;
            }
        }

        Err(NetError::DownloadFailed {
            url: url.to_string(),
            reason: last_err,
        })
    }

    pub async fn download_file(&self, req: &DownloadRequest) -> Result<DownloadOutcome> {
        self.download_file_with_progress(req, |_, _| {}).await
    }

    pub async fn download_file_with_progress<F>(
        &self,
        req: &DownloadRequest,
        on_bytes: F,
    ) -> Result<DownloadOutcome>
    where
        F: Fn(u64, u64) + Send + Sync + 'static,
    {
        self.check_control().await?;
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

        let on_bytes_cb: Arc<dyn Fn(u64, u64) + Send + Sync> = Arc::new(on_bytes);
        let part_path = part_path_for(&req.destination);
        let urls = req.candidate_urls();
        let mut last_err = String::from("no candidate URLs");
        let seg_count = global_segments_per_file();

        for url in urls {
            let mut allow_segmented = seg_count >= 2
                && (req.expected_size == 0 || req.expected_size >= SEGMENT_MIN_BYTES);

            if allow_segmented {
                if let Some(total_len) = self.probe_range_length(url, req.expected_size).await {
                    if total_len >= SEGMENT_MIN_BYTES {
                        for attempt in 0..=self.max_retries {
                            self.check_control().await?;
                            match self
                                .download_segmented(
                                    url,
                                    &part_path,
                                    total_len,
                                    seg_count,
                                    Arc::clone(&on_bytes_cb),
                                )
                                .await
                            {
                                Ok(bytes_written) => {
                                    verify_file(
                                        &part_path,
                                        req.expected_sha1.as_deref(),
                                        req.expected_size,
                                    )?;
                                    rename_with_retry(&part_path, &req.destination)?;
                                    return Ok(DownloadOutcome {
                                        destination: req.destination.clone(),
                                        skipped: false,
                                        bytes_written,
                                    });
                                }
                                Err(SegmentAttemptError::Cancelled) => {
                                    cleanup_segment_files(&part_path, seg_count);
                                    return Err(NetError::Cancelled);
                                }
                                Err(SegmentAttemptError::Io { path, source }) => {
                                    cleanup_segment_files(&part_path, seg_count);
                                    return Err(NetError::Io { path, source });
                                }
                                Err(SegmentAttemptError::FallbackSingleStream(reason)) => {
                                    cleanup_segment_files(&part_path, seg_count);
                                    debug!(url = %url, reason = %reason, "Falling back to single-stream download");
                                    allow_segmented = false;
                                    break;
                                }
                                Err(SegmentAttemptError::Permanent(reason)) => {
                                    cleanup_segment_files(&part_path, seg_count);
                                    last_err = reason;
                                    allow_segmented = false;
                                    break;
                                }
                                Err(SegmentAttemptError::Retryable(reason)) => {
                                    cleanup_segment_files(&part_path, seg_count);
                                    last_err = reason;
                                    if attempt < self.max_retries {
                                        sleep(Duration::from_millis(250 * (1 << attempt.min(4))))
                                            .await;
                                    }
                                }
                            }
                        }
                        if allow_segmented {
                            // Retries exhausted on segmented download; try next mirror URL
                            let _ = tokio::fs::remove_file(&part_path).await;
                            continue;
                        }
                    }
                }
            }

            for attempt in 0..=self.max_retries {
                self.check_control().await?;
                match self
                    .download_once_with_progress(url, &part_path, req, Arc::clone(&on_bytes_cb))
                    .await
                {
                    Ok(bytes_written) => {
                        verify_file(&part_path, req.expected_sha1.as_deref(), req.expected_size)?;
                        rename_with_retry(&part_path, &req.destination)?;
                        return Ok(DownloadOutcome {
                            destination: req.destination.clone(),
                            skipped: false,
                            bytes_written,
                        });
                    }
                    Err(NetError::Cancelled) => {
                        return Err(NetError::Cancelled);
                    }
                    Err(err) => {
                        let non_retryable = is_permanent_download_error(&err);
                        last_err = err.to_string();
                        debug!(
                            url = %url,
                            attempt = attempt + 1,
                            error = %last_err,
                            "download attempt failed"
                        );
                        if matches!(
                            err,
                            NetError::HashMismatch { .. } | NetError::SizeMismatch { .. }
                        ) {
                            let _ = tokio::fs::remove_file(&part_path).await;
                        }
                        if non_retryable {
                            break;
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

    async fn probe_range_length(&self, url: &str, expected_size: u64) -> Option<u64> {
        let resp = self
            .http
            .head(url)
            .header(USER_AGENT, mcw_models::user_agent())
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let accepts_ranges = resp
            .headers()
            .get(ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("bytes"));
        if !accepts_ranges {
            return None;
        }
        let head_len = resp
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(0);
        if expected_size > 0 && head_len > 0 && head_len != expected_size {
            return None;
        }
        let total = if expected_size > 0 {
            expected_size
        } else {
            head_len
        };
        (total >= SEGMENT_MIN_BYTES).then_some(total)
    }

    async fn download_segmented(
        &self,
        url: &str,
        part_path: &Path,
        total_len: u64,
        segments: usize,
        on_bytes: Arc<dyn Fn(u64, u64) + Send + Sync>,
    ) -> std::result::Result<u64, SegmentAttemptError> {
        let ranges = compute_byte_ranges(total_len, segments);
        if ranges.len() <= 1 {
            return Err(SegmentAttemptError::FallbackSingleStream(
                "file too small for multiple segments".to_string(),
            ));
        }

        let shared_done = Arc::new(AtomicU64::new(0));
        let last_emitted = Arc::new(AtomicU64::new(0));
        on_bytes(0, total_len);

        let jobs: Vec<_> = ranges
            .into_iter()
            .enumerate()
            .map(|(idx, (start, end))| (idx, start, end))
            .collect();
        let seg_total = jobs.len();

        let results: Vec<std::result::Result<(usize, PathBuf, u64), SegmentAttemptError>> =
            stream::iter(jobs)
                .map(|(idx, start, end)| {
                    let client = self.clone();
                    let url = url.to_string();
                    let seg_path = segment_path_for(part_path, idx);
                    let shared_done = Arc::clone(&shared_done);
                    let last_emitted = Arc::clone(&last_emitted);
                    let on_bytes = Arc::clone(&on_bytes);
                    async move {
                        client
                            .download_segment_part(
                                &url,
                                &seg_path,
                                start,
                                end,
                                total_len,
                                shared_done,
                                last_emitted,
                                on_bytes,
                            )
                            .await
                            .map(|len| (idx, seg_path, len))
                    }
                })
                .buffer_unordered(seg_total)
                .collect()
                .await;

        let mut seg_files = vec![None; seg_total];
        let mut combined_written = 0u64;
        for res in results {
            let (idx, path, len) = res?;
            combined_written += len;
            seg_files[idx] = Some(path);
        }
        on_bytes(total_len, total_len);

        let ordered_paths: Vec<PathBuf> = seg_files.into_iter().flatten().collect();
        let dest_part = part_path.to_path_buf();
        tokio::task::spawn_blocking(move || combine_segment_files(&ordered_paths, &dest_part))
            .await
            .map_err(|e| SegmentAttemptError::Retryable(format!("Segment join panicked: {e}")))?
            .map_err(|source| SegmentAttemptError::Io {
                path: part_path.display().to_string(),
                source,
            })?;

        Ok(combined_written)
    }

    #[allow(clippy::too_many_arguments)]
    async fn download_segment_part(
        &self,
        url: &str,
        seg_path: &Path,
        start: u64,
        end: u64,
        total_len: u64,
        shared_done: Arc<AtomicU64>,
        last_emitted: Arc<AtomicU64>,
        on_bytes: Arc<dyn Fn(u64, u64) + Send + Sync>,
    ) -> std::result::Result<u64, SegmentAttemptError> {
        self.check_control()
            .await
            .map_err(|_| SegmentAttemptError::Cancelled)?;

        let expected_seg_len = end - start + 1;
        let response = self
            .http
            .get(url)
            .header(USER_AGENT, mcw_models::user_agent())
            .header(RANGE, format!("bytes={start}-{end}"))
            .send()
            .await
            .map_err(|e| SegmentAttemptError::Retryable(e.to_string()))?;

        let status = response.status();
        if status == StatusCode::OK {
            return Err(SegmentAttemptError::FallbackSingleStream(
                "Server returned HTTP 200 instead of 206 Partial Content".to_string(),
            ));
        }
        if status != StatusCode::PARTIAL_CONTENT {
            let msg = format!("HTTP {status}");
            return if is_retryable_status(status) {
                Err(SegmentAttemptError::Retryable(msg))
            } else {
                Err(SegmentAttemptError::Permanent(msg))
            };
        }

        let raw_file =
            tokio::fs::File::create(seg_path)
                .await
                .map_err(|source| SegmentAttemptError::Io {
                    path: seg_path.display().to_string(),
                    source,
                })?;
        let mut writer = tokio::io::BufWriter::with_capacity(DISK_BUF_BYTES, raw_file);
        let mut seg_written = 0u64;
        let mut stream = response.bytes_stream();

        while let Some(chunk_res) = stream.next().await {
            self.check_control()
                .await
                .map_err(|_| SegmentAttemptError::Cancelled)?;
            let chunk = chunk_res.map_err(|e| SegmentAttemptError::Retryable(e.to_string()))?;
            writer
                .write_all(&chunk)
                .await
                .map_err(|source| SegmentAttemptError::Io {
                    path: seg_path.display().to_string(),
                    source,
                })?;
            let n = chunk.len() as u64;
            seg_written += n;
            GLOBAL_DOWNLOADED_BYTES.fetch_add(n, Ordering::Relaxed);
            let done_now = shared_done.fetch_add(n, Ordering::Relaxed) + n;
            let prev = last_emitted.load(Ordering::Relaxed);
            if done_now.saturating_sub(prev) >= PROGRESS_THROTTLE_BYTES
                && last_emitted
                    .compare_exchange(prev, done_now, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
            {
                on_bytes(done_now.min(total_len), total_len);
            }
            bandwidth::throttle_bytes(chunk.len()).await;
        }
        writer
            .flush()
            .await
            .map_err(|source| SegmentAttemptError::Io {
                path: seg_path.display().to_string(),
                source,
            })?;

        if seg_written != expected_seg_len {
            return Err(SegmentAttemptError::Retryable(format!(
                "Segment length mismatch: expected {expected_seg_len}, got {seg_written}"
            )));
        }

        Ok(seg_written)
    }

    async fn download_once_with_progress(
        &self,
        url: &str,
        part_path: &Path,
        req: &DownloadRequest,
        on_bytes: Arc<dyn Fn(u64, u64) + Send + Sync>,
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
            let suffix = if !is_retryable_status(status) {
                " (non-retryable)"
            } else {
                ""
            };
            return Err(NetError::DownloadFailed {
                url: url.to_string(),
                reason: format!("HTTP {status}{suffix}"),
            });
        }

        let append_mode = resume_from > 0 && status == StatusCode::PARTIAL_CONTENT;
        let total_len = if req.expected_size > 0 {
            req.expected_size
        } else if let Some(cl) = response.content_length() {
            if append_mode {
                resume_from + cl
            } else {
                cl
            }
        } else {
            0
        };

        let raw_file = if append_mode {
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
        let mut file = tokio::io::BufWriter::with_capacity(DISK_BUF_BYTES, raw_file);

        let mut written = if append_mode { resume_from } else { 0 };
        let mut last_reported = written;
        on_bytes(written, total_len);

        let mut stream = response.bytes_stream();
        while let Some(chunk_res) = stream.next().await {
            self.check_control().await?;
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
            if written.saturating_sub(last_reported) >= PROGRESS_THROTTLE_BYTES {
                last_reported = written;
                on_bytes(written, total_len);
            }
            bandwidth::throttle_bytes(chunk.len()).await;
        }
        file.flush().await.map_err(|source| NetError::Io {
            path: part_path.display().to_string(),
            source,
        })?;
        on_bytes(written, total_len.max(written));

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedVerifyEntry {
    path: String,
    size: u64,
    mtime_ns: u128,
    sha1: String,
}

/// Initialize the persistent verification cache from disk so unchanged assets/libraries are
/// never re-hashed across launcher restarts.
pub fn init_persistent_verification_cache(cache_file: &Path) {
    if let Ok(mut path_guard) = verification_cache_file().lock() {
        *path_guard = Some(cache_file.to_path_buf());
    }
    let Ok(raw) = fs::read_to_string(cache_file) else {
        return;
    };
    let Ok(entries) = serde_json::from_str::<Vec<PersistedVerifyEntry>>(&raw) else {
        return;
    };
    if let Ok(mut guard) = verification_cache().lock() {
        for item in entries {
            let p = PathBuf::from(&item.path);
            if p.is_file() {
                guard.insert((p, item.size, item.mtime_ns), item.sha1);
            }
        }
    }
}

/// Flush the in-memory verification cache to disk atomically (bounded to 50,000 entries).
pub fn flush_persistent_verification_cache() {
    let cache_path = verification_cache_file()
        .lock()
        .ok()
        .and_then(|g| g.clone());
    let Some(cache_path) = cache_path else {
        return;
    };
    let entries: Vec<PersistedVerifyEntry> = {
        let Ok(guard) = verification_cache().lock() else {
            return;
        };
        guard
            .iter()
            .take(MAX_PERSISTED_VERIFY_ENTRIES)
            .map(|((path, size, mtime_ns), sha1)| PersistedVerifyEntry {
                path: path.to_string_lossy().to_string(),
                size: *size,
                mtime_ns: *mtime_ns,
                sha1: sha1.clone(),
            })
            .collect()
    };
    VERIFICATION_CACHE_DIRTY.store(0, Ordering::Relaxed);
    if let Ok(json) = serde_json::to_string(&entries) {
        let _ = write_text_atomic(&cache_path, &json);
    }
}

/// Verify file size (only when `expected_size > 0`) and SHA-1 (when provided),
/// using an in-memory + persistent `(path, size, mtime_ns)` verification cache so unchanged
/// libraries and assets are not re-hashed repeatedly.
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
            let mtime_ns = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let cache_key = (path.to_path_buf(), meta.len(), mtime_ns);
            if mtime_ns > 0 {
                if let Ok(guard) = verification_cache().lock() {
                    if let Some(cached_sha1) = guard.get(&cache_key) {
                        if cached_sha1 == &expected_clean {
                            return Ok(());
                        }
                    }
                }
            }

            let actual = compute_sha1(path)?;
            if actual != expected_clean {
                return Err(NetError::HashMismatch {
                    path: path.display().to_string(),
                    expected: expected_clean,
                    actual,
                });
            }
            if mtime_ns > 0 {
                if let Ok(mut guard) = verification_cache().lock() {
                    guard.insert(cache_key, actual);
                }
                if VERIFICATION_CACHE_DIRTY.fetch_add(1, Ordering::Relaxed) >= 128 {
                    flush_persistent_verification_cache();
                }
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

fn segment_path_for(part_path: &Path, idx: usize) -> PathBuf {
    let mut s = part_path.as_os_str().to_os_string();
    s.push(format!(".seg.{idx}"));
    PathBuf::from(s)
}

fn cleanup_segment_files(part_path: &Path, segments: usize) {
    for idx in 0..segments {
        let _ = fs::remove_file(segment_path_for(part_path, idx));
    }
}

fn combine_segment_files(seg_paths: &[PathBuf], dest_part: &Path) -> io::Result<()> {
    let out = File::create(dest_part)?;
    let mut writer = io::BufWriter::with_capacity(DISK_BUF_BYTES, out);
    for seg in seg_paths {
        let mut reader = io::BufReader::with_capacity(DISK_BUF_BYTES, File::open(seg)?);
        io::copy(&mut reader, &mut writer)?;
        drop(reader);
        let _ = fs::remove_file(seg);
    }
    io::Write::flush(&mut writer)?;
    Ok(())
}

fn etag_sidecar_path(cache_path: &Path) -> PathBuf {
    let mut s = cache_path.as_os_str().to_os_string();
    s.push(".etag");
    PathBuf::from(s)
}

fn write_text_atomic(dest: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = part_path_for(dest);
    fs::write(&tmp, content.as_bytes())?;
    let _ = fs::remove_file(dest);
    fs::rename(&tmp, dest)
}

fn is_permanent_download_error(err: &NetError) -> bool {
    matches!(err, NetError::DownloadFailed { reason, .. } if reason.contains("(non-retryable)"))
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

    #[test]
    fn computes_segmented_ranges_and_retryable_statuses() {
        assert_eq!(
            compute_byte_ranges(10, 4),
            vec![(0, 2), (3, 5), (6, 7), (8, 9)]
        );
        assert_eq!(compute_byte_ranges(100, 1), vec![(0, 99)]);
        assert!(!is_retryable_status(StatusCode::NOT_FOUND));
        assert!(!is_retryable_status(StatusCode::FORBIDDEN));
        assert!(is_retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable_status(StatusCode::SERVICE_UNAVAILABLE));
    }

    #[test]
    fn persistent_verification_cache_roundtrips_to_disk() {
        let dir = tempdir().unwrap();
        let sample = dir.path().join("asset.dat");
        let cache_file = dir.path().join("verify.json");
        fs::write(&sample, b"hello world").unwrap();
        let sha1 = "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed";

        init_persistent_verification_cache(&cache_file);
        assert!(verify_file(&sample, Some(sha1), 11).is_ok());
        flush_persistent_verification_cache();

        assert!(cache_file.is_file());
        if let Ok(mut guard) = verification_cache().lock() {
            guard.clear();
        }
        init_persistent_verification_cache(&cache_file);
        if let Ok(guard) = verification_cache().lock() {
            assert!(guard.values().any(|v| v == sha1));
        }
    }
}
