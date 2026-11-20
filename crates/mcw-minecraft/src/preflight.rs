use crate::rules::LibraryRules;
use crate::{MinecraftError, Result};
use mcw_models::instance::Instance;
use mcw_models::minecraft::VersionMetadata;
use mcw_models::FailurePolicy;
use mcw_mods::{ModCompatibilityAnalyzer, ModIssueSeverity, ModManager};
use mcw_runtime::sanitize_log_text;
use mcw_storage::LauncherPaths;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgePreflightIssue {
    pub severity: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct ForgePreflightReport {
    pub loader: String,
    pub issues: Vec<ForgePreflightIssue>,
}

impl ForgePreflightReport {
    pub fn error_count(&self) -> usize {
        self.issues.iter().filter(|i| i.severity == "error").count()
    }

    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == "warning")
            .count()
    }

    pub fn can_launch(&self) -> bool {
        self.error_count() == 0
    }

    pub fn format_summary(&self) -> String {
        format!(
            "{} error(s), {} warning(s)",
            self.error_count(),
            self.warning_count()
        )
    }
}

/// Port of Python `ForgePreflightManager` (`src/core/modloader/forge/forge_preflight_manager.py`).
pub struct ForgePreflightManager;

impl ForgePreflightManager {
    pub fn scan(
        paths: &LauncherPaths,
        instance: &Instance,
        metadata: &VersionMetadata,
        verify_files: bool,
    ) -> ForgePreflightReport {
        let loader = instance.mod_loader.name.trim().to_ascii_lowercase();
        if loader != "forge" && loader != "neoforge" {
            return ForgePreflightReport {
                loader,
                issues: Vec::new(),
            };
        }

        let mut issues = Vec::new();
        if metadata.main_class.trim().is_empty() {
            issues.push(ForgePreflightIssue {
                severity: "error".to_string(),
                code: format!("{loader}-installation"),
                message: format!("{loader} launch metadata is missing mainClass."),
            });
        }

        if verify_files {
            let libs_dir = paths.libraries_dir();
            for lib in &metadata.libraries {
                if !LibraryRules::is_allowed(&lib.rules) || lib.clientreq == Some(false) {
                    continue;
                }
                if let Some(ref rel) = lib
                    .downloads
                    .as_ref()
                    .and_then(|d| d.artifact.as_ref())
                    .and_then(|a| a.path.as_ref())
                {
                    if let Ok(dest) = LauncherPaths::safe_join_relative(&libs_dir, rel) {
                        if !dest.is_file() {
                            issues.push(ForgePreflightIssue {
                                severity: "error".to_string(),
                                code: format!("{loader}-installation"),
                                message: format!("Missing required {loader} library: {rel}"),
                            });
                        }
                    }
                }
            }
        }

        let mods_dir = paths.minecraft_dir(&instance.instance_id).join("mods");
        if mods_dir.is_dir() {
            let mods = ModManager::new(&mods_dir).list();
            let compat = ModCompatibilityAnalyzer::analyze(&loader, &mods);
            for issue in compat.issues {
                let sev = match issue.severity {
                    ModIssueSeverity::Error => "error",
                    ModIssueSeverity::Warning => "warning",
                    ModIssueSeverity::Info => "info",
                };
                issues.push(ForgePreflightIssue {
                    severity: sev.to_string(),
                    code: issue.code,
                    message: issue.message,
                });
            }
        }

        issues.sort_by(|a, b| {
            severity_rank(&a.severity)
                .cmp(&severity_rank(&b.severity))
                .then_with(|| {
                    a.message
                        .to_ascii_lowercase()
                        .cmp(&b.message.to_ascii_lowercase())
                })
        });

        ForgePreflightReport { loader, issues }
    }

    /// Evaluates preflight issues under the configured `FailurePolicy`, returning warning strings
    /// or blocking launch on `FailurePolicy::Block`.
    pub fn enforce(report: &ForgePreflightReport, policy: FailurePolicy) -> Result<Vec<String>> {
        if report.issues.is_empty() || policy == FailurePolicy::Allow {
            return Ok(Vec::new());
        }
        let errors: Vec<&ForgePreflightIssue> = report
            .issues
            .iter()
            .filter(|i| i.severity == "error")
            .collect();
        if !errors.is_empty() && policy.is_blocking() {
            let title = if report.loader == "neoforge" {
                "NeoForge"
            } else {
                "Forge"
            };
            let details: String = errors
                .iter()
                .map(|e| format!("- [{}] {}", e.code, e.message))
                .collect::<Vec<_>>()
                .join("\n");
            return Err(MinecraftError::InvalidMetadata(format!(
                "{title} pre-launch check failed ({}):\n{details}",
                report.format_summary()
            )));
        }

        Ok(report
            .issues
            .iter()
            .map(|i| format!("[{}] {}: {}", i.severity.to_uppercase(), i.code, i.message))
            .collect())
    }
}

fn severity_rank(s: &str) -> u8 {
    match s {
        "error" => 0,
        "warning" => 1,
        "info" => 2,
        _ => 3,
    }
}

/// Port of Python `ForgeDiagnosticsManager` & `QuiltDiagnosticsManager`
/// (Schema `2.1` sanitized mod-loader diagnostic package `.zip`).
pub struct LoaderDiagnosticsExporter;

impl LoaderDiagnosticsExporter {
    const SCHEMA_VERSION: &'static str = "2.1";
    const MAX_LOG_BYTES: usize = 2 * 1024 * 1024;

    pub fn export(
        paths: &LauncherPaths,
        instance: &Instance,
        metadata: Option<&VersionMetadata>,
        destination: &Path,
        launcher_version: &str,
    ) -> Result<PathBuf> {
        let loader = instance.mod_loader.name.trim().to_ascii_lowercase();
        if !matches!(loader.as_str(), "forge" | "neoforge" | "quilt") {
            return Err(MinecraftError::InvalidMetadata(
                "Mod-loader diagnostics export is available for Forge, NeoForge, and Quilt instances."
                    .to_string(),
            ));
        }

        let target = if destination
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        {
            destination.to_path_buf()
        } else {
            destination.with_extension("zip")
        };
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }

        let temp = target.with_extension("zip.part");
        let _ = fs::remove_file(&temp);

        let report = metadata.map(|m| ForgePreflightManager::scan(paths, instance, m, false));
        let summary = Self::build_summary(
            instance,
            &loader,
            metadata,
            report.as_ref(),
            launcher_version,
        );

        let write_result = (|| -> Result<()> {
            let file = File::create(&temp)?;
            let mut zw = ZipWriter::new(file);
            let opts = SimpleFileOptions::default();

            Self::write_sanitized_text(&mut zw, "summary.txt", &summary, opts)?;

            let inst_dir = paths.instance_dir(&instance.instance_id);
            for (src_name, zip_name) in [
                ("instance.json", "instance/instance.json"),
                ("settings.json", "instance/settings.json"),
            ] {
                let p = inst_dir.join(src_name);
                if p.is_file() {
                    if let Ok(raw) = fs::read_to_string(&p) {
                        Self::write_sanitized_text(&mut zw, zip_name, &raw, opts)?;
                    }
                }
            }

            if let Some(meta) = metadata {
                if let Ok(json) = serde_json::to_string_pretty(meta) {
                    Self::write_sanitized_text(
                        &mut zw,
                        &format!("{loader}/profile.json"),
                        &json,
                        opts,
                    )?;
                }
            }

            let mc_dir = paths.minecraft_dir(&instance.instance_id);
            let mods = ModManager::new(&mc_dir.join("mods")).list();
            if let Ok(mods_json) = serde_json::to_string_pretty(&mods) {
                Self::write_sanitized_text(&mut zw, "mods/inventory.json", &mods_json, opts)?;
            }

            let latest_log = mc_dir.join("logs").join("latest.log");
            if latest_log.is_file() {
                Self::add_log_tail(&mut zw, &latest_log, "minecraft/latest.log", opts)?;
            }
            let lan_log = mcw_runtime::LanAgentManager::log_path(&inst_dir);
            if lan_log.is_file() {
                Self::add_log_tail(&mut zw, &lan_log, "lan/mcw-lan-agent.log", opts)?;
            }

            zw.finish()?;
            Ok(())
        })();

        if let Err(err) = write_result {
            let _ = fs::remove_file(&temp);
            return Err(err);
        }

        if target.exists() {
            let _ = fs::remove_file(&target);
        }
        fs::rename(&temp, &target)?;
        Ok(target)
    }

    fn build_summary(
        instance: &Instance,
        loader: &str,
        metadata: Option<&VersionMetadata>,
        report: Option<&ForgePreflightReport>,
        launcher_version: &str,
    ) -> String {
        let title = match loader {
            "neoforge" => "NeoForge",
            "quilt" => "Quilt",
            _ => "Forge",
        };
        let heading = format!("MCW Launcher {title} Diagnostic Package");
        let java_major = metadata
            .and_then(|m| m.java_version.as_ref())
            .map(|j| j.major_version)
            .unwrap_or(8);

        let mut lines = vec![
            heading.clone(),
            "=".repeat(heading.len()),
            format!("schema_version: {}", Self::SCHEMA_VERSION),
            format!("launcher_version: {launcher_version}"),
            format!("instance_name: {}", instance.name),
            format!("instance_id: {}", instance.instance_id),
            format!("minecraft_version: {}", instance.version_id),
            format!("loader: {loader}"),
            format!("loader_version: {}", instance.mod_loader.version),
            format!("required_java_major: {java_major}"),
            String::new(),
        ];

        if let Some(meta) = metadata {
            lines.push(format!("profile_id: {}", meta.id));
            lines.push(format!("main_class: {}", meta.main_class));
            lines.push(format!("library_count: {}", meta.libraries.len()));
            lines.push(String::new());
        }

        if let Some(rep) = report {
            lines.push("Pre-launch check".to_string());
            lines.push("----------------".to_string());
            lines.push(rep.format_summary());
            for issue in &rep.issues {
                lines.push(format!(
                    "[{}] {}: {}",
                    issue.severity.to_uppercase(),
                    issue.code,
                    issue.message
                ));
            }
            lines.push(String::new());
        }

        lines.push("Privacy".to_string());
        lines.push("-------".to_string());
        lines.push(
            "This package excludes account databases, tokens, passwords, worlds, saves, and mod JAR contents."
                .to_string(),
        );
        lines.join("\n") + "\n"
    }

    fn write_sanitized_text(
        zw: &mut ZipWriter<File>,
        name: &str,
        raw: &str,
        opts: SimpleFileOptions,
    ) -> Result<()> {
        let safe = sanitize_log_text(raw);
        zw.start_file(name, opts)?;
        zw.write_all(safe.as_bytes())?;
        Ok(())
    }

    fn add_log_tail(
        zw: &mut ZipWriter<File>,
        path: &Path,
        zip_name: &str,
        opts: SimpleFileOptions,
    ) -> Result<()> {
        if let Ok(bytes) = fs::read(path) {
            let start = bytes.len().saturating_sub(Self::MAX_LOG_BYTES);
            let txt = String::from_utf8_lossy(&bytes[start..]);
            Self::write_sanitized_text(zw, zip_name, &txt, opts)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcw_models::instance::ModLoader;
    use tempfile::tempdir;
    use zip::ZipArchive;

    #[test]
    fn scans_and_enforces_forge_preflight_policies() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();

        let mut inst = Instance::new("forge-demo", "Forge Demo", "1.20.1");
        inst.mod_loader = ModLoader {
            name: "forge".to_string(),
            version: "47.3.0".to_string(),
        };
        let meta = VersionMetadata {
            id: "1.20.1-forge".to_string(),
            version_type: "release".to_string(),
            main_class: String::new(),
            inherits_from: Some("1.20.1".to_string()),
            asset_index: None,
            assets: None,
            downloads: None,
            libraries: Vec::new(),
            java_version: None,
            arguments: None,
            minecraft_arguments: None,
        };

        let report = ForgePreflightManager::scan(&paths, &inst, &meta, false);
        assert!(!report.can_launch());
        assert_eq!(report.error_count(), 1);
        assert!(ForgePreflightManager::enforce(&report, FailurePolicy::Block).is_err());
        let warnings = ForgePreflightManager::enforce(&report, FailurePolicy::Warn).unwrap();
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn exports_sanitized_loader_diagnostics_zip() {
        let dir = tempdir().unwrap();
        let paths = LauncherPaths::portable(dir.path());
        paths.initialize().unwrap();

        let store = mcw_storage::InstanceStore::new(&paths);
        let (inst, _) = store
            .create_instance(
                "quilt-diag",
                "1.21.4",
                ModLoader {
                    name: "quilt".to_string(),
                    version: "0.27.0".to_string(),
                },
            )
            .unwrap();
        let dest = dir.path().join("quilt-diag.zip");
        let exported =
            LoaderDiagnosticsExporter::export(&paths, &inst, None, &dest, "0.10.0-beta.1").unwrap();
        assert!(exported.is_file());

        let zip = ZipArchive::new(File::open(&exported).unwrap()).unwrap();
        assert!(zip.file_names().any(|n| n == "summary.txt"));
        assert!(zip.file_names().any(|n| n == "mods/inventory.json"));
    }
}
