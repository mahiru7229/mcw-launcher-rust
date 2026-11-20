use crate::java::{JavaDiscovery, JavaInstallation};
use crate::{Result, RuntimeError};
use regex::Regex;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_RECOVERY_EVENTS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaDiagnostic {
    pub major_version: u32,
    pub version_string: String,
    pub vendor: String,
    pub architecture: String,
    pub bitness: u32,
    pub java_home: String,
    pub executable: PathBuf,
    pub source: String,
    pub is_managed: bool,
    pub valid: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaRecoveryEvent {
    pub timestamp_unix: u64,
    pub event: String,
    pub detail: String,
}

static RECOVERY_EVENTS: OnceLock<Mutex<VecDeque<JavaRecoveryEvent>>> = OnceLock::new();

fn recovery_ring() -> &'static Mutex<VecDeque<JavaRecoveryEvent>> {
    RECOVERY_EVENTS.get_or_init(|| Mutex::new(VecDeque::with_capacity(MAX_RECOVERY_EVENTS)))
}

/// Bounded in-memory trace of automatic Java selection/provisioning decisions
/// (port of Python `JavaRecoveryDiagnostics`).
pub struct JavaRecoveryDiagnostics;

impl JavaRecoveryDiagnostics {
    pub fn record(event: impl Into<String>, detail: impl Into<String>) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let ev_str = event.into();
        let trimmed = ev_str.trim();
        let row = JavaRecoveryEvent {
            timestamp_unix: now,
            event: if trimmed.is_empty() {
                "unknown".to_string()
            } else {
                trimmed.to_string()
            },
            detail: detail.into(),
        };
        if let Ok(mut guard) = recovery_ring().lock() {
            if guard.len() >= MAX_RECOVERY_EVENTS {
                guard.pop_front();
            }
            guard.push_back(row);
        }
    }

    pub fn snapshot() -> Vec<JavaRecoveryEvent> {
        recovery_ring()
            .lock()
            .map(|g| g.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn clear() {
        if let Ok(mut guard) = recovery_ring().lock() {
            guard.clear();
        }
    }
}

/// Port of Python `JavaDiagnosticsManager` (`src/core/java/java_diagnostics_manager.py`).
pub struct JavaDiagnosticsManager;

impl JavaDiagnosticsManager {
    /// Parses JVM `-XshowSettings:properties` output into key -> value pairs.
    pub fn parse_properties(output: &str) -> HashMap<String, String> {
        let re = Regex::new(r"^\s*([^=]+?)\s*=\s*(.*?)\s*$").expect("valid property regex");
        let mut properties = HashMap::new();
        for line in output.lines() {
            if let Some(caps) = re.captures(line) {
                let key = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                let val = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
                if !key.is_empty() {
                    properties.insert(key.to_string(), val.to_string());
                }
            }
        }
        properties
    }

    /// Probes all discovered Java installations with vendor, arch, bitness, and source metadata.
    pub fn scan(runtimes_root: Option<&Path>) -> Vec<JavaDiagnostic> {
        let installs = JavaDiscovery::discover(runtimes_root);
        let mut diagnostics: Vec<JavaDiagnostic> = installs
            .iter()
            .map(|j| Self::inspect(j, runtimes_root))
            .collect();
        diagnostics.sort_by(|a, b| {
            a.major_version
                .cmp(&b.major_version)
                .then_with(|| {
                    a.vendor
                        .to_ascii_lowercase()
                        .cmp(&b.vendor.to_ascii_lowercase())
                })
                .then_with(|| {
                    a.executable
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .cmp(&b.executable.to_string_lossy().to_ascii_lowercase())
                })
        });
        diagnostics
    }

    /// Inspects a single `JavaInstallation` for full properties (`java -XshowSettings:properties -version`).
    pub fn inspect(java: &JavaInstallation, runtimes_root: Option<&Path>) -> JavaDiagnostic {
        let props = Self::read_properties(&java.path);
        let version_string = props
            .get("java.version")
            .cloned()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| java.version_string.clone());
        let vendor = props.get("java.vendor").cloned().unwrap_or_default();
        let architecture = props.get("os.arch").cloned().unwrap_or_default();
        let java_home = props.get("java.home").cloned().unwrap_or_default();

        let bitness = match props.get("sun.arch.data.model").map(String::as_str) {
            Some("64") => 64,
            Some("32") => 32,
            _ => infer_bitness(&architecture),
        };

        let is_managed = is_path_under_opt(&java.path, runtimes_root);
        let source = if is_managed {
            "managed".to_string()
        } else if std::env::var("JAVA_HOME")
            .map(PathBuf::from)
            .ok()
            .is_some_and(|jh| is_path_under(&java.path, &jh))
        {
            "java_home".to_string()
        } else {
            "system".to_string()
        };

        JavaDiagnostic {
            major_version: java.major,
            version_string,
            vendor,
            architecture,
            bitness,
            java_home,
            executable: java.path.clone(),
            source,
            is_managed,
            valid: !props.is_empty(),
        }
    }

    fn read_properties(executable: &Path) -> HashMap<String, String> {
        let console_exe = console_java_executable(executable);
        let mut cmd = Command::new(&console_exe);
        cmd.args(["-XshowSettings:properties", "-version"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let Ok(output) = cmd.output() else {
            return HashMap::new();
        };
        let combined = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Self::parse_properties(&combined)
    }

    /// Returns the JDK root directory containing `executable` (parent of `bin/`).
    pub fn java_root_dir(executable: &Path) -> PathBuf {
        match executable.parent() {
            Some(parent)
                if parent
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case("bin")) =>
            {
                parent.parent().unwrap_or(parent).to_path_buf()
            }
            Some(parent) => parent.to_path_buf(),
            None => executable.to_path_buf(),
        }
    }

    /// Safely uninstalls a launcher-managed Java runtime under `runtimes_root`.
    /// Refuses with an error if `executable` is not strictly inside `runtimes_root`.
    pub fn uninstall_managed(runtimes_root: &Path, executable: &Path) -> Result<bool> {
        let canonical_root =
            fs::canonicalize(runtimes_root).unwrap_or_else(|_| runtimes_root.to_path_buf());
        let canonical_exe =
            fs::canonicalize(executable).unwrap_or_else(|_| executable.to_path_buf());
        let rel = canonical_exe.strip_prefix(&canonical_root).map_err(|_| {
            RuntimeError::Process(format!(
                "Refusing to uninstall external Java runtime outside managed runtimes/: {}",
                executable.display()
            ))
        })?;
        let Some(first_component) = rel.components().next() else {
            return Err(RuntimeError::Process(
                "Invalid managed Java runtime path".to_string(),
            ));
        };
        let runtime_dir = canonical_root.join(first_component);
        if !runtime_dir.is_dir() {
            return Ok(false);
        }
        fs::remove_dir_all(&runtime_dir)?;
        JavaRecoveryDiagnostics::record(
            "java.uninstall",
            format!("Uninstalled managed runtime {}", runtime_dir.display()),
        );
        Ok(true)
    }
}

fn console_java_executable(executable: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(name) = executable.file_name().and_then(|n| n.to_str()) {
            if name.eq_ignore_ascii_case("javaw.exe") {
                let console = executable.with_file_name("java.exe");
                if console.is_file() {
                    return console;
                }
            }
        }
    }
    executable.to_path_buf()
}

fn infer_bitness(arch: &str) -> u32 {
    let lower = arch.trim().to_ascii_lowercase();
    if lower.contains("64") || lower == "aarch64" {
        64
    } else if !lower.is_empty() {
        32
    } else if cfg!(target_pointer_width = "64") {
        64
    } else {
        32
    }
}

fn is_path_under_opt(child: &Path, root: Option<&Path>) -> bool {
    root.is_some_and(|r| is_path_under(child, r))
}

fn is_path_under(child: &Path, root: &Path) -> bool {
    let c = fs::canonicalize(child).unwrap_or_else(|_| child.to_path_buf());
    let r = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    c.starts_with(&r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_jvm_settings_properties_cleanly() {
        let sample = r#"
Property settings:
    java.home = C:\Program Files\Eclipse Adoptium\jdk-21.0.5
    java.vendor = Eclipse Adoptium
    java.version = 21.0.5
    os.arch = amd64
    sun.arch.data.model = 64
"#;
        let map = JavaDiagnosticsManager::parse_properties(sample);
        assert_eq!(map.get("java.vendor").unwrap(), "Eclipse Adoptium");
        assert_eq!(map.get("java.version").unwrap(), "21.0.5");
        assert_eq!(map.get("os.arch").unwrap(), "amd64");
        assert_eq!(map.get("sun.arch.data.model").unwrap(), "64");
    }

    #[test]
    fn uninstalls_only_managed_runtimes_inside_root() {
        let dir = tempdir().unwrap();
        let runtimes = dir.path().join("runtimes");
        let jdk21 = runtimes.join("java-21").join("bin");
        fs::create_dir_all(&jdk21).unwrap();
        let managed_exe = jdk21.join("java.exe");
        fs::write(&managed_exe, b"mock-java").unwrap();

        let outside = dir.path().join("system_java").join("bin");
        fs::create_dir_all(&outside).unwrap();
        let outside_exe = outside.join("java.exe");
        fs::write(&outside_exe, b"sys-java").unwrap();

        // Must refuse deleting outside runtime
        assert!(JavaDiagnosticsManager::uninstall_managed(&runtimes, &outside_exe).is_err());
        assert!(outside_exe.is_file());

        // Must succeed deleting managed runtime directory
        assert!(JavaDiagnosticsManager::uninstall_managed(&runtimes, &managed_exe).unwrap());
        assert!(!runtimes.join("java-21").exists());
    }

    #[test]
    fn recovery_diagnostics_enforces_max_capacity() {
        JavaRecoveryDiagnostics::clear();
        for i in 0..80 {
            JavaRecoveryDiagnostics::record("select", format!("step-{i}"));
        }
        let snap = JavaRecoveryDiagnostics::snapshot();
        assert_eq!(snap.len(), MAX_RECOVERY_EVENTS);
        assert_eq!(snap.first().unwrap().detail, "step-16");
        assert_eq!(snap.last().unwrap().detail, "step-79");
        JavaRecoveryDiagnostics::clear();
    }
}
