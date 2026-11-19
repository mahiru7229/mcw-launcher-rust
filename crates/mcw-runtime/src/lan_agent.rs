use crate::{Result, RuntimeError};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// Embedded `mcw-lan-agent.jar` (11.2 KB pure-Java instrumentation agent).
/// Built from `tools/mcw_lan_agent/` and verified by SHA-256 on extraction.
const BUNDLED_AGENT_BYTES: &[u8] = include_bytes!("../assets/mcw-lan-agent.jar");

pub const AGENT_FILENAME: &str = "mcw-lan-agent.jar";
pub const AGENT_LOG_FILENAME: &str = "mcw-lan-agent.log";
pub const AGENT_SHA256: &str = "c682cd51fbfc9b5e3ed34520eb38a667212c183a68e37ad17694f14f4eace4dc";

pub const AUTH_PRIVATE_OFFLINE: &str = "private_offline";

/// Default named runtime targets for `MinecraftServer#setUsesAuthentication(boolean)`
/// and `MinecraftServer#setOnlineMode(boolean)`.
const DEFAULT_TARGETS: &[&str] = &[
    "net/minecraft/server/MinecraftServer#setUsesAuthentication",
    "net/minecraft/server/MinecraftServer#setOnlineMode",
];

pub struct LanAgentManager;

impl LanAgentManager {
    /// Returns true if the given instance LAN settings request the MCW LAN Agent.
    pub fn is_enabled(offline_multiplayer_enabled: bool, lan_auth_mode: &str) -> bool {
        if offline_multiplayer_enabled {
            return true;
        }
        let mode = lan_auth_mode.trim().to_ascii_lowercase();
        mode == AUTH_PRIVATE_OFFLINE || mode == "friends"
    }

    /// Ensures `<runtime_dir>/mcw-lan-agent.jar` exists and matches the expected SHA-256.
    pub fn ensure_installed(runtime_dir: &Path) -> Result<PathBuf> {
        fs::create_dir_all(runtime_dir)?;
        let dest = runtime_dir.join(AGENT_FILENAME);

        if dest.is_file() {
            if let Ok(bytes) = fs::read(&dest) {
                if sha256_hex(&bytes) == AGENT_SHA256 {
                    return Ok(dest);
                }
            }
        }

        let actual = sha256_hex(BUNDLED_AGENT_BYTES);
        if actual != AGENT_SHA256 {
            return Err(RuntimeError::Process(format!(
                "Bundled MCW LAN Agent SHA-256 mismatch (expected {AGENT_SHA256}, got {actual})"
            )));
        }

        let tmp = runtime_dir.join(format!("{AGENT_FILENAME}.part"));
        fs::write(&tmp, BUNDLED_AGENT_BYTES)?;
        if dest.exists() {
            let _ = fs::remove_file(&dest);
        }
        fs::rename(&tmp, &dest)?;
        Ok(dest)
    }

    /// Returns the dedicated log file path for an instance's LAN Agent diagnostics.
    pub fn log_path(instance_dir: &Path) -> PathBuf {
        instance_dir.join("logs").join(AGENT_LOG_FILENAME)
    }

    /// Strips any user-supplied `-Dmcw.lan.*` or `-javaagent:*mcw-lan-agent.jar` flags
    /// so the launcher remains the single source of truth for agent arguments.
    pub fn sanitize_user_jvm_args(args: &[String]) -> Vec<String> {
        let needle = AGENT_FILENAME.to_ascii_lowercase();
        args.iter()
            .filter(|arg| {
                if arg.starts_with("-Dmcw.lan.") {
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

    /// Builds the JVM arguments to attach `mcw-lan-agent.jar` to a Minecraft launch.
    pub fn build_jvm_args(
        runtime_dir: &Path,
        instance_dir: &Path,
        loader_name: &str,
        extra_targets: &[String],
    ) -> Result<Vec<String>> {
        let jar_path = Self::ensure_installed(runtime_dir)?;
        let log_file = Self::log_path(instance_dir);
        if let Some(parent) = log_file.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let mut targets: Vec<String> = DEFAULT_TARGETS.iter().map(|s| s.to_string()).collect();
        for t in extra_targets {
            let trimmed = t.trim();
            if !trimmed.is_empty() && !targets.iter().any(|x| x == trimmed) {
                targets.push(trimmed.to_string());
            }
        }

        let loader = if loader_name.trim().is_empty() {
            "vanilla"
        } else {
            loader_name.trim()
        };
        let log_posix = log_file.to_string_lossy().replace('\\', "/");

        Ok(vec![
            "-Dmcw.lan.offline=true".to_string(),
            format!("-Dmcw.lan.loader={loader}"),
            format!("-Dmcw.lan.targets={}", targets.join(";")),
            format!("-Dmcw.lan.log={log_posix}"),
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
    fn extracts_and_verifies_bundled_lan_agent_jar() {
        let tmp = tempdir().unwrap();
        let jar = LanAgentManager::ensure_installed(tmp.path()).unwrap();
        assert!(jar.is_file());
        let bytes = fs::read(&jar).unwrap();
        assert_eq!(sha256_hex(&bytes), AGENT_SHA256);

        // Idempotent second call
        let jar2 = LanAgentManager::ensure_installed(tmp.path()).unwrap();
        assert_eq!(jar, jar2);
    }

    #[test]
    fn builds_jvm_args_and_sanitizes_user_overrides() {
        let tmp = tempdir().unwrap();
        let runtime_dir = tmp.path().join("runtime");
        let inst_dir = tmp.path().join("instances").join("demo");

        let args = LanAgentManager::build_jvm_args(&runtime_dir, &inst_dir, "fabric", &[]).unwrap();
        assert!(args.iter().any(|a| a == "-Dmcw.lan.offline=true"));
        assert!(args.iter().any(|a| a == "-Dmcw.lan.loader=fabric"));
        assert!(args
            .iter()
            .any(|a| a.starts_with("-javaagent:") && a.contains(AGENT_FILENAME)));

        let user_args = vec![
            "-XX:+UseG1GC".to_string(),
            "-Dmcw.lan.offline=false".to_string(),
            "-javaagent:/tmp/mcw-lan-agent.jar".to_string(),
        ];
        let clean = LanAgentManager::sanitize_user_jvm_args(&user_args);
        assert_eq!(clean, vec!["-XX:+UseG1GC".to_string()]);
    }
}
