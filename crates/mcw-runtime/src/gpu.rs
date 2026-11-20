use std::path::Path;
use std::process::Command;

pub const WINDOWS_GPU_PREF_REG_PATH: &str = r"Software\Microsoft\DirectX\UserGpuPreferences";
pub const HIGH_PERFORMANCE_VALUE: &str = "GpuPreference=2;";

pub struct GpuPreferenceManager;

impl GpuPreferenceManager {
    pub fn is_dedicated_adapter_name(name: &str) -> bool {
        let lower = name
            .trim()
            .to_ascii_lowercase()
            .replace("(r)", "")
            .replace("(tm)", "");
        let lower = lower.split_whitespace().collect::<Vec<_>>().join(" ");
        if lower.is_empty() {
            return false;
        }
        let software_or_integrated = [
            "microsoft basic display",
            "microsoft remote display",
            "remote display adapter",
            "virtualbox",
            "vmware",
            "parallels",
            "intel hd graphics",
            "intel uhd graphics",
            "intel iris",
            "radeon graphics",
            "vega 3 graphics",
            "vega 6 graphics",
            "vega 7 graphics",
            "vega 8 graphics",
            "vega 10 graphics",
            "vega 11 graphics",
        ];
        if software_or_integrated.iter().any(|t| lower.contains(t)) {
            return false;
        }
        let dedicated_tokens = [
            "nvidia",
            "geforce",
            "quadro",
            "rtx",
            "gtx",
            "radeon rx",
            "radeon r9",
            "radeon r7",
            "radeon r5",
            "radeon pro",
            "firepro",
            "intel arc",
        ];
        dedicated_tokens.iter().any(|t| lower.contains(t))
    }

    /// Best-effort Windows DirectX High-Performance GPU preference registration for `java_exe`.
    pub fn apply_windows_java_preference(java_exe: &Path, prefer_dedicated: bool) {
        if !prefer_dedicated {
            return;
        }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let key = format!(r"HKCU\{WINDOWS_GPU_PREF_REG_PATH}");
            let exe = java_exe.display().to_string();
            let _ = Command::new("reg.exe")
                .args([
                    "add",
                    &key,
                    "/v",
                    &exe,
                    "/t",
                    "REG_SZ",
                    "/d",
                    HIGH_PERFORMANCE_VALUE,
                    "/f",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = java_exe;
        }
    }

    /// Environment overrides for Linux dedicated GPU offload (`DRI_PRIME` / NVIDIA PRIME).
    pub fn linux_gpu_env_vars(prefer_dedicated: bool) -> Vec<(&'static str, &'static str)> {
        if !prefer_dedicated {
            return Vec::new();
        }
        vec![
            ("DRI_PRIME", "1"),
            ("__NV_PRIME_RENDER_OFFLOAD", "1"),
            ("__GLX_VENDOR_LIBRARY_NAME", "nvidia"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_dedicated_vs_integrated_adapters() {
        assert!(GpuPreferenceManager::is_dedicated_adapter_name(
            "NVIDIA GeForce RTX 4070"
        ));
        assert!(GpuPreferenceManager::is_dedicated_adapter_name(
            "AMD Radeon RX 7800 XT"
        ));
        assert!(GpuPreferenceManager::is_dedicated_adapter_name(
            "Intel(R) Arc(TM) A770"
        ));
        assert!(!GpuPreferenceManager::is_dedicated_adapter_name(
            "Intel(R) UHD Graphics 770"
        ));
        assert!(!GpuPreferenceManager::is_dedicated_adapter_name(
            "Microsoft Basic Display Adapter"
        ));
        assert_eq!(GpuPreferenceManager::linux_gpu_env_vars(false).len(), 0);
        assert_eq!(GpuPreferenceManager::linux_gpu_env_vars(true).len(), 3);
    }
}
