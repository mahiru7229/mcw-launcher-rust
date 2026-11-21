/// Direct port of MCW Launcher Python's `LaunchErrorPresenter` (`src/gui/presenters/launch_error_presenter.py`),
/// `GameRuntimeManager.latest_crash_report`, and `DiagnosticsManager` (`src/core/diagnostics/diagnostics_manager.py`).
use crate::java::JavaInstallation;
use crate::mclogs::sanitize_log_text;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchErrorView {
    pub title: String,
    pub summary: String,
    pub status: String,
    pub technical_message: String,
}

impl LaunchErrorView {
    pub fn banner_text(&self) -> String {
        format!(
            "{}: {} ({})",
            self.title, self.summary, self.technical_message
        )
    }
}

pub struct LaunchErrorPresenter;

impl LaunchErrorPresenter {
    pub fn present(raw_error: &str, unsupported_class_version: bool) -> LaunchErrorView {
        let technical = raw_error.trim();
        let normalized = technical.to_ascii_lowercase();

        if unsupported_class_version || normalized.contains("unsupportedclassversionerror") {
            return Self::build(
                "Java runtime version mismatch",
                "A mod or library requires a newer Java version than the one selected. Switch to a newer Java runtime (e.g. Java 21) in Instance Settings.",
                "Java version mismatch",
                technical,
            );
        }
        if normalized.contains("already running") {
            return Self::build(
                "Instance already running",
                "This instance is already being used by Minecraft. Close that game before launching the same instance again.",
                "Instance already running",
                technical,
            );
        }
        if normalized.contains("required mod dependencies")
            || normalized.contains("missing required dependencies")
        {
            return Self::build(
                "Required mod dependencies are missing",
                "MCW Launcher could not restore every required mod dependency. Retry while online or repair the modpack.",
                "Required dependencies missing",
                technical,
            );
        }
        if normalized.contains("sha-256")
            || normalized.contains("sha1")
            || normalized.contains("hash verification failed")
            || normalized.contains("checksum")
        {
            return Self::build(
                "Download verification failed",
                "A downloaded file did not pass its integrity check. Retry the launch to download a clean copy.",
                "Download verification failed",
                technical,
            );
        }
        if normalized.contains("winerror 206")
            || normalized.contains("os error 206")
            || normalized.contains("filename or extension is too long")
        {
            return Self::build(
                "Windows path is too long",
                "Windows rejected the Minecraft launch command or one of its paths. Move MCW Launcher to a short folder such as C:\\MCW and shorten the instance name.",
                "Windows path is too long",
                technical,
            );
        }
        if normalized.contains("no space") || normalized.contains("disk full") {
            return Self::build(
                "Not enough storage",
                "There is not enough free disk space to finish preparing Minecraft or Java.",
                "Not enough storage",
                technical,
            );
        }
        if normalized.contains("permission") || normalized.contains("access is denied") {
            return Self::build(
                "File access denied",
                "The launcher could not write to one of its folders. Check folder permissions and whether another program is locking the file.",
                "File access denied",
                technical,
            );
        }
        if ["timeout", "timed out", "connection", "network", "http "]
            .iter()
            .any(|k| normalized.contains(k))
        {
            return Self::build(
                "Network error",
                "The launcher could not finish the download. Check the connection and try launching again.",
                "Network error",
                technical,
            );
        }
        if ["java", "adoptium", "temurin", "runtime"]
            .iter()
            .any(|k| normalized.contains(k))
        {
            return Self::build(
                "Java runtime setup failed",
                "The launcher could not prepare the Java runtime required by this Minecraft version.",
                "Java setup failed",
                technical,
            );
        }

        Self::build(
            "Minecraft launch failed",
            "Minecraft could not be started. Inspect the Logs tab or latest crash report for details.",
            "Launch failed",
            technical,
        )
    }

    fn build(title: &str, summary: &str, status: &str, technical: &str) -> LaunchErrorView {
        LaunchErrorView {
            title: title.to_string(),
            summary: summary.to_string(),
            status: status.to_string(),
            technical_message: technical.to_string(),
        }
    }
}

/// Locate the most recently modified `crash-*.txt` file under `<mc_dir>/crash-reports/`.
pub fn find_latest_crash_report(mc_dir: &Path) -> Option<PathBuf> {
    let crash_dir = mc_dir.join("crash-reports");
    let entries = fs::read_dir(&crash_dir).ok()?;
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("crash-") || !name.ends_with(".txt") {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        if best.as_ref().map(|(b_t, _)| mtime >= *b_t).unwrap_or(true) {
            best = Some((mtime, path));
        }
    }
    best.map(|(_, p)| p)
}

pub struct DiagnosticsExporter;

impl DiagnosticsExporter {
    /// Export a sanitized diagnostic support `.zip` bundle for an instance
    /// (port of Python `DiagnosticsManager.export_bundle`).
    #[allow(clippy::too_many_arguments)]
    pub fn export_bundle(
        mc_dir: &Path,
        instance_name: &str,
        version_id: &str,
        loader_name: &str,
        pc_ram_mb: u32,
        java_list: &[JavaInstallation],
        live_logs: &[String],
        output_zip: &Path,
    ) -> std::io::Result<PathBuf> {
        if let Some(parent) = output_zip.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = File::create(output_zip)?;
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        let report_txt = format!(
            "MCW Launcher Diagnostic Report\n\
             ==============================\n\
             schema_version: 2.1\n\
             launcher_version: {}\n\
             platform: {}\n\
             architecture: {}\n\
             system_ram_mb: {pc_ram_mb}\n\n\
             Target Instance\n\
             ---------------\n\
             instance_name: {instance_name}\n\
             minecraft_version: {version_id}\n\
             mod_loader: {loader_name}\n\
             minecraft_dir: {}\n\
             detected_java_runtimes: {}\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            mc_dir.display(),
            java_list.len()
        );
        zip.start_file("report.txt", opts)
            .map_err(std::io::Error::other)?;
        zip.write_all(sanitize_log_text(&report_txt).as_bytes())?;

        let sys_info = serde_json::json!({
            "schema_version": "2.1",
            "launcher_version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "pc_ram_mb": pc_ram_mb,
            "instance_name": instance_name,
            "version_id": version_id,
            "mod_loader": loader_name,
        });
        zip.start_file("system-info.json", opts)
            .map_err(std::io::Error::other)?;
        zip.write_all(
            sanitize_log_text(&serde_json::to_string_pretty(&sys_info).unwrap_or_default())
                .as_bytes(),
        )?;

        let java_json: Vec<serde_json::Value> = java_list
            .iter()
            .map(|j| {
                serde_json::json!({
                    "major": j.major,
                    "version": j.version_string,
                    "path": sanitize_log_text(&j.path.display().to_string()),
                })
            })
            .collect();
        zip.start_file("java-runtimes.json", opts)
            .map_err(std::io::Error::other)?;
        zip.write_all(
            serde_json::to_string_pretty(&java_json)
                .unwrap_or_default()
                .as_bytes(),
        )?;

        for (file_name, payload) in [
            (
                "java-recovery.json",
                serde_json::json!({ "schema_version": "2.1", "detected_count": java_list.len(), "status": if java_list.is_empty() { "missing_java" } else { "healthy" } }),
            ),
            (
                "instance-health.json",
                serde_json::json!({ "schema_version": "2.1", "instance": instance_name, "version_id": version_id, "loader": loader_name, "mc_dir_exists": mc_dir.is_dir(), "mods_dir_exists": mc_dir.join("mods").is_dir(), "has_crash_report": find_latest_crash_report(mc_dir).is_some() }),
            ),
            (
                "process-sessions.json",
                serde_json::json!({ "schema_version": "2.1", "sessions": [] }),
            ),
            (
                "operation-journals.json",
                serde_json::json!({ "schema_version": "2.1", "journals": [] }),
            ),
            (
                "download-recovery.json",
                serde_json::json!({ "schema_version": "2.1", "total_downloaded_bytes": mcw_net::total_downloaded_bytes() }),
            ),
            (
                "task-timeline.json",
                serde_json::json!({ "schema_version": "2.1", "events": [] }),
            ),
            (
                "issue-context.json",
                serde_json::json!({ "schema_version": "2.1", "instance_name": instance_name, "version_id": version_id, "mod_loader": loader_name }),
            ),
            (
                "diagnostic-summary.json",
                serde_json::json!({ "schema_version": "2.1", "healthy": find_latest_crash_report(mc_dir).is_none(), "java_count": java_list.len() }),
            ),
            (
                "collector-errors.json",
                serde_json::json!({ "schema_version": "2.1", "errors": [] }),
            ),
        ] {
            zip.start_file(file_name, opts)
                .map_err(std::io::Error::other)?;
            zip.write_all(
                serde_json::to_string_pretty(&payload)
                    .unwrap_or_default()
                    .as_bytes(),
            )?;
        }

        if !live_logs.is_empty() {
            let joined = sanitize_log_text(&live_logs.join("\n"));
            zip.start_file("logs/live-session.log", opts)
                .map_err(std::io::Error::other)?;
            zip.write_all(joined.as_bytes())?;
        }

        let latest_log = mc_dir.join("logs").join("latest.log");
        if latest_log.is_file() {
            if let Ok(raw) = fs::read_to_string(&latest_log) {
                zip.start_file("logs/latest.log", opts)
                    .map_err(std::io::Error::other)?;
                zip.write_all(sanitize_log_text(&raw).as_bytes())?;
            }
        }

        if let Some(crash_path) = find_latest_crash_report(mc_dir) {
            if let Ok(raw) = fs::read_to_string(&crash_path) {
                zip.start_file("crash-reports/latest-crash.txt", opts)
                    .map_err(std::io::Error::other)?;
                zip.write_all(sanitize_log_text(&raw).as_bytes())?;
            }
        }

        zip.finish().map_err(std::io::Error::other)?;
        Ok(output_zip.to_path_buf())
    }
}

/// Sanitized GitHub / bug report builder mirroring Python `IssueReportDialog` & `IssueReportBuilder`.
#[derive(Debug, Clone, Default)]
pub struct IssueReportInput {
    pub launcher_version: String,
    pub os_info: String,
    pub summary: String,
    pub instance_name: Option<String>,
    pub minecraft_version: Option<String>,
    pub mod_loader: Option<String>,
    pub log_excerpt: String,
}

pub struct IssueReportBuilder;

impl IssueReportBuilder {
    pub fn build_markdown(input: &IssueReportInput) -> String {
        let mut out = String::new();
        out.push_str("## Environment\n");
        out.push_str(&format!(
            "- Launcher: MCW Launcher: Rust Edition {}\n",
            input.launcher_version.trim()
        ));
        out.push_str(&format!("- OS: {}\n", input.os_info.trim()));
        if let Some(ref inst) = input.instance_name {
            let mc = input.minecraft_version.as_deref().unwrap_or("unknown");
            let loader = input.mod_loader.as_deref().unwrap_or("vanilla");
            out.push_str(&format!("- Instance: {inst} (MC {mc} - {loader})\n"));
        }

        out.push_str("\n## Description\n");
        let desc = input.summary.trim();
        if desc.is_empty() {
            out.push_str("(Describe what happened and how to reproduce it.)\n");
        } else {
            out.push_str(&sanitize_log_text(desc));
            out.push('\n');
        }

        let excerpt = sanitize_log_text(input.log_excerpt.trim());
        if !excerpt.is_empty() {
            let lines: Vec<&str> = excerpt.lines().collect();
            let start = lines.len().saturating_sub(60);
            let tail = lines[start..].join("\n");
            out.push_str("\n## Sanitized Log Excerpt\n```text\n");
            out.push_str(&tail);
            out.push_str("\n```\n");
        }
        out
    }

    pub fn build_github_issue_url(input: &IssueReportInput, repo_slug: &str) -> String {
        let body = Self::build_markdown(input);
        let slug = repo_slug.trim().trim_matches('/');
        let slug = if slug.is_empty() {
            "mahiru7229/mcw-launcher-rust"
        } else {
            slug
        };
        format!(
            "https://github.com/{slug}/issues/new?title={}&body={}",
            url_encode_component("[Bug]: Launcher Issue"),
            url_encode_component(&body)
        )
    }
}

fn url_encode_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_sanitized_issue_report_markdown_and_url() {
        let input = IssueReportInput {
            launcher_version: "v0.14.0-beta.1".to_string(),
            os_info: "windows x86_64".to_string(),
            summary: "Crashed when launching".to_string(),
            instance_name: Some("FabricPack".to_string()),
            minecraft_version: Some("1.21.4".to_string()),
            mod_loader: Some("fabric".to_string()),
            log_excerpt: "Token eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abc123def456\nError at C:\\Users\\alice\\mc".to_string(),
        };
        let md = IssueReportBuilder::build_markdown(&input);
        assert!(md.contains("MCW Launcher: Rust Edition v0.14.0-beta.1"));
        assert!(!md.contains("eyJhbGciOi"));
        assert!(!md.contains("alice"));
        assert!(md.contains("<REDACTED_JWT>"));
        let url =
            IssueReportBuilder::build_github_issue_url(&input, "mahiru7229/mcw-launcher-rust");
        assert!(url.starts_with("https://github.com/mahiru7229/mcw-launcher-rust/issues/new?"));
    }

    #[test]
    fn classifies_launch_errors_and_locates_latest_crash_report() {
        let v1 =
            LaunchErrorPresenter::present("WinError 206: filename or extension is too long", false);
        assert_eq!(v1.title, "Windows path is too long");

        let v2 = LaunchErrorPresenter::present("exit code 1", true);
        assert_eq!(v2.title, "Java runtime version mismatch");

        let dir = std::env::temp_dir().join(format!("mcw_diag_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let crash_dir = dir.join("crash-reports");
        fs::create_dir_all(&crash_dir).unwrap();
        let crash_file = crash_dir.join("crash-2026-11-20_client.txt");
        fs::write(&crash_file, "---- Minecraft Crash Report ----\nAccessToken: eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abc123def456").unwrap();

        let found = find_latest_crash_report(&dir).unwrap();
        assert_eq!(found, crash_file);

        let bundle_zip = dir.join("diag.zip");
        DiagnosticsExporter::export_bundle(
            &dir,
            "TestInst",
            "1.21.4",
            "fabric",
            16384,
            &[],
            &["[INFO] Starting".to_string()],
            &bundle_zip,
        )
        .unwrap();
        assert!(bundle_zip.is_file());

        let _ = fs::remove_dir_all(&dir);
    }
}
