use crate::{atomic_write_json, LauncherPaths, Result, StorageError};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const ACQUIRE_ATTEMPTS: usize = 5;
const PREPARING_TIMEOUT_SECS: u64 = 600;
const MALFORMED_GRACE_SECS: u64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockState {
    Preparing,
    Running,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningInstanceInfo {
    pub schema_version: u32,
    pub token: String,
    pub instance_id: String,
    pub state: LockState,
    pub launcher_pid: u32,
    pub minecraft_pid: Option<u32>,
    pub updated_at_epoch: u64,
}

pub struct InstanceRunLock {
    pub instance_id: String,
    lock_path: PathBuf,
    token: String,
    released: bool,
}

impl InstanceRunLock {
    pub fn lock_path_for(paths: &LauncherPaths, instance_id: &str) -> PathBuf {
        paths
            .instance_locks_root
            .join(format!("{instance_id}.lock"))
    }

    pub fn acquire(paths: &LauncherPaths, instance_id: &str) -> Result<Self> {
        fs::create_dir_all(&paths.instance_locks_root).map_err(|source| StorageError::Io {
            path: paths.instance_locks_root.display().to_string(),
            source,
        })?;

        let lock_path = Self::lock_path_for(paths, instance_id);
        let token = Uuid::new_v4().simple().to_string();
        let payload = RunningInstanceInfo {
            schema_version: 1,
            token: token.clone(),
            instance_id: instance_id.to_string(),
            state: LockState::Preparing,
            launcher_pid: std::process::id(),
            minecraft_pid: None,
            updated_at_epoch: now_epoch_secs(),
        };
        let bytes = serde_json::to_vec_pretty(&payload).map_err(|source| StorageError::Json {
            path: lock_path.display().to_string(),
            source,
        })?;

        for _ in 0..ACQUIRE_ATTEMPTS {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(mut file) => {
                    file.write_all(&bytes).map_err(|source| StorageError::Io {
                        path: lock_path.display().to_string(),
                        source,
                    })?;
                    return Ok(Self {
                        instance_id: instance_id.to_string(),
                        lock_path,
                        token,
                        released: false,
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    if Self::is_lock_file_active(&lock_path) {
                        return Err(StorageError::InstanceAlreadyRunning(
                            instance_id.to_string(),
                        ));
                    }
                    // Stale lock — remove and retry.
                    let _ = fs::remove_file(&lock_path);
                }
                Err(source) => {
                    return Err(StorageError::Io {
                        path: lock_path.display().to_string(),
                        source,
                    });
                }
            }
        }

        Err(StorageError::InstanceAlreadyRunning(
            instance_id.to_string(),
        ))
    }

    pub fn mark_running(&self, minecraft_pid: u32) -> Result<()> {
        let payload = RunningInstanceInfo {
            schema_version: 1,
            token: self.token.clone(),
            instance_id: self.instance_id.clone(),
            state: LockState::Running,
            launcher_pid: std::process::id(),
            minecraft_pid: Some(minecraft_pid),
            updated_at_epoch: now_epoch_secs(),
        };
        atomic_write_json(&self.lock_path, &payload)
    }

    pub fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        if let Some(info) = Self::read_info(&self.lock_path) {
            if info.token == self.token {
                let _ = fs::remove_file(&self.lock_path);
            }
        }
    }

    pub fn is_running(paths: &LauncherPaths, instance_id: &str) -> bool {
        let lock_path = Self::lock_path_for(paths, instance_id);
        Self::is_lock_file_active(&lock_path)
    }

    pub fn inspect(paths: &LauncherPaths, instance_id: &str) -> Option<RunningInstanceInfo> {
        let lock_path = Self::lock_path_for(paths, instance_id);
        if !Self::is_lock_file_active(&lock_path) {
            return None;
        }
        Self::read_info(&lock_path)
    }

    fn read_info(lock_path: &Path) -> Option<RunningInstanceInfo> {
        let raw = fs::read_to_string(lock_path).ok()?;
        serde_json::from_str(&raw).ok()
    }

    fn is_lock_file_active(lock_path: &Path) -> bool {
        let Ok(meta) = fs::metadata(lock_path) else {
            return false;
        };
        let age_secs = meta
            .modified()
            .ok()
            .and_then(|m| SystemTime::now().duration_since(m).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let Some(info) = Self::read_info(lock_path) else {
            // Grace period for a lock file currently being written.
            return age_secs < MALFORMED_GRACE_SECS;
        };

        match info.state {
            LockState::Preparing => {
                if age_secs > PREPARING_TIMEOUT_SECS {
                    return false;
                }
                is_pid_alive(info.launcher_pid)
            }
            LockState::Running => {
                if let Some(mc_pid) = info.minecraft_pid {
                    is_pid_alive(mc_pid)
                } else {
                    is_pid_alive(info.launcher_pid)
                }
            }
        }
    }
}

impl Drop for InstanceRunLock {
    fn drop(&mut self) {
        self.release();
    }
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    if pid == std::process::id() {
        return true;
    }
    #[cfg(windows)]
    {
        // Fast check without spawning PowerShell: use OpenProcess? Without windows-rs in storage,
        // check `tasklist /FI "PID eq <pid>" /NH` or treat current process as alive.
        // We keep storage lightweight; ProcessSupervisor in mcw-runtime will use Win32 APIs directly.
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .map(|out| {
                let text = String::from_utf8_lossy(&out.stdout);
                text.contains(&format!("\"{pid}\""))
            })
            .unwrap_or(true)
    }
    #[cfg(not(windows))]
    {
        Path::new(&format!("/proc/{pid}")).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn run_lock_blocks_duplicate_and_releases_on_drop() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();

        {
            let lock = InstanceRunLock::acquire(&paths, "survival").unwrap();
            assert!(InstanceRunLock::is_running(&paths, "survival"));
            assert!(matches!(
                InstanceRunLock::acquire(&paths, "survival"),
                Err(StorageError::InstanceAlreadyRunning(_))
            ));
            lock.mark_running(std::process::id()).unwrap();
            let info = InstanceRunLock::inspect(&paths, "survival").unwrap();
            assert_eq!(info.state, LockState::Running);
        }

        // Dropped -> released
        assert!(!InstanceRunLock::is_running(&paths, "survival"));
        assert!(InstanceRunLock::acquire(&paths, "survival").is_ok());
    }
}
