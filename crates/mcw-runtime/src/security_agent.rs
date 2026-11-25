use crate::{Result, RuntimeError};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// Embedded `mcw-security-agent.jar` (pure-Java instrumentation agent).
/// Built from `tools/mcw_security_agent/` and verified by SHA-256 on extraction.
const BUNDLED_AGENT_BYTES: &[u8] = include_bytes!("../assets/mcw-security-agent.jar");

pub const SECURITY_AGENT_FILENAME: &str = "mcw-security-agent.jar";
pub const SECURITY_AGENT_SHA256: &str =
    "83413f43ebde4d6e84bb0c5283b0fdf6262ce6e75180c5b3e1ab5ee9db258a26";

pub struct SecurityAgentManager;

impl SecurityAgentManager {
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

    /// Builds the JVM arguments to attach `mcw-security-agent.jar` with FreeType hinting disabled.
    pub fn build_jvm_args(runtime_dir: &Path) -> Result<Vec<String>> {
        let jar_path = Self::ensure_installed(runtime_dir)?;
        Ok(vec![
            "-Dmcw.security.freetype_no_hinting=true".to_string(),
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
    fn extracts_and_repairs_bundled_security_agent_jar() {
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
    }

    #[test]
    fn builds_jvm_args_and_sanitizes_user_overrides() {
        let tmp = tempdir().unwrap();
        let args = SecurityAgentManager::build_jvm_args(tmp.path()).unwrap();
        assert!(args
            .iter()
            .any(|a| a == "-Dmcw.security.freetype_no_hinting=true"));
        assert!(args
            .iter()
            .any(|a| a.starts_with("-javaagent:") && a.contains(SECURITY_AGENT_FILENAME)));

        let user_args = vec![
            "-XX:+UseG1GC".to_string(),
            "-Dmcw.security.freetype_no_hinting=false".to_string(),
            "-javaagent:/tmp/mcw-security-agent.jar".to_string(),
        ];
        let clean = SecurityAgentManager::sanitize_user_jvm_args(&user_args);
        assert_eq!(clean, vec!["-XX:+UseG1GC".to_string()]);
    }
}
