use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const BYTES_PER_MEGABYTE: f64 = 1024.0 * 1024.0;
pub const MAX_LIMIT_MBPS: f64 = 1024.0;

#[derive(Debug)]
struct LimiterState {
    limit_bps: f64,
    next_available_at: Instant,
}

fn limiter_state() -> &'static Mutex<LimiterState> {
    static STATE: OnceLock<Mutex<LimiterState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(LimiterState {
            limit_bps: 0.0,
            next_available_at: Instant::now(),
        })
    })
}

/// Normalize a user-provided bandwidth cap in MB/s (`<= 0` or non-finite disables throttling; capped at `1024.0` MB/s).
pub fn normalize_limit_mbps(raw: f64) -> f64 {
    if !raw.is_finite() || raw <= 0.0 {
        return 0.0;
    }
    raw.min(MAX_LIMIT_MBPS)
}

/// Configure the process-wide download bandwidth limit in MB/s (`0.0` = unlimited).
pub fn set_global_bandwidth_limit_mbps(mbps: f64) -> f64 {
    let normalized = normalize_limit_mbps(mbps);
    let new_bps = normalized * BYTES_PER_MEGABYTE;
    if let Ok(mut guard) = limiter_state().lock() {
        if (guard.limit_bps - new_bps).abs() >= 0.5 {
            guard.limit_bps = new_bps;
            guard.next_available_at = Instant::now();
        }
    }
    normalized
}

pub fn global_bandwidth_limit_mbps() -> f64 {
    limiter_state()
        .lock()
        .map(|g| g.limit_bps / BYTES_PER_MEGABYTE)
        .unwrap_or(0.0)
}

fn compute_throttle_delay(
    now: Instant,
    next_available_at: Instant,
    limit_bps: f64,
    byte_count: usize,
) -> (Duration, Instant) {
    if byte_count == 0 || limit_bps <= 0.0 {
        return (Duration::ZERO, now);
    }
    let slice = Duration::from_secs_f64(byte_count as f64 / limit_bps);
    let base = if next_available_at > now {
        next_available_at
    } else {
        now
    };
    let scheduled = base + slice;
    let delay = scheduled.saturating_duration_since(now);
    (delay, scheduled)
}

/// Pace async downloads to honor the configured `download_limit_mbps` without holding locks across `.await`.
pub async fn throttle_bytes(byte_count: usize) {
    if byte_count == 0 {
        return;
    }
    let delay = {
        let Ok(mut guard) = limiter_state().lock() else {
            return;
        };
        if guard.limit_bps <= 0.0 {
            return;
        }
        let now = Instant::now();
        let (delay, scheduled) =
            compute_throttle_delay(now, guard.next_available_at, guard.limit_bps, byte_count);
        guard.next_available_at = scheduled;
        delay
    };

    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_mbps_limits_matching_python() {
        assert_eq!(normalize_limit_mbps(0.0), 0.0);
        assert_eq!(normalize_limit_mbps(-3.5), 0.0);
        assert_eq!(normalize_limit_mbps(f64::NAN), 0.0);
        assert_eq!(normalize_limit_mbps(f64::INFINITY), 0.0);
        assert_eq!(normalize_limit_mbps(12.5), 12.5);
        assert_eq!(normalize_limit_mbps(5000.0), 1024.0);
    }

    #[test]
    fn schedules_sequential_throttle_slices() {
        let t0 = Instant::now();
        let limit_bps = 1024.0 * 1024.0; // 1 MB/s
        let (d1, next1) = compute_throttle_delay(t0, t0, limit_bps, 524_288);
        assert!((d1.as_secs_f64() - 0.5).abs() < 0.01);
        let (d2, _next2) = compute_throttle_delay(t0, next1, limit_bps, 524_288);
        assert!((d2.as_secs_f64() - 1.0).abs() < 0.01);
    }
}
