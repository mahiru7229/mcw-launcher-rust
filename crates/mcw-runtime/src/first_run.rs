use crate::{detect_total_memory_mb, JavaDiscovery, JavaInstallation};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct FirstRunRecommendation {
    pub total_memory_mb: u32,
    pub available_memory_mb: u32,
    pub recommended_min_memory_mb: u32,
    pub recommended_max_memory_mb: u32,
    pub java_installations: Vec<JavaInstallation>,
    pub recommended_java: Option<JavaInstallation>,
}

/// Detect currently available physical system RAM in megabytes (MB).
pub fn detect_available_memory_mb() -> u32 {
    #[cfg(target_os = "windows")]
    {
        #[repr(C)]
        struct MemoryStatusEx {
            dw_length: u32,
            dw_memory_load: u32,
            ull_total_phys: u64,
            ull_avail_phys: u64,
            ull_total_page_file: u64,
            ull_avail_page_file: u64,
            ull_total_virtual: u64,
            ull_avail_virtual: u64,
            ull_avail_extended_virtual: u64,
        }
        extern "system" {
            fn GlobalMemoryStatusEx(lp_buffer: *mut MemoryStatusEx) -> i32;
        }
        let mut status = MemoryStatusEx {
            dw_length: std::mem::size_of::<MemoryStatusEx>() as u32,
            dw_memory_load: 0,
            ull_total_phys: 0,
            ull_avail_phys: 0,
            ull_total_page_file: 0,
            ull_avail_page_file: 0,
            ull_total_virtual: 0,
            ull_avail_virtual: 0,
            ull_avail_extended_virtual: 0,
        };
        // SAFETY: `status` is a properly sized `MEMORYSTATUSEX` struct allocated on the stack.
        let ok = unsafe { GlobalMemoryStatusEx(&mut status as *mut MemoryStatusEx) };
        if ok != 0 && status.ull_avail_phys > 0 {
            return (status.ull_avail_phys / (1024 * 1024)) as u32;
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            let mut mem_free: Option<u32> = None;
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("MemAvailable:") {
                    if let Some(kb) = rest
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse::<u64>().ok())
                    {
                        return (kb / 1024) as u32;
                    }
                } else if let Some(rest) = line.strip_prefix("MemFree:") {
                    if let Some(kb) = rest
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse::<u64>().ok())
                    {
                        mem_free = Some((kb / 1024) as u32);
                    }
                }
            }
            if let Some(mf) = mem_free {
                return mf;
            }
        }
    }
    detect_total_memory_mb() / 2
}

pub struct FirstRunRecommendationService;

impl FirstRunRecommendationService {
    pub fn inspect(managed_runtimes_root: &Path) -> FirstRunRecommendation {
        let total_memory_mb = detect_total_memory_mb();
        let available_memory_mb = detect_available_memory_mb();
        let recommended_max_memory_mb =
            Self::recommended_max_memory_mb(total_memory_mb, available_memory_mb);
        let recommended_min_memory_mb = Self::recommended_min_memory_mb(recommended_max_memory_mb);
        let java_installations = JavaDiscovery::discover(Some(managed_runtimes_root));
        let recommended_java = Self::select_preferred_java(&java_installations);

        FirstRunRecommendation {
            total_memory_mb,
            available_memory_mb,
            recommended_min_memory_mb,
            recommended_max_memory_mb,
            java_installations,
            recommended_java,
        }
    }

    /// Exact parity with Python `FirstRunRecommendationService.recommended_max_memory_mb`.
    pub fn recommended_max_memory_mb(total_mb: u32, avail_mb: u32) -> u32 {
        if total_mb == 0 {
            return 2048;
        }
        let mut rec = if total_mb < 4096 {
            1024
        } else if total_mb < 8192 {
            2048
        } else if total_mb < 12288 {
            4096
        } else if total_mb < 24576 {
            6144
        } else {
            8192
        };

        if avail_mb > 0 {
            let safe_avail = avail_mb.saturating_sub(1536).max(1024);
            rec = rec.min(safe_avail);
        }
        let sixty_pct = ((total_mb as f64 * 0.60) as u32).max(1024);
        rec = rec.min(sixty_pct);

        // Snap down to 128 MB step, never below 1024 MB unless physical total is lower.
        let snapped = ((rec / 128) * 128).max(1024);
        snapped.min(total_mb.max(512))
    }

    pub fn recommended_min_memory_mb(max_mb: u32) -> u32 {
        1024u32.min(max_mb)
    }

    pub fn select_preferred_java(installations: &[JavaInstallation]) -> Option<JavaInstallation> {
        let priority = |major: u32| -> u32 {
            match major {
                21 => 0,
                17 => 1,
                25 => 2,
                26 => 3,
                8 => 4,
                _ => 50,
            }
        };
        installations
            .iter()
            .filter(|j| j.major > 0)
            .min_by_key(|j| (priority(j.major), j.path.as_os_str().len()))
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn computes_first_run_memory_tiers_and_caps_matching_python() {
        assert_eq!(
            FirstRunRecommendationService::recommended_max_memory_mb(3072, 2048),
            1024
        );
        assert_eq!(
            FirstRunRecommendationService::recommended_max_memory_mb(6144, 4096),
            2048
        );
        assert_eq!(
            FirstRunRecommendationService::recommended_max_memory_mb(8192, 6000),
            4096
        );
        assert_eq!(
            FirstRunRecommendationService::recommended_max_memory_mb(16384, 12000),
            6144
        );
        assert_eq!(
            FirstRunRecommendationService::recommended_max_memory_mb(32768, 24000),
            8192
        );
        // Low available memory tightens the ceiling safely
        assert_eq!(
            FirstRunRecommendationService::recommended_max_memory_mb(16384, 3000),
            1408
        );
        assert_eq!(
            FirstRunRecommendationService::recommended_min_memory_mb(2048),
            1024
        );
    }

    #[test]
    fn prefers_java_21_then_17_then_25_then_26_then_8() {
        let installs = vec![
            JavaInstallation {
                path: PathBuf::from("/jdk8/bin/java"),
                major: 8,
                version_string: "1.8.0".to_string(),
            },
            JavaInstallation {
                path: PathBuf::from("/jdk17/bin/java"),
                major: 17,
                version_string: "17.0.10".to_string(),
            },
            JavaInstallation {
                path: PathBuf::from("/jdk21/bin/java"),
                major: 21,
                version_string: "21.0.2".to_string(),
            },
        ];
        let best = FirstRunRecommendationService::select_preferred_java(&installs).unwrap();
        assert_eq!(best.major, 21);
    }
}
