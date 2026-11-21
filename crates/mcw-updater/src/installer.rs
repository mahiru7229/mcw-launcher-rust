use crate::{ReleaseInfo, Result, UpdateError};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct StagedUpdate {
    pub version: String,
    pub tag_name: String,
    pub asset_name: String,
    pub staged_path: PathBuf,
    pub size_bytes: u64,
}

pub struct UpdateInstaller;

impl UpdateInstaller {
    /// Download and stage a release archive/binary under `staging_root/<version>/<asset_name>`.
    pub async fn download_and_stage(
        release: &ReleaseInfo,
        staging_root: &Path,
        progress: Option<Arc<dyn Fn(u64, u64) + Send + Sync>>,
    ) -> Result<StagedUpdate> {
        let download_url = release
            .download_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                UpdateError::InstallFailed(
                    "Release does not provide a downloadable package asset for this platform."
                        .to_string(),
                )
            })?;

        let asset_name = release
            .asset_name
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                download_url
                    .split(['/', '?'])
                    .rfind(|seg| !seg.is_empty() && seg.contains('.'))
                    .map(String::from)
            })
            .unwrap_or_else(|| format!("mcw-launcher-{}.zip", sanitize_segment(&release.version)));

        let stage_dir = staging_root.join(sanitize_segment(&release.version));
        fs::create_dir_all(&stage_dir)?;
        let staged_path = stage_dir.join(&asset_name);
        let tmp_path = stage_dir.join(format!("{asset_name}.tmp"));

        let http = reqwest::Client::builder()
            .user_agent(mcw_models::user_agent())
            .timeout(Duration::from_secs(300))
            .build()?;

        let mut resp = http.get(download_url).send().await?.error_for_status()?;
        let total = resp.content_length().or(release.asset_size).unwrap_or(0);

        let mut file = fs::File::create(&tmp_path)?;
        let mut downloaded: u64 = 0;
        while let Some(bytes) = resp.chunk().await? {
            file.write_all(&bytes)?;
            downloaded += bytes.len() as u64;
            if let Some(ref cb) = progress {
                cb(downloaded, total.max(downloaded));
            }
        }
        file.flush()?;
        drop(file);

        if staged_path.exists() {
            let _ = fs::remove_file(&staged_path);
        }
        fs::rename(&tmp_path, &staged_path)?;

        Ok(StagedUpdate {
            version: release.version.clone(),
            tag_name: release.tag_name.clone(),
            asset_name,
            staged_path,
            size_bytes: downloaded,
        })
    }

    /// Locate the external standalone updater binary next to the installed launcher executable.
    pub fn find_updater_binary(install_dir: &Path) -> Option<PathBuf> {
        #[cfg(windows)]
        let candidates = ["mcw-updater.exe", "updater.exe"];
        #[cfg(not(windows))]
        let candidates = ["mcw-updater", "updater"];

        candidates
            .iter()
            .map(|name| install_dir.join(name))
            .find(|p| p.is_file())
    }

    /// Generate the detached Windows PowerShell self-update fallback script with
    /// transactional backup/rollback and protected user data folders.
    pub fn build_windows_update_script(
        staged_path: &Path,
        install_dir: &Path,
        target_exe: &Path,
        wait_pid: u32,
    ) -> String {
        let staged = escape_ps_single_quoted(&staged_path.display().to_string());
        let install = escape_ps_single_quoted(&install_dir.display().to_string());
        let exe = escape_ps_single_quoted(&target_exe.display().to_string());

        format!(
            r#"$ErrorActionPreference = 'Stop'
$waitPid = {wait_pid}
$stagedPath = '{staged}'
$installDir = '{install}'
$targetExe = '{exe}'
$protectedDirs = @('instances', 'accounts', 'config', 'cache', 'backups', 'logs', 'runtimes', 'themes', 'lang')
$backupDir = Join-Path $installDir ('.mcw-update-backup-' + $waitPid)

$deadline = (Get-Date).AddSeconds(60)
while ((Get-Date) -lt $deadline) {{
    $proc = Get-Process -Id $waitPid -ErrorAction SilentlyContinue
    if ($null -eq $proc) {{ break }}
    Start-Sleep -Milliseconds 250
}}

Start-Sleep -Milliseconds 300

try {{
    New-Item -ItemType Directory -Path $backupDir -Force | Out-Null
    Get-ChildItem -Path $installDir | ForEach-Object {{
        if ($_.Name -eq $backupDir -or $_.Name.StartsWith('.mcw-update-backup')) {{ return }}
        if ($_.PSIsContainer -and ($protectedDirs -contains $_.Name.ToLowerInvariant())) {{ return }}
        Copy-Item -Path $_.FullName -Destination (Join-Path $backupDir $_.Name) -Recurse -Force
    }}

    if ($stagedPath.ToLowerInvariant().EndsWith('.zip')) {{
        $extractDir = Join-Path ([System.IO.Path]::GetTempPath()) ("mcw-extract-" + $waitPid + "-" + [guid]::NewGuid().ToString('N'))
        try {{
            New-Item -ItemType Directory -Path $extractDir -Force | Out-Null
            Expand-Archive -Path $stagedPath -DestinationPath $extractDir -Force
            $sourceRoot = $extractDir
            $topEntries = Get-ChildItem -Path $extractDir
            if ($topEntries.Count -eq 1 -and $topEntries[0].PSIsContainer -and -not (Test-Path (Join-Path $extractDir ([System.IO.Path]::GetFileName($targetExe))))) {{
                $sourceRoot = $topEntries[0].FullName
            }}
            Get-ChildItem -Path $sourceRoot | ForEach-Object {{
                if ($_.PSIsContainer -and ($protectedDirs -contains $_.Name.ToLowerInvariant())) {{
                    return
                }}
                $dest = Join-Path $installDir $_.Name
                Copy-Item -Path $_.FullName -Destination $dest -Recurse -Force
            }}
        }} finally {{
            if (Test-Path $extractDir) {{
                Remove-Item -Path $extractDir -Recurse -Force -ErrorAction SilentlyContinue
            }}
        }}
    }} else {{
        $copied = $false
        for ($attempt = 0; $attempt -lt 20; $attempt++) {{
            try {{
                Copy-Item -Path $stagedPath -Destination $targetExe -Force
                $copied = $true
                break
            }} catch {{
                Start-Sleep -Milliseconds 250
            }}
        }}
        if (-not $copied) {{ throw 'Timed out waiting to replace launcher executable.' }}
    }}
}} catch {{
    if (Test-Path $backupDir) {{
        Get-ChildItem -Path $backupDir | ForEach-Object {{
            Copy-Item -Path $_.FullName -Destination (Join-Path $installDir $_.Name) -Recurse -Force
        }}
    }}
}} finally {{
    if (Test-Path $backupDir) {{
        Remove-Item -Path $backupDir -Recurse -Force -ErrorAction SilentlyContinue
    }}
}}

Start-Process -FilePath $targetExe -WorkingDirectory $installDir
"#
        )
    }

    /// Generate the detached Unix shell self-update fallback script with backup/rollback.
    pub fn build_linux_update_script(
        staged_path: &Path,
        install_dir: &Path,
        target_exe: &Path,
        wait_pid: u32,
    ) -> String {
        let staged = escape_sh_single_quoted(&staged_path.display().to_string());
        let install = escape_sh_single_quoted(&install_dir.display().to_string());
        let exe = escape_sh_single_quoted(&target_exe.display().to_string());

        format!(
            r#"#!/usr/bin/env sh
set -eu
WAIT_PID={wait_pid}
STAGED='{staged}'
INSTALL_DIR='{install}'
TARGET_EXE='{exe}'
BACKUP_DIR="$INSTALL_DIR/.mcw-update-backup-$WAIT_PID"

i=0
while [ $i -lt 240 ]; do
  if ! kill -0 "$WAIT_PID" 2>/dev/null; then
    break
  fi
  sleep 0.25
  i=$((i + 1))
done

mkdir -p "$BACKUP_DIR"
for item in "$INSTALL_DIR"/*; do
  [ -e "$item" ] || continue
  base="$(basename "$item")"
  case "$base" in
    instances|accounts|config|cache|backups|logs|runtimes|themes|lang|.mcw-update-backup*) continue ;;
  esac
  cp -rf "$item" "$BACKUP_DIR/" || true
done

rollback() {{
  cp -rf "$BACKUP_DIR"/* "$INSTALL_DIR/" 2>/dev/null || true
  rm -rf "$BACKUP_DIR"
}}

apply_update() {{
  case "$STAGED" in
    *.tar.gz|*.tgz)
      TMP_DIR="$(mktemp -d)"
      tar -xzf "$STAGED" -C "$TMP_DIR"
      for item in "$TMP_DIR"/*; do
        base="$(basename "$item")"
        case "$base" in
          instances|accounts|config|cache|backups|logs|runtimes|themes|lang) continue ;;
        esac
        cp -rf "$item" "$INSTALL_DIR/"
      done
      rm -rf "$TMP_DIR"
      ;;
    *.zip)
      TMP_DIR="$(mktemp -d)"
      unzip -o "$STAGED" -d "$TMP_DIR" >/dev/null
      for item in "$TMP_DIR"/*; do
        base="$(basename "$item")"
        case "$base" in
          instances|accounts|config|cache|backups|logs|runtimes|themes|lang) continue ;;
        esac
        cp -rf "$item" "$INSTALL_DIR/"
      done
      rm -rf "$TMP_DIR"
      ;;
    *)
      cp -f "$STAGED" "$TARGET_EXE"
      chmod +x "$TARGET_EXE"
      ;;
  esac
}}

if ! apply_update; then
  rollback
fi
rm -rf "$BACKUP_DIR"

chmod +x "$TARGET_EXE" || true
nohup "$TARGET_EXE" >/dev/null 2>&1 &
"#
        )
    }

    /// Spawn the external transactional updater (`mcw-updater`) with automatic fallback to
    /// Method 2 (detached PowerShell/sh self-update script with rollback) if unavailable.
    pub fn launch_self_update(
        staged: &StagedUpdate,
        current_exe: &Path,
        current_pid: u32,
    ) -> Result<()> {
        if !staged.staged_path.is_file() {
            return Err(UpdateError::InstallFailed(format!(
                "Staged update file not found at {}",
                staged.staged_path.display()
            )));
        }
        let install_dir = current_exe.parent().ok_or_else(|| {
            UpdateError::InstallFailed("Unable to determine launcher install directory".to_string())
        })?;

        // Method 1 (Primary): Copy external updater executable to temp dir & spawn detached
        // so it can safely display progress, replace even itself in install_dir, and roll back cleanly.
        if let Some(updater_bin) = Self::find_updater_binary(install_dir) {
            let ext = updater_bin
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| format!(".{s}"))
                .unwrap_or_default();
            let temp_runner =
                std::env::temp_dir().join(format!("mcw-updater-runner-{current_pid}{ext}"));
            if fs::copy(&updater_bin, &temp_runner).is_ok() {
                #[cfg(unix)]
                if let Ok(meta) = fs::metadata(&updater_bin) {
                    let _ = fs::set_permissions(&temp_runner, meta.permissions());
                }
                let spawn_res = Command::new(&temp_runner)
                    .args([
                        "--wait-pid",
                        &current_pid.to_string(),
                        "--staged",
                        &staged.staged_path.display().to_string(),
                        "--install-dir",
                        &install_dir.display().to_string(),
                        "--target-exe",
                        &current_exe.display().to_string(),
                        "--version",
                        &staged.version,
                    ])
                    .spawn();
                if spawn_res.is_ok() {
                    return Ok(());
                }
                let _ = fs::remove_file(&temp_runner);
            }
        }

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            const DETACHED_PROCESS: u32 = 0x0000_0008;

            let script_path = std::env::temp_dir().join(format!("mcw-update-{current_pid}.ps1"));
            let script = Self::build_windows_update_script(
                &staged.staged_path,
                install_dir,
                current_exe,
                current_pid,
            );
            fs::write(&script_path, script)?;

            Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-WindowStyle",
                    "Hidden",
                    "-File",
                    &script_path.display().to_string(),
                ])
                .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
                .spawn()?;
            Ok(())
        }

        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            let script_path = std::env::temp_dir().join(format!("mcw-update-{current_pid}.sh"));
            let script = Self::build_linux_update_script(
                &staged.staged_path,
                install_dir,
                current_exe,
                current_pid,
            );
            fs::write(&script_path, script)?;
            let mut perms = fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_path, perms)?;

            Command::new("sh").arg(&script_path).spawn()?;
            Ok(())
        }
    }
}

fn sanitize_segment(raw: &str) -> String {
    let clean: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if clean.is_empty() {
        "update".to_string()
    } else {
        clean
    }
}

fn escape_ps_single_quoted(s: &str) -> String {
    s.replace('\'', "''")
}

fn escape_sh_single_quoted(s: &str) -> String {
    s.replace('\'', "'\\''")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_update_script_waits_for_pid_and_protects_user_data() {
        let script = UpdateInstaller::build_windows_update_script(
            Path::new(r"C:\mcw\cache\updates\0.8.0-beta.1\mcw-win.zip"),
            Path::new(r"C:\mcw"),
            Path::new(r"C:\mcw\mcw-desktop.exe"),
            4242,
        );
        assert!(script.contains("$waitPid = 4242"));
        assert!(script.contains("'instances'"));
        assert!(script.contains("'accounts'"));
        assert!(script.contains("'config'"));
        assert!(script.contains("'themes'"));
        assert!(script.contains("$backupDir"));
        assert!(script.contains("Start-Process -FilePath $targetExe"));

        let tmp = tempfile::tempdir().unwrap();
        assert!(UpdateInstaller::find_updater_binary(tmp.path()).is_none());
        #[cfg(windows)]
        let fake_name = "mcw-updater.exe";
        #[cfg(not(windows))]
        let fake_name = "mcw-updater";
        fs::write(tmp.path().join(fake_name), b"bin").unwrap();
        assert_eq!(
            UpdateInstaller::find_updater_binary(tmp.path()).as_deref(),
            Some(tmp.path().join(fake_name).as_path())
        );
    }

    #[tokio::test]
    async fn download_and_stage_errors_cleanly_when_url_missing() {
        let release = ReleaseInfo {
            tag_name: "v0.8.0-beta.2".to_string(),
            version: "0.8.0-beta.2".to_string(),
            prerelease: true,
            source: "github".to_string(),
            notes: String::new(),
            html_url: String::new(),
            download_url: None,
            asset_name: None,
            asset_size: None,
        };
        let dir = tempfile::tempdir().unwrap();
        let err = UpdateInstaller::download_and_stage(&release, dir.path(), None)
            .await
            .unwrap_err();
        assert!(matches!(err, UpdateError::InstallFailed(_)));
    }
}
