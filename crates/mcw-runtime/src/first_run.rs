use crate::{
    detect_total_memory_mb, GpuPreferenceManager, GraphicsDetectionResult, JavaDiscovery,
    JavaInstallation,
};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct FirstRunRecommendation {
    pub total_memory_mb: u32,
    pub available_memory_mb: u32,
    pub recommended_min_memory_mb: u32,
    pub recommended_max_memory_mb: u32,
    pub java_installations: Vec<JavaInstallation>,
    pub recommended_java: Option<JavaInstallation>,
    pub gpu_detection: GraphicsDetectionResult,
}

impl FirstRunRecommendation {
    pub fn java_majors(&self) -> Vec<u32> {
        let mut set = BTreeSet::new();
        for j in &self.java_installations {
            if j.major > 0 {
                set.insert(j.major);
            }
        }
        set.into_iter().collect()
    }
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
        let gpu_detection = GpuPreferenceManager::detect();

        FirstRunRecommendation {
            total_memory_mb,
            available_memory_mb,
            recommended_min_memory_mb,
            recommended_max_memory_mb,
            java_installations,
            recommended_java,
            gpu_detection,
        }
    }

    /// Recommended max memory using `max(4096 MB, 50% of total RAM)` snapped to 256 MB steps,
    /// capped at 16384 MB and clamped safely on low-RAM machines (`<= total_mb - 1024`).
    pub fn recommended_max_memory_mb(total_mb: u32, _avail_mb: u32) -> u32 {
        if total_mb == 0 {
            return 4096;
        }
        let raw = 4096u32.max(total_mb / 2);
        let safe_limit = total_mb.saturating_sub(1024).max(512);
        let clamped = raw.min(16384).min(safe_limit);
        let snapped = ((clamped / 256) * 256).max(512);
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
    fn computes_first_run_memory_tiers_and_caps_matching_formula() {
        assert_eq!(
            FirstRunRecommendationService::recommended_max_memory_mb(2048, 1024),
            1024
        );
        assert_eq!(
            FirstRunRecommendationService::recommended_max_memory_mb(4096, 2048),
            3072
        );
        assert_eq!(
            FirstRunRecommendationService::recommended_max_memory_mb(6144, 4096),
            4096
        );
        assert_eq!(
            FirstRunRecommendationService::recommended_max_memory_mb(8192, 6000),
            4096
        );
        assert_eq!(
            FirstRunRecommendationService::recommended_max_memory_mb(16087, 4071),
            7936
        );
        assert_eq!(
            FirstRunRecommendationService::recommended_max_memory_mb(16384, 3000),
            8192
        );
        assert_eq!(
            FirstRunRecommendationService::recommended_max_memory_mb(32768, 24000),
            16384
        );
        assert_eq!(
            FirstRunRecommendationService::recommended_max_memory_mb(65536, 48000),
            16384
        );
        assert_eq!(
            FirstRunRecommendationService::recommended_min_memory_mb(4096),
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
