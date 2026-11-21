use std::path::Path;
use std::process::Command;

pub const WINDOWS_GPU_PREF_REG_PATH: &str = r"Software\Microsoft\DirectX\UserGpuPreferences";
pub const HIGH_PERFORMANCE_VALUE: &str = "GpuPreference=2;";

#[derive(Debug, Clone, Default)]
pub struct GraphicsAdapter {
    pub name: String,
    pub vendor: String,
    pub adapter_ram: u64,
    pub dedicated: bool,
}

#[derive(Debug, Clone, Default)]
pub struct GraphicsDetectionResult {
    pub supported: bool,
    pub adapters: Vec<GraphicsAdapter>,
    pub error: String,
}

impl GraphicsDetectionResult {
    pub fn dedicated_adapters(&self) -> Vec<&GraphicsAdapter> {
        self.adapters.iter().filter(|a| a.dedicated).collect()
    }

    pub fn has_dedicated_gpu(&self) -> bool {
        self.adapters.iter().any(|a| a.dedicated)
    }
}

pub struct GpuPreferenceManager;

impl GpuPreferenceManager {
    pub fn is_software_adapter_name(name: &str) -> bool {
        let lower = name.trim().to_ascii_lowercase();
        lower.is_empty()
            || [
                "microsoft basic display",
                "microsoft remote display",
                "remote display adapter",
                "virtualbox",
                "vmware",
                "parallels",
            ]
            .iter()
            .any(|t| lower.contains(t))
    }

    pub fn detect() -> GraphicsDetectionResult {
        #[cfg(target_os = "windows")]
        {
            Self::detect_windows()
        }
        #[cfg(target_os = "linux")]
        {
            Self::detect_linux()
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        {
            GraphicsDetectionResult {
                supported: false,
                ..Default::default()
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn detect_windows() -> GraphicsDetectionResult {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let cmd = "Get-CimInstance Win32_VideoController | Select-Object Name,AdapterCompatibility,AdapterRAM | ConvertTo-Json -Compress";
        let output = match Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                cmd,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            Ok(o) => o,
            Err(err) => {
                return GraphicsDetectionResult {
                    supported: true,
                    error: err.to_string(),
                    ..Default::default()
                };
            }
        };
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return GraphicsDetectionResult {
                supported: true,
                error: if err.is_empty() {
                    "GPU detection failed".to_string()
                } else {
                    err
                },
                ..Default::default()
            };
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let trimmed = stdout.trim();
        if trimmed.is_empty() {
            return GraphicsDetectionResult {
                supported: true,
                ..Default::default()
            };
        }
        let parsed: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(err) => {
                return GraphicsDetectionResult {
                    supported: true,
                    error: err.to_string(),
                    ..Default::default()
                };
            }
        };
        let records: Vec<&serde_json::Value> = match &parsed {
            serde_json::Value::Array(arr) => arr.iter().collect(),
            serde_json::Value::Object(_) => vec![&parsed],
            _ => Vec::new(),
        };
        let mut adapters = Vec::new();
        for rec in records {
            let name = rec
                .get("Name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if Self::is_software_adapter_name(&name) {
                continue;
            }
            let vendor = rec
                .get("AdapterCompatibility")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let adapter_ram = rec.get("AdapterRAM").and_then(|v| v.as_u64()).unwrap_or(0);
            let combined = format!("{name} {vendor}");
            let dedicated = Self::is_dedicated_adapter_name(&combined);
            adapters.push(GraphicsAdapter {
                name,
                vendor,
                adapter_ram,
                dedicated,
            });
        }
        GraphicsDetectionResult {
            supported: true,
            adapters,
            error: String::new(),
        }
    }

    #[cfg(target_os = "linux")]
    fn detect_linux() -> GraphicsDetectionResult {
        let mut adapters = Vec::new();
        if let Ok(output) = Command::new("lspci").output() {
            if output.status.success() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    let lower = line.to_ascii_lowercase();
                    if lower.contains("vga") || lower.contains("3d") || lower.contains("display") {
                        let name = line
                            .split_once(':')
                            .and_then(|(_, rest)| rest.split_once(':'))
                            .map(|(_, dev)| dev.trim())
                            .unwrap_or(line.trim())
                            .to_string();
                        if !Self::is_software_adapter_name(&name) {
                            let dedicated = Self::is_dedicated_adapter_name(&name);
                            adapters.push(GraphicsAdapter {
                                name,
                                vendor: String::new(),
                                adapter_ram: 0,
                                dedicated,
                            });
                        }
                    }
                }
            }
        }
        GraphicsDetectionResult {
            supported: true,
            adapters,
            error: String::new(),
        }
    }

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
        #[cfg(target_os = "windows")]
        {
            if !prefer_dedicated {
                return;
            }
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
            let _ = (java_exe, prefer_dedicated);
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
