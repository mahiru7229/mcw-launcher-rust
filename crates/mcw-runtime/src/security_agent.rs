use crate::{Result, RuntimeError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// Embedded `mcw-security-agent.jar` (pure-Java instrumentation agent).
/// Built from `tools/mcw_security_agent/` and verified by SHA-256 on extraction.
const BUNDLED_AGENT_BYTES: &[u8] = include_bytes!("../assets/mcw-security-agent.jar");

pub const SECURITY_AGENT_FILENAME: &str = "mcw-security-agent.jar";
pub const SECURITY_AGENT_STATUS_FILENAME: &str = "mcw-security-agent-status.json";
pub const SECURITY_AGENT_SHA256: &str =
    "62eb3b62044a4d7717e4e5de6640ff63d8c71c7c2dfbc915c994239232fce98b";

/// Explicit runtime state emitted by `McwSecurityAgent`.
///
/// Notice that [`SecurityAgentRuntimeState::Initialized`] (agent loaded in JVM) is intentionally
/// distinct from [`SecurityAgentRuntimeState::ActiveVerified`] (FreeType class patched successfully).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SecurityAgentRuntimeState {
    NotLoaded,
    Dormant,
    Initialized,
    ActiveVerified,
    Incompatible,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityAgentStatusReport {
    #[serde(default)]
    pub schema_version: u32,
    pub state: SecurityAgentRuntimeState,
    #[serde(default)]
    pub java_version: String,
    #[serde(default)]
    pub java_major: u32,
    #[serde(default)]
    pub patched_methods: u32,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub updated_epoch_ms: u64,
}

pub struct SecurityAgentManager;

impl SecurityAgentManager {
    /// Checks whether the selected Java runtime satisfies MCW Security Agent requirements (`Java 8+`).
    pub fn check_java_compatibility(java_major: u32) -> std::result::Result<(), String> {
        if java_major < 8 {
            return Err(format!(
                "Java {java_major} is not supported by MCW Security Agent (requires Java 8 or newer)."
            ));
        }
        Ok(())
    }

    /// Returns the per-instance runtime status file path (`<instance_dir>/logs/mcw-security-agent-status.json`).
    pub fn status_file_path(instance_dir: &Path) -> PathBuf {
        instance_dir
            .join("logs")
            .join(SECURITY_AGENT_STATUS_FILENAME)
    }

    /// Clears any stale status file from previous runs before launching Minecraft.
    pub fn reset_status_file(instance_dir: &Path) {
        let path = Self::status_file_path(instance_dir);
        let _ = fs::remove_file(path);
    }

    /// Reads the latest structured runtime status reported by `McwSecurityAgent` for an instance.
    pub fn read_runtime_status(instance_dir: &Path) -> Option<SecurityAgentStatusReport> {
        let path = Self::status_file_path(instance_dir);
        let raw = fs::read_to_string(path).ok()?;
        serde_json::from_str::<SecurityAgentStatusReport>(&raw).ok()
    }

    /// Ensures `<runtime_dir>/mcw-security-agent.jar` exists and matches the expected SHA-256.
    pub fn ensure_installed(runtime_dir: &Path) -> Result<PathBuf> {
        fs::create_dir_all(runtime_dir)?;
        let dest = runtime_dir.join(SECURITY_AGENT_FILENAME);

        if dest.is_file() {
            if let Ok(bytes) = fs::read(&dest) {
                if sha256_hex(&bytes) == SECURITY_AGENT_SHA256 {
                    return Ok(dest);
                }
            }
        }

        let actual = sha256_hex(BUNDLED_AGENT_BYTES);
        if actual != SECURITY_AGENT_SHA256 {
            return Err(RuntimeError::Process(format!(
                "Bundled MCW Security Agent SHA-256 mismatch (expected {SECURITY_AGENT_SHA256}, got {actual})"
            )));
        }

        let tmp = runtime_dir.join(format!("{SECURITY_AGENT_FILENAME}.part"));
        fs::write(&tmp, BUNDLED_AGENT_BYTES)?;
        if dest.exists() {
            let _ = fs::remove_file(&dest);
        }
        fs::rename(&tmp, &dest)?;
        Ok(dest)
    }

    /// Removes the extracted `mcw-security-agent.jar` cache cleanly without affecting MCW Core.
    pub fn uninstall(runtime_dir: &Path) -> Result<bool> {
        let dest = runtime_dir.join(SECURITY_AGENT_FILENAME);
        let part = runtime_dir.join(format!("{SECURITY_AGENT_FILENAME}.part"));
        let mut removed = false;
        if dest.exists() {
            fs::remove_file(&dest)?;
            removed = true;
        }
        if part.exists() {
            let _ = fs::remove_file(&part);
        }
        Ok(removed)
    }

    /// Strips any user-supplied `-Dmcw.security.*` or `-javaagent:*mcw-security-agent.jar` flags
    /// so the launcher remains the single source of truth for security mitigations.
    pub fn sanitize_user_jvm_args(args: &[String]) -> Vec<String> {
        let needle = SECURITY_AGENT_FILENAME.to_ascii_lowercase();
        args.iter()
            .filter(|arg| {
                if arg.starts_with("-Dmcw.security.") {
                    return false;
                }
                if arg.starts_with("-javaagent:") && arg.to_ascii_lowercase().contains(&needle) {
                    return false;
                }
                true
            })
            .cloned()
            .collect()
    }

    /// Builds the JVM arguments to attach `mcw-security-agent.jar` with FreeType hinting disabled
    /// and a per-run status verification handshake file.
    pub fn build_jvm_args(
        runtime_dir: &Path,
        instance_dir: &Path,
        java_major: u32,
    ) -> Result<Vec<String>> {
        Self::check_java_compatibility(java_major).map_err(RuntimeError::Process)?;
        let jar_path = Self::ensure_installed(runtime_dir)?;
        let status_file = Self::status_file_path(instance_dir);
        if let Some(parent) = status_file.parent() {
            let _ = fs::create_dir_all(parent);
        }
        Self::reset_status_file(instance_dir);
        let status_posix = status_file.to_string_lossy().replace('\\', "/");

        Ok(vec![
            "-Dmcw.security.freetype_no_hinting=true".to_string(),
            format!("-Dmcw.security.java_major={java_major}"),
            format!("-Dmcw.security.status_file={status_posix}"),
            format!("-javaagent:{}", jar_path.display()),
        ])
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn extracts_repairs_and_uninstalls_bundled_security_agent_jar() {
        let tmp = tempdir().unwrap();
        let jar = SecurityAgentManager::ensure_installed(tmp.path()).unwrap();
        assert!(jar.is_file());
        let bytes = fs::read(&jar).unwrap();
        assert_eq!(sha256_hex(&bytes), SECURITY_AGENT_SHA256);

        // Corrupt the jar on disk and verify self-healing re-extraction.
        fs::write(&jar, b"corrupted-agent").unwrap();
        let repaired = SecurityAgentManager::ensure_installed(tmp.path()).unwrap();
        assert_eq!(repaired, jar);
        assert_eq!(
            sha256_hex(&fs::read(&repaired).unwrap()),
            SECURITY_AGENT_SHA256
        );

        // Independent clean uninstallation.
        assert!(SecurityAgentManager::uninstall(tmp.path()).unwrap());
        assert!(!jar.exists());
        assert!(!SecurityAgentManager::uninstall(tmp.path()).unwrap());
    }

    #[test]
    fn validates_java_compatibility_and_reads_runtime_status() {
        assert!(SecurityAgentManager::check_java_compatibility(7).is_err());
        assert!(SecurityAgentManager::check_java_compatibility(8).is_ok());
        assert!(SecurityAgentManager::check_java_compatibility(21).is_ok());

        let tmp = tempdir().unwrap();
        let runtime_dir = tmp.path().join("runtime");
        let inst_dir = tmp.path().join("instances").join("demo");
        let stale_status = SecurityAgentManager::status_file_path(&inst_dir);
        fs::create_dir_all(stale_status.parent().unwrap()).unwrap();
        fs::write(
            &stale_status,
            r#"{"schema_version":1,"state":"INITIALIZED","patched_methods":0}"#,
        )
        .unwrap();
        assert_eq!(
            SecurityAgentManager::read_runtime_status(&inst_dir)
                .unwrap()
                .state,
            SecurityAgentRuntimeState::Initialized
        );

        // build_jvm_args clears stale status files cleanly.
        let args = SecurityAgentManager::build_jvm_args(&runtime_dir, &inst_dir, 21).unwrap();
        assert!(SecurityAgentManager::read_runtime_status(&inst_dir).is_none());
        assert!(args
            .iter()
            .any(|a| a == "-Dmcw.security.freetype_no_hinting=true"));
        assert!(args.iter().any(|a| a == "-Dmcw.security.java_major=21"));
        assert!(args
            .iter()
            .any(|a| a.starts_with("-Dmcw.security.status_file=")));
        assert!(args
            .iter()
            .any(|a| a.starts_with("-javaagent:") && a.contains(SECURITY_AGENT_FILENAME)));

        fs::write(
            &stale_status,
            r#"{"schema_version":1,"state":"ACTIVE_VERIFIED","patched_methods":4,"detail":"ok"}"#,
        )
        .unwrap();
        let parsed = SecurityAgentManager::read_runtime_status(&inst_dir).unwrap();
        assert_eq!(parsed.state, SecurityAgentRuntimeState::ActiveVerified);
        assert_eq!(parsed.patched_methods, 4);

        let user_args = vec![
            "-XX:+UseG1GC".to_string(),
            "-Dmcw.security.freetype_no_hinting=false".to_string(),
            "-Dmcw.security.status_file=/tmp/spoof.json".to_string(),
            "-javaagent:/tmp/mcw-security-agent.jar".to_string(),
        ];
        let clean = SecurityAgentManager::sanitize_user_jvm_args(&user_args);
        assert_eq!(clean, vec!["-XX:+UseG1GC".to_string()]);
    }
}
