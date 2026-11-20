use crate::{InstanceRunLock, LauncherPaths};
use mcw_models::instance::{Instance, InstanceSettings};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InstanceHealthState {
    Healthy = 0,
    NeedsAttention = 1,
    MissingFiles = 2,
    MissingJava = 3,
    Incomplete = 4,
    Corrupted = 5,
}

impl InstanceHealthState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Healthy => "Healthy",
            Self::NeedsAttention => "Needs Attention",
            Self::MissingFiles => "Missing Files",
            Self::MissingJava => "Missing Java",
            Self::Incomplete => "Incomplete",
            Self::Corrupted => "Corrupted",
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstanceHealthIssue {
    pub code: String,
    pub state: InstanceHealthState,
    pub severity: HealthSeverity,
    pub message: String,
    pub fixable: bool,
}

#[derive(Debug, Clone)]
pub struct InstanceHealthReport {
    pub instance_id: String,
    pub name: String,
    pub state: InstanceHealthState,
    pub issues: Vec<InstanceHealthIssue>,
    pub client_jar_present: bool,
    pub version_json_present: bool,
}

pub struct InstanceHealthManager;

impl InstanceHealthManager {
    pub fn scan(paths: &LauncherPaths, instance: &Instance) -> InstanceHealthReport {
        let mut issues = Vec::new();
        let inst_dir = paths.instance_dir(&instance.instance_id);
        let meta_path = paths.instance_json(&instance.instance_id);
        let settings_path = paths.instance_settings_json(&instance.instance_id);

        let version_json_present = paths.version_json(&instance.version_id).is_file();
        let client_jar_present = paths.client_jar(&instance.version_id).is_file();

        if !inst_dir.is_dir() {
            issues.push(InstanceHealthIssue {
                code: "instance_directory_missing".to_string(),
                state: InstanceHealthState::Incomplete,
                severity: HealthSeverity::Error,
                message: "The instance directory is missing.".to_string(),
                fixable: false,
            });
            return Self::finalize(instance, issues, client_jar_present, version_json_present);
        }

        match fs::read_to_string(&meta_path) {
            Err(_) => issues.push(InstanceHealthIssue {
                code: "metadata_missing".to_string(),
                state: InstanceHealthState::Incomplete,
                severity: HealthSeverity::Error,
                message: "instance.json is missing.".to_string(),
                fixable: false,
            }),
            Ok(raw) => match serde_json::from_str::<Instance>(&raw) {
                Err(_) => issues.push(InstanceHealthIssue {
                    code: "metadata_invalid".to_string(),
                    state: InstanceHealthState::Corrupted,
                    severity: HealthSeverity::Error,
                    message: "instance.json contains invalid JSON.".to_string(),
                    fixable: false,
                }),
                Ok(parsed) => {
                    if parsed.version_id.trim().is_empty() {
                        issues.push(InstanceHealthIssue {
                            code: "metadata_version_missing".to_string(),
                            state: InstanceHealthState::Corrupted,
                            severity: HealthSeverity::Error,
                            message: "instance.json does not specify a Minecraft version."
                                .to_string(),
                            fixable: false,
                        });
                    }
                }
            },
        }

        match fs::read_to_string(&settings_path) {
            Err(_) => issues.push(InstanceHealthIssue {
                code: "settings_missing".to_string(),
                state: InstanceHealthState::NeedsAttention,
                severity: HealthSeverity::Warning,
                message: "settings.json is missing and will use defaults.".to_string(),
                fixable: true,
            }),
            Ok(raw) => match serde_json::from_str::<InstanceSettings>(&raw) {
                Err(_) => issues.push(InstanceHealthIssue {
                    code: "settings_invalid".to_string(),
                    state: InstanceHealthState::Corrupted,
                    severity: HealthSeverity::Error,
                    message: "settings.json is not valid JSON.".to_string(),
                    fixable: true,
                }),
                Ok(settings) => {
                    if let Some(ref java) = settings.java_path {
                        if !java.as_os_str().is_empty() && !java.is_file() {
                            issues.push(InstanceHealthIssue {
                                code: "configured_java_missing".to_string(),
                                state: InstanceHealthState::MissingJava,
                                severity: HealthSeverity::Error,
                                message: format!(
                                    "Configured Java executable does not exist: {}",
                                    java.display()
                                ),
                                fixable: true,
                            });
                        }
                    }
                }
            },
        }

        let loader = instance.mod_loader.name.trim().to_ascii_lowercase();
        let supported_loaders = ["vanilla", "fabric", "quilt", "forge", "neoforge"];
        if !supported_loaders.contains(&loader.as_str()) {
            issues.push(InstanceHealthIssue {
                code: "loader_unsupported".to_string(),
                state: InstanceHealthState::Corrupted,
                severity: HealthSeverity::Error,
                message: format!("Unsupported mod loader '{}'.", instance.mod_loader.name),
                fixable: true,
            });
        } else if loader != "vanilla"
            && (instance.mod_loader.version.trim().is_empty()
                || instance.mod_loader.version.trim() == "-1")
        {
            issues.push(InstanceHealthIssue {
                code: "loader_version_missing".to_string(),
                state: InstanceHealthState::Incomplete,
                severity: HealthSeverity::Warning,
                message: format!(
                    "Loader '{}' has no pinned version yet; launch will resolve latest.",
                    instance.mod_loader.name
                ),
                fixable: true,
            });
        }

        let icon = instance.icon.trim();
        if !icon.is_empty()
            && icon != "grass_block"
            && (icon.contains('/') || icon.contains('\\') || icon.contains('.'))
        {
            let icon_path = if Path::new(icon).is_absolute() {
                Path::new(icon).to_path_buf()
            } else {
                inst_dir.join(icon)
            };
            if !icon_path.is_file() {
                issues.push(InstanceHealthIssue {
                    code: "icon_missing".to_string(),
                    state: InstanceHealthState::NeedsAttention,
                    severity: HealthSeverity::Warning,
                    message: format!("Custom instance icon missing at {}", icon_path.display()),
                    fixable: true,
                });
            }
        }

        if !version_json_present || !client_jar_present {
            issues.push(InstanceHealthIssue {
                code: "minecraft_core_missing".to_string(),
                state: InstanceHealthState::MissingFiles,
                severity: HealthSeverity::Warning,
                message: format!(
                    "Cached Minecraft {} core files are incomplete; use Repair Files or Launch to download.",
                    instance.version_id
                ),
                fixable: true,
            });
        }

        if instance.last_launch_crashed || instance.last_launch_state == "crashed" {
            issues.push(InstanceHealthIssue {
                code: "last_launch_crashed".to_string(),
                state: InstanceHealthState::NeedsAttention,
                severity: HealthSeverity::Warning,
                message: "The previous Minecraft session ended in a crash.".to_string(),
                fixable: false,
            });
        }

        Self::finalize(instance, issues, client_jar_present, version_json_present)
    }

    fn finalize(
        instance: &Instance,
        issues: Vec<InstanceHealthIssue>,
        client_jar_present: bool,
        version_json_present: bool,
    ) -> InstanceHealthReport {
        let worst = issues
            .iter()
            .map(|i| i.state)
            .max()
            .unwrap_or(InstanceHealthState::Healthy);
        InstanceHealthReport {
            instance_id: instance.instance_id.clone(),
            name: instance.name.clone(),
            state: worst,
            issues,
            client_jar_present,
            version_json_present,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct StartupRecoveryReport {
    pub stale_locks: Vec<String>,
    pub orphan_temp_files: usize,
}

impl StartupRecoveryReport {
    pub fn recovered_count(&self) -> usize {
        self.stale_locks.len() + self.orphan_temp_files
    }
}

pub struct StartupRecoveryService;

impl StartupRecoveryService {
    /// Clean up stale instance run locks and interrupted temp/staging artifacts at launcher startup.
    pub fn reconcile(paths: &LauncherPaths) -> StartupRecoveryReport {
        let stale_locks = InstanceRunLock::reconcile_stale_locks(paths);
        let mut orphan_temp_files = 0usize;

        for dir in [
            paths.cache_root.join("tmp"),
            paths.instances_root.join(".runtime").join("staging"),
        ] {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    let removed = if p.is_dir() {
                        fs::remove_dir_all(&p).is_ok()
                    } else {
                        fs::remove_file(&p).is_ok()
                    };
                    if removed {
                        orphan_temp_files += 1;
                    }
                }
            }
        }

        StartupRecoveryReport {
            stale_locks,
            orphan_temp_files,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{atomic_write_json, InstanceStore};
    use mcw_models::instance::ModLoader;
    use tempfile::tempdir;

    #[test]
    fn health_scan_detects_missing_java_and_missing_core_files() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();

        let store = InstanceStore::new(&paths);
        let (inst, mut settings) = store
            .create_instance("TestHealth", "1.21.4", ModLoader::vanilla())
            .unwrap();

        // Provide core version files -> healthy except custom java check
        let vj = paths.version_json("1.21.4");
        let cj = paths.client_jar("1.21.4");
        fs::create_dir_all(vj.parent().unwrap()).unwrap();
        fs::write(&vj, "{}").unwrap();
        fs::write(&cj, "jar").unwrap();

        let ok_rep = InstanceHealthManager::scan(&paths, &inst);
        assert_eq!(ok_rep.state, InstanceHealthState::Healthy);

        settings.java_path = Some(dir.path().join("nonexistent-java.exe"));
        atomic_write_json(&paths.instance_settings_json(&inst.instance_id), &settings).unwrap();

        let bad_java = InstanceHealthManager::scan(&paths, &inst);
        assert_eq!(bad_java.state, InstanceHealthState::MissingJava);
        assert!(bad_java
            .issues
            .iter()
            .any(|i| i.code == "configured_java_missing"));
    }

    #[test]
    fn startup_recovery_cleans_stale_locks_and_staging_orphans() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();

        let stale_lock = InstanceRunLock::lock_path_for(&paths, "crashed_inst");
        fs::write(
            &stale_lock,
            r#"{"schema_version":1,"token":"t","instance_id":"crashed_inst","state":"running","launcher_pid":999999,"minecraft_pid":999999,"updated_at_epoch":1}"#,
        )
        .unwrap();

        let tmp_file = paths.cache_root.join("tmp").join("aborted.part");
        fs::create_dir_all(tmp_file.parent().unwrap()).unwrap();
        fs::write(&tmp_file, "partial").unwrap();

        let report = StartupRecoveryService::reconcile(&paths);
        assert_eq!(report.stale_locks, vec!["crashed_inst".to_string()]);
        assert_eq!(report.orphan_temp_files, 1);
        assert!(!stale_lock.exists());
        assert!(!tmp_file.exists());
    }
}
