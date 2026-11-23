#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{
    self, Align, Button, Color32, CornerRadius, Frame, Layout, RichText, ScrollArea, Vec2,
};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static STAGE_SEQ: AtomicU64 = AtomicU64::new(1);

const PROTECTED_DIRS: &[&str] = &[
    "instances",
    "accounts",
    "config",
    "cache",
    "backups",
    "logs",
    "runtimes",
    "themes",
    "lang",
];

const MIGRATION_USER_DIRS: &[&str] = &[
    "instances",
    "accounts",
    "config",
    "themes",
    "backups",
    "logs",
    "runtimes",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MigrationChoice {
    BackupAndMove,
    DirectMove,
    DirectInstallEmpty,
}

#[derive(Debug, Clone)]
pub struct UpdaterArgs {
    pub wait_pid: u32,
    pub staged: PathBuf,
    pub install_dir: PathBuf,
    pub target_exe: PathBuf,
    pub version: String,
}

#[derive(Debug, Clone)]
enum WorkerMsg {
    Progress {
        fraction: f32,
        status: String,
    },
    Success(Option<UpdaterArgs>),
    PermissionDenied {
        error: String,
        source_dir: PathBuf,
        target_dir: PathBuf,
        has_user_data: bool,
    },
    RolledBackError(String),
}

enum UpdaterUiState {
    Running {
        fraction: f32,
        status: String,
    },
    Done,
    PermissionMigrationRequired {
        error: String,
        source_dir: PathBuf,
        target_dir: PathBuf,
        has_user_data: bool,
        shown_at: Instant,
    },
    ManualCancelled {
        source_dir: PathBuf,
        target_dir: PathBuf,
    },
    Failed {
        error: String,
        status: String,
    },
}

fn parse_args_from(args: &[String]) -> Result<UpdaterArgs, String> {
    let mut wait_pid: Option<u32> = None;
    let mut staged: Option<PathBuf> = None;
    let mut install_dir: Option<PathBuf> = None;
    let mut target_exe: Option<PathBuf> = None;
    let mut version = String::from("latest");

    let mut i = 1usize;
    while i < args.len() {
        let key = args[i].as_str();
        let next = args.get(i + 1);
        match key {
            "--wait-pid" => {
                let raw = next.ok_or("Missing value for --wait-pid")?;
                wait_pid = Some(
                    raw.parse::<u32>()
                        .map_err(|e| format!("Invalid --wait-pid '{raw}': {e}"))?,
                );
                i += 2;
            }
            "--staged" => {
                let raw = next.ok_or("Missing value for --staged")?;
                staged = Some(PathBuf::from(raw));
                i += 2;
            }
            "--install-dir" => {
                let raw = next.ok_or("Missing value for --install-dir")?;
                install_dir = Some(PathBuf::from(raw));
                i += 2;
            }
            "--target-exe" => {
                let raw = next.ok_or("Missing value for --target-exe")?;
                target_exe = Some(PathBuf::from(raw));
                i += 2;
            }
            "--version" => {
                let raw = next.ok_or("Missing value for --version")?;
                version = raw.clone();
                i += 2;
            }
            other => return Err(format!("Unknown updater flag: {other}")),
        }
    }

    let staged = staged.ok_or("Missing required --staged path")?;
    let target_exe = target_exe.ok_or("Missing required --target-exe path")?;
    let install_dir = install_dir.unwrap_or_else(|| {
        target_exe
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    });

    Ok(UpdaterArgs {
        wait_pid: wait_pid.unwrap_or(0),
        staged,
        install_dir,
        target_exe,
        version,
    })
}

fn is_protected_entry(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    PROTECTED_DIRS.contains(&lower.as_str()) || lower.starts_with(".mcw-update-backup")
}

fn wait_for_pid_exit(pid: u32, max_wait: Duration) {
    if pid == 0 {
        return;
    }
    let start = Instant::now();
    while start.elapsed() < max_wait {
        if !is_pid_running(pid) {
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    thread::sleep(Duration::from_millis(300));
}

#[cfg(target_os = "windows")]
fn is_pid_running(pid: u32) -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(
            desired_access: u32,
            inherit_handle: i32,
            process_id: u32,
        ) -> *mut core::ffi::c_void;
        fn WaitForSingleObject(handle: *mut core::ffi::c_void, millis: u32) -> u32;
        fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
    }
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_TIMEOUT: u32 = 0x0000_0102;
    unsafe {
        let handle = OpenProcess(SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return false;
        }
        let res = WaitForSingleObject(handle, 0);
        CloseHandle(handle);
        res == WAIT_TIMEOUT
    }
}

#[cfg(not(target_os = "windows"))]
fn is_pid_running(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn extract_staged_source(
    staged: &Path,
    target_exe: &Path,
    extract_dir: &Path,
) -> Result<PathBuf, String> {
    if !staged.is_file() {
        return Err(format!("Staged update file missing: {}", staged.display()));
    }
    fs::create_dir_all(extract_dir).map_err(|e| format!("Failed creating staging dir: {e}"))?;

    let lower = staged
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if lower.ends_with(".zip") {
        let file = File::open(staged).map_err(|e| format!("Cannot open zip archive: {e}"))?;
        let mut archive =
            zip::ZipArchive::new(file).map_err(|e| format!("Corrupt zip archive: {e}"))?;
        for idx in 0..archive.len() {
            let mut entry = archive
                .by_index(idx)
                .map_err(|e| format!("Zip read failure: {e}"))?;
            let Some(enclosed) = entry.enclosed_name() else {
                continue;
            };
            let out_path = extract_dir.join(enclosed);
            if entry.is_dir() {
                fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
            } else {
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                let mut out_file = File::create(&out_path).map_err(|e| e.to_string())?;
                std::io::copy(&mut entry, &mut out_file).map_err(|e| e.to_string())?;
            }
        }
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        let status = Command::new("tar")
            .args([
                "-xzf",
                &staged.display().to_string(),
                "-C",
                &extract_dir.display().to_string(),
            ])
            .status()
            .map_err(|e| format!("Failed running tar: {e}"))?;
        if !status.success() {
            return Err(format!("tar extraction exited with status {status}"));
        }
    } else {
        let exe_name = target_exe
            .file_name()
            .ok_or("Invalid target executable filename")?;
        let dest = extract_dir.join(exe_name);
        fs::copy(staged, &dest).map_err(|e| format!("Failed staging binary: {e}"))?;
    }

    // Unwrap single nested folder if present (matching GitHub release zip packaging).
    let target_name = target_exe
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    if !target_name.is_empty() && !extract_dir.join(&target_name).is_file() {
        let entries: Vec<PathBuf> = fs::read_dir(extract_dir)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .collect();
        if entries.len() == 1 && entries[0].is_dir() {
            return Ok(entries[0].clone());
        }
    }

    Ok(extract_dir.to_path_buf())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("mkdir {}: {e}", dst.display()))?;
    let entries = fs::read_dir(src).map_err(|e| format!("readdir {}: {e}", src.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let src_path = entry.path();
        let dst_path = dst.join(&name);
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if src_path.is_file() {
            copy_with_retry(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn copy_with_retry(src: &Path, dst: &Path) -> Result<(), String> {
    let mut last_err = String::new();
    for attempt in 0..20 {
        match fs::copy(src, dst) {
            Ok(_) => {
                #[cfg(unix)]
                if let Ok(meta) = fs::metadata(src) {
                    let _ = fs::set_permissions(dst, meta.permissions());
                }
                return Ok(());
            }
            Err(e) => {
                last_err = e.to_string();
                if attempt < 19 {
                    thread::sleep(Duration::from_millis(250));
                }
            }
        }
    }
    Err(format!(
        "Failed copying {} -> {}: {last_err}",
        src.display(),
        dst.display()
    ))
}

fn rollback_backup(
    install_dir: &Path,
    backup_dir: &Path,
    backed_up_names: &[String],
    newly_added_names: &[String],
) -> Result<(), String> {
    for added in newly_added_names {
        let p = install_dir.join(added);
        if p.is_dir() {
            let _ = fs::remove_dir_all(&p);
        } else if p.is_file() {
            let _ = fs::remove_file(&p);
        }
    }
    for name in backed_up_names {
        let b_path = backup_dir.join(name);
        let d_path = install_dir.join(name);
        if b_path.is_dir() {
            if d_path.exists() {
                let _ = fs::remove_dir_all(&d_path);
            }
            copy_dir_recursive(&b_path, &d_path)?;
        } else if b_path.is_file() {
            copy_with_retry(&b_path, &d_path)?;
        }
    }
    Ok(())
}

fn append_updater_log(install_dir: &Path, message: &str) {
    let log_dir = install_dir.join("logs");
    if fs::create_dir_all(&log_dir).is_ok() {
        let log_path = log_dir.join("updater.log");
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
        {
            let _ = writeln!(f, "{message}");
        }
    }
}

pub fn apply_transactional_update<F: FnMut(f32, &str)>(
    args: &UpdaterArgs,
    inject_fail_after_backup: bool,
    mut on_progress: F,
) -> Result<(), String> {
    on_progress(0.10, "Waiting for MCW Launcher to close...");
    wait_for_pid_exit(args.wait_pid, Duration::from_secs(60));

    let seq = STAGE_SEQ.fetch_add(1, Ordering::Relaxed);
    let extract_dir =
        std::env::temp_dir().join(format!("mcw-updater-stage-{}-{seq}", std::process::id()));
    if extract_dir.exists() {
        let _ = fs::remove_dir_all(&extract_dir);
    }
    let backup_dir = args
        .install_dir
        .join(format!(".mcw-update-backup-{}-{seq}", std::process::id()));

    on_progress(0.32, "Unpacking verified update package...");
    let source_root = match extract_staged_source(&args.staged, &args.target_exe, &extract_dir) {
        Ok(root) => root,
        Err(e) => {
            let _ = fs::remove_dir_all(&extract_dir);
            append_updater_log(&args.install_dir, &format!("[ERROR] Unpack failed: {e}"));
            return Err(e);
        }
    };

    on_progress(0.55, "Backing up current version for safety...");
    if backup_dir.exists() {
        let _ = fs::remove_dir_all(&backup_dir);
    }
    if let Err(e) = fs::create_dir_all(&backup_dir) {
        let _ = fs::remove_dir_all(&extract_dir);
        return Err(format!("Failed creating backup directory: {e}"));
    }

    let mut backed_up_names: Vec<String> = Vec::new();
    let mut existing_names: HashSet<String> = HashSet::new();
    if let Ok(entries) = fs::read_dir(&args.install_dir) {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            existing_names.insert(fname.clone());
            if is_protected_entry(&fname) {
                continue;
            }
            let src_p = entry.path();
            let dst_p = backup_dir.join(&fname);
            let backup_res = if src_p.is_dir() {
                copy_dir_recursive(&src_p, &dst_p)
            } else if src_p.is_file() {
                fs::copy(&src_p, &dst_p)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            } else {
                Ok(())
            };
            if let Err(e) = backup_res {
                let _ = fs::remove_dir_all(&backup_dir);
                let _ = fs::remove_dir_all(&extract_dir);
                append_updater_log(&args.install_dir, &format!("[ERROR] Backup failed: {e}"));
                return Err(format!("Failed creating pre-update backup: {e}"));
            }
            backed_up_names.push(fname);
        }
    }

    on_progress(0.78, "Installing updated launcher binaries...");
    let mut newly_added_names: Vec<String> = Vec::new();
    let apply_res = (|| -> Result<(), String> {
        let entries = fs::read_dir(&source_root).map_err(|e| e.to_string())?;
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if is_protected_entry(&fname) {
                continue;
            }
            if !existing_names.contains(&fname) {
                newly_added_names.push(fname.clone());
            }
            let src_p = entry.path();
            let dst_p = args.install_dir.join(&fname);
            if src_p.is_dir() {
                copy_dir_recursive(&src_p, &dst_p)?;
            } else if src_p.is_file() {
                copy_with_retry(&src_p, &dst_p)?;
            }
        }
        if inject_fail_after_backup {
            return Err(
                "Injected apply failure for transactional rollback verification".to_string(),
            );
        }
        Ok(())
    })();

    let _ = fs::remove_dir_all(&extract_dir);

    match apply_res {
        Ok(()) => {
            on_progress(1.0, "Update applied! Relaunching MCW Launcher...");
            let _ = fs::remove_dir_all(&backup_dir);
            append_updater_log(
                &args.install_dir,
                &format!("[OK] Applied update v{} cleanly.", args.version),
            );
            Ok(())
        }
        Err(apply_err) => {
            on_progress(0.90, "Update failed - rolling back to previous version...");
            let rb_res = rollback_backup(
                &args.install_dir,
                &backup_dir,
                &backed_up_names,
                &newly_added_names,
            );
            let _ = fs::remove_dir_all(&backup_dir);
            match rb_res {
                Ok(()) => {
                    let msg = format!(
                        "{apply_err}. Safely rolled back to the previous launcher version."
                    );
                    append_updater_log(&args.install_dir, &format!("[ROLLBACK] {msg}"));
                    Err(msg)
                }
                Err(rb_err) => {
                    let msg = format!("{apply_err}. Rollback encountered an error: {rb_err}");
                    append_updater_log(&args.install_dir, &format!("[ERROR] {msg}"));
                    Err(msg)
                }
            }
        }
    }
}

fn spawn_target_launcher(args: &UpdaterArgs) {
    let mut cmd = Command::new(&args.target_exe);
    cmd.current_dir(&args.install_dir);
    let _ = cmd.spawn();
}

fn default_safe_launcher_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local).join("MCWLauncher");
        }
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            return PathBuf::from(profile)
                .join("AppData")
                .join("Local")
                .join("MCWLauncher");
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(xdg).join("mcw-launcher");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("mcw-launcher");
        }
    }
    std::env::temp_dir().join("MCWLauncher")
}

fn probe_install_writable(dir: &Path, target_exe: &Path) -> Result<(), String> {
    if !dir.exists() {
        fs::create_dir_all(dir)
            .map_err(|e| format!("Cannot create directory {}: {e}", dir.display()))?;
    }
    let probe = dir.join(format!(".mcw-write-probe-{}", std::process::id()));
    let file =
        fs::File::create(&probe).map_err(|e| format!("Cannot write in {}: {e}", dir.display()))?;
    drop(file);
    let _ = fs::remove_file(&probe);

    if target_exe.is_file() {
        fs::OpenOptions::new()
            .write(true)
            .open(target_exe)
            .map_err(|e| format!("Cannot overwrite {}: {e}", target_exe.display()))?;
    }
    Ok(())
}

fn is_permission_denied_err(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("os error 5")
        || lower.contains("access is denied")
        || lower.contains("permission denied")
        || lower.contains("read-only file system")
}

fn dir_has_user_data(dir: &Path) -> bool {
    ["instances", "accounts", "config", "themes", "backups"]
        .iter()
        .any(|sub| {
            let p = dir.join(sub);
            p.is_dir()
                && fs::read_dir(&p)
                    .map(|mut rd| rd.next().is_some())
                    .unwrap_or(false)
        })
}

fn add_dir_to_zip(
    zip: &mut zip::ZipWriter<File>,
    root_dir: &Path,
    rel_prefix: &str,
    skip_zip_name: Option<&str>,
) -> Result<(), String> {
    let entries = fs::read_dir(root_dir).map_err(|e| e.to_string())?;
    let opts = zip::write::SimpleFileOptions::default();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if skip_zip_name.is_some_and(|skip| skip == name) {
            continue;
        }
        let path = entry.path();
        let entry_rel = if rel_prefix.is_empty() {
            name.clone()
        } else {
            format!("{rel_prefix}/{name}")
        };
        if path.is_dir() {
            add_dir_to_zip(zip, &path, &entry_rel, None)?;
        } else if path.is_file() {
            zip.start_file(&entry_rel, opts)
                .map_err(|e| e.to_string())?;
            let mut f = File::open(&path).map_err(|e| e.to_string())?;
            std::io::copy(&mut f, zip).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn create_user_data_backup_zip(source_dir: &Path, target_dir: &Path) -> Result<PathBuf, String> {
    let backups_dir = target_dir.join("backups");
    fs::create_dir_all(&backups_dir)
        .map_err(|e| format!("mkdir {}: {e}", backups_dir.display()))?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let zip_name = format!("pre-migration-backup-{ts}.zip");
    let zip_path = backups_dir.join(&zip_name);
    let file = File::create(&zip_path).map_err(|e| format!("create backup zip: {e}"))?;
    let mut zw = zip::ZipWriter::new(file);

    for sub in [
        "instances",
        "accounts",
        "config",
        "themes",
        "backups",
        "logs",
    ] {
        let src_sub = source_dir.join(sub);
        if src_sub.is_dir() {
            add_dir_to_zip(&mut zw, &src_sub, sub, Some(&zip_name))?;
        }
    }
    if let Ok(entries) = fs::read_dir(source_dir) {
        let opts = zip::write::SimpleFileOptions::default();
        for entry in entries.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if p.is_file() && name.to_ascii_lowercase().ends_with(".json") {
                zw.start_file(&name, opts).map_err(|e| e.to_string())?;
                let mut f = File::open(&p).map_err(|e| e.to_string())?;
                std::io::copy(&mut f, &mut zw).map_err(|e| e.to_string())?;
            }
        }
    }
    zw.finish()
        .map_err(|e| format!("finalize backup zip: {e}"))?;
    Ok(zip_path)
}

fn copy_user_data_to_target(source_dir: &Path, target_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(target_dir).map_err(|e| format!("mkdir {}: {e}", target_dir.display()))?;
    for sub in MIGRATION_USER_DIRS {
        let src_sub = source_dir.join(sub);
        if src_sub.is_dir() {
            copy_dir_recursive(&src_sub, &target_dir.join(sub))?;
        }
    }
    if let Ok(entries) = fs::read_dir(source_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if p.is_file() && name.to_ascii_lowercase().ends_with(".json") {
                copy_with_retry(&p, &target_dir.join(&name))?;
            }
        }
    }
    Ok(())
}

fn open_folder(path: &Path) {
    #[cfg(windows)]
    {
        let _ = Command::new("explorer.exe").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(path).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if Command::new("xdg-open").arg(path).spawn().is_err() {
            let _ = Command::new("gio").arg("open").arg(path).spawn();
        }
    }
}

fn spawn_migration_worker(
    args: UpdaterArgs,
    source_dir: PathBuf,
    target_dir: PathBuf,
    choice: MigrationChoice,
    tx: mpsc::Sender<WorkerMsg>,
    ctx: egui::Context,
) {
    thread::spawn(move || {
        let work = (|| -> Result<UpdaterArgs, String> {
            if choice == MigrationChoice::BackupAndMove {
                let _ = tx.send(WorkerMsg::Progress {
                    fraction: 0.15,
                    status: "Creating safety backup ZIP of all instances & worlds...".to_string(),
                });
                ctx.request_repaint();
                create_user_data_backup_zip(&source_dir, &target_dir)?;
            }
            if matches!(
                choice,
                MigrationChoice::BackupAndMove | MigrationChoice::DirectMove
            ) {
                let _ = tx.send(WorkerMsg::Progress {
                    fraction: 0.40,
                    status: format!("Moving user data & worlds to {}...", target_dir.display()),
                });
                ctx.request_repaint();
                copy_user_data_to_target(&source_dir, &target_dir)?;
            }
            let exe_name = args
                .target_exe
                .file_name()
                .map(|n| n.to_os_string())
                .unwrap_or_else(|| "mcw-desktop".into());
            let new_args = UpdaterArgs {
                wait_pid: 0,
                staged: args.staged.clone(),
                install_dir: target_dir.clone(),
                target_exe: target_dir.join(exe_name),
                version: args.version.clone(),
            };
            apply_transactional_update(&new_args, false, |fraction, status| {
                let scaled = 0.55 + fraction * 0.45;
                let _ = tx.send(WorkerMsg::Progress {
                    fraction: scaled,
                    status: status.to_string(),
                });
                ctx.request_repaint();
            })?;
            Ok(new_args)
        })();
        match work {
            Ok(new_args) => {
                let _ = tx.send(WorkerMsg::Success(Some(new_args)));
            }
            Err(err) => {
                let _ = tx.send(WorkerMsg::RolledBackError(err));
            }
        }
        ctx.request_repaint();
    });
}

struct UpdaterApp {
    args: UpdaterArgs,
    tx: mpsc::Sender<WorkerMsg>,
    rx: Receiver<WorkerMsg>,
    state: UpdaterUiState,
    logo_texture: Option<egui::TextureHandle>,
    done_at: Option<Instant>,
    relaunched: bool,
}

impl UpdaterApp {
    fn new(cc: &eframe::CreationContext<'_>, args: UpdaterArgs) -> Self {
        let mut style = egui::Style {
            visuals: egui::Visuals::dark(),
            ..Default::default()
        };
        style.visuals.window_fill = Color32::from_rgb(0x0c, 0x0c, 0x12);
        style.visuals.panel_fill = Color32::BLACK;
        cc.egui_ctx.set_style(style);

        let logo_texture = load_logo_texture(&cc.egui_ctx);
        let (tx, rx) = mpsc::channel();
        let tx_worker = tx.clone();
        let worker_args = args.clone();
        let ctx_w = cc.egui_ctx.clone();

        thread::spawn(move || {
            wait_for_pid_exit(worker_args.wait_pid, Duration::from_secs(60));
            let safe_dir = default_safe_launcher_dir();
            if let Err(perm_err) =
                probe_install_writable(&worker_args.install_dir, &worker_args.target_exe)
            {
                if worker_args.install_dir != safe_dir {
                    let _ = tx_worker.send(WorkerMsg::PermissionDenied {
                        error: perm_err,
                        source_dir: worker_args.install_dir.clone(),
                        target_dir: safe_dir,
                        has_user_data: dir_has_user_data(&worker_args.install_dir),
                    });
                    ctx_w.request_repaint();
                    return;
                }
            }

            let mut exec_args = worker_args.clone();
            exec_args.wait_pid = 0;
            let res = apply_transactional_update(&exec_args, false, |fraction, status| {
                let _ = tx_worker.send(WorkerMsg::Progress {
                    fraction,
                    status: status.to_string(),
                });
                ctx_w.request_repaint();
            });
            match res {
                Ok(()) => {
                    let _ = tx_worker.send(WorkerMsg::Success(None));
                }
                Err(err) => {
                    if is_permission_denied_err(&err) && worker_args.install_dir != safe_dir {
                        let _ = tx_worker.send(WorkerMsg::PermissionDenied {
                            error: err,
                            source_dir: worker_args.install_dir.clone(),
                            target_dir: safe_dir,
                            has_user_data: dir_has_user_data(&worker_args.install_dir),
                        });
                    } else {
                        let _ = tx_worker.send(WorkerMsg::RolledBackError(err));
                    }
                }
            }
            ctx_w.request_repaint();
        });

        Self {
            args,
            tx,
            rx,
            state: UpdaterUiState::Running {
                fraction: 0.05,
                status: "Starting MCW Launcher Updater...".to_string(),
            },
            logo_texture,
            done_at: None,
            relaunched: false,
        }
    }
}

impl eframe::App for UpdaterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                WorkerMsg::Progress { fraction, status } => {
                    self.state = UpdaterUiState::Running { fraction, status };
                }
                WorkerMsg::Success(relocated_args) => {
                    if let Some(new_args) = relocated_args {
                        self.args = new_args;
                    }
                    self.state = UpdaterUiState::Done;
                    self.done_at = Some(Instant::now());
                    if !self.relaunched {
                        self.relaunched = true;
                        spawn_target_launcher(&self.args);
                    }
                }
                WorkerMsg::PermissionDenied {
                    error,
                    source_dir,
                    target_dir,
                    has_user_data,
                } => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(
                        560.0, 390.0,
                    )));
                    self.state = UpdaterUiState::PermissionMigrationRequired {
                        error,
                        source_dir,
                        target_dir,
                        has_user_data,
                        shown_at: Instant::now(),
                    };
                }
                WorkerMsg::RolledBackError(err) => {
                    self.state = UpdaterUiState::Failed {
                        status: "Update failed - previous version restored.".to_string(),
                        error: err,
                    };
                }
            }
        }

        if let UpdaterUiState::Done = self.state {
            if let Some(at) = self.done_at {
                if at.elapsed() >= Duration::from_millis(450) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                } else {
                    ctx.request_repaint_after(Duration::from_millis(50));
                }
            }
        } else if matches!(self.state, UpdaterUiState::Running { .. }) {
            ctx.request_repaint_after(Duration::from_millis(40));
        } else if let UpdaterUiState::PermissionMigrationRequired { shown_at, .. } = self.state {
            if shown_at.elapsed() < Duration::from_secs(5) {
                if ctx.input(|i| i.viewport().close_requested()) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                }
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }

        let accent = Color32::from_rgb(0x7c, 0x3a, 0xed);
        let warning = Color32::from_rgb(0xf5, 0x9e, 0x0b);
        let text = Color32::from_rgb(0xf4, 0xf4, 0xf8);
        let muted = Color32::from_rgb(0xa1, 0xa1, 0xb5);

        let mut pending_migration: Option<(PathBuf, PathBuf, MigrationChoice)> = None;
        let mut cancel_migration: Option<(PathBuf, PathBuf)> = None;

        egui::CentralPanel::default()
            .frame(Frame::new().fill(Color32::BLACK).inner_margin(18.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(ref tex) = self.logo_texture {
                        ui.add(
                            egui::Image::new(tex)
                                .fit_to_exact_size(Vec2::splat(40.0))
                                .corner_radius(CornerRadius::same(10)),
                        );
                        ui.add_space(6.0);
                    }
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("MCW Launcher Updater")
                                .strong()
                                .size(17.0)
                                .color(text),
                        );
                        ui.label(
                            RichText::new(format!("Target version: v{}", self.args.version))
                                .monospace()
                                .size(11.5)
                                .color(accent),
                        );
                    });
                });

                ui.add_space(14.0);
                match &self.state {
                    UpdaterUiState::Running { fraction, status } => {
                        ui.label(RichText::new(status).size(12.5).color(muted));
                        ui.add_space(8.0);
                        ui.add(
                            egui::ProgressBar::new(*fraction)
                                .desired_height(8.0)
                                .animate(true)
                                .fill(accent),
                        );
                    }
                    UpdaterUiState::Done => {
                        ui.label(
                            RichText::new("Update complete! Launching MCW Launcher...")
                                .size(12.5)
                                .color(accent)
                                .strong(),
                        );
                        ui.add_space(8.0);
                        ui.add(egui::ProgressBar::new(1.0).desired_height(8.0).fill(accent));
                    }
                    UpdaterUiState::PermissionMigrationRequired {
                        error,
                        source_dir,
                        target_dir,
                        has_user_data,
                        shown_at,
                    } => {
                        let elapsed = shown_at.elapsed().as_secs();
                        let remaining = 5u64.saturating_sub(elapsed);
                        let unlocked = remaining == 0;
                        ScrollArea::vertical().show(ui, |ui| {
                            ui.label(
                                RichText::new("Write Permission Required (os error 5 / C: Drive)")
                                    .size(13.5)
                                    .strong()
                                    .color(warning),
                            );
                            ui.add_space(4.0);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(
                                        "Windows denied write permission while updating in the current folder (common when the launcher is placed directly on C:\\ or Program Files). Moving to a user-writable folder prevents update failures.",
                                    )
                                    .size(11.5)
                                    .color(text),
                                )
                                .wrap(),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(format!("Current folder: {}", source_dir.display()))
                                    .monospace()
                                    .size(11.0)
                                    .color(muted),
                            );
                            ui.label(
                                RichText::new(format!("New safe folder: {}", target_dir.display()))
                                    .monospace()
                                    .size(11.0)
                                    .color(accent),
                            );
                            ui.add_space(4.0);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("Detail: {error}"))
                                        .size(10.5)
                                        .color(muted),
                                )
                                .wrap(),
                            );
                            ui.add_space(10.0);

                            if *has_user_data {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(
                                            "Existing launcher data (instances, worlds/saves, accounts, configs) was detected in the current folder:",
                                        )
                                        .size(11.5)
                                        .color(warning),
                                    )
                                    .wrap(),
                                );
                                ui.add_space(8.0);
                                let suffix = if unlocked {
                                    String::new()
                                } else {
                                    format!(" ({remaining}s)")
                                };
                                ui.horizontal_wrapped(|ui| {
                                    let b1 = Button::new(
                                        RichText::new(format!(
                                            "Backup & Move (Recommended){suffix}"
                                        ))
                                        .color(Color32::WHITE)
                                        .strong(),
                                    )
                                    .fill(accent);
                                    if ui
                                        .add_enabled(unlocked, b1)
                                        .on_hover_text(
                                            "Creates a safety .zip backup of all instances & worlds first, then copies your data and installs the update.",
                                        )
                                        .clicked()
                                    {
                                        pending_migration = Some((
                                            source_dir.clone(),
                                            target_dir.clone(),
                                            MigrationChoice::BackupAndMove,
                                        ));
                                    }
                                    let b2 = Button::new(
                                        RichText::new(format!("Move Directly (Faster, risky){suffix}"))
                                            .color(warning),
                                    );
                                    if ui
                                        .add_enabled(unlocked, b2)
                                        .on_hover_text(
                                            "Copies files directly without creating a backup archive first (faster, higher risk if interrupted).",
                                        )
                                        .clicked()
                                    {
                                        pending_migration = Some((
                                            source_dir.clone(),
                                            target_dir.clone(),
                                            MigrationChoice::DirectMove,
                                        ));
                                    }
                                    if ui
                                        .add_enabled(
                                            unlocked,
                                            Button::new(format!("Cancel (Move Manually){suffix}")),
                                        )
                                        .clicked()
                                    {
                                        cancel_migration =
                                            Some((source_dir.clone(), target_dir.clone()));
                                    }
                                });
                            } else {
                                let ok_lbl = if unlocked {
                                    "OK — Install in New Safe Folder".to_string()
                                } else {
                                    format!("OK ({remaining}s)")
                                };
                                let ok_btn = Button::new(
                                    RichText::new(ok_lbl).color(Color32::WHITE).strong(),
                                )
                                .fill(accent);
                                if ui.add_enabled(unlocked, ok_btn).clicked() {
                                    pending_migration = Some((
                                        source_dir.clone(),
                                        target_dir.clone(),
                                        MigrationChoice::DirectInstallEmpty,
                                    ));
                                }
                            }
                        });
                    }
                    UpdaterUiState::ManualCancelled {
                        source_dir,
                        target_dir,
                    } => {
                        ui.label(
                            RichText::new("Automatic migration cancelled — no files were moved.")
                                .size(13.0)
                                .strong()
                                .color(warning),
                        );
                        ui.add_space(6.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    "Please move the launcher folder (and your instances/worlds) out of C:\\ manually, then run the update again.",
                                )
                                .size(11.5)
                                .color(text),
                            )
                            .wrap(),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(format!("Old folder: {}", source_dir.display()))
                                .monospace()
                                .size(11.0)
                                .color(muted),
                        );
                        ui.label(
                            RichText::new(format!("Suggested safe folder: {}", target_dir.display()))
                                .monospace()
                                .size(11.0)
                                .color(accent),
                        );
                        ui.add_space(12.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Open Old Folder").clicked() {
                                open_folder(source_dir);
                            }
                            if ui.button("Open New Folder").clicked() {
                                let _ = fs::create_dir_all(target_dir);
                                open_folder(target_dir);
                            }
                            if ui.button("Close").clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                    }
                    UpdaterUiState::Failed { status, error } => {
                        ui.label(RichText::new(status).size(13.0).strong().color(warning));
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(RichText::new(error).size(11.5).color(muted)).wrap(),
                        );
                        ui.add_space(12.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("Close").clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                            let launch_btn = Button::new(
                                RichText::new("Launch Previous Version")
                                    .color(Color32::WHITE)
                                    .strong(),
                            )
                            .fill(accent);
                            if ui.add(launch_btn).clicked() {
                                spawn_target_launcher(&self.args);
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                    }
                }
            });

        if let Some((src, dst, choice)) = pending_migration {
            self.state = UpdaterUiState::Running {
                fraction: 0.10,
                status: "Preparing migration to safe folder...".to_string(),
            };
            spawn_migration_worker(
                self.args.clone(),
                src,
                dst,
                choice,
                self.tx.clone(),
                ctx.clone(),
            );
        } else if let Some((src, dst)) = cancel_migration {
            self.state = UpdaterUiState::ManualCancelled {
                source_dir: src,
                target_dir: dst,
            };
        }
    }
}

fn load_icon_data() -> Option<egui::IconData> {
    let bytes = include_bytes!("../../../assets/icons/mcw_launcher.png");
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

fn load_logo_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let bytes = include_bytes!("../../../assets/icons/mcw_launcher.png");
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color_img = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture("mcw_updater_logo", color_img, egui::TextureOptions::LINEAR))
}

fn main() -> eframe::Result {
    let raw_args: Vec<String> = std::env::args().collect();
    let args = match parse_args_from(&raw_args) {
        Ok(a) => a,
        Err(err) => {
            eprintln!("mcw-updater error: {err}");
            std::process::exit(2);
        }
    };

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("MCW Launcher Updater")
        .with_inner_size([480.0, 230.0])
        .with_min_inner_size([420.0, 210.0])
        .with_resizable(true);
    if let Some(icon) = load_icon_data() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }

    let options = eframe::NativeOptions {
        centered: true,
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "MCW Launcher Updater",
        options,
        Box::new(move |cc| Ok(Box::new(UpdaterApp::new(cc, args)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let f = File::create(path).unwrap();
        let mut zw = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            zw.start_file(*name, opts).unwrap();
            zw.write_all(data).unwrap();
        }
        zw.finish().unwrap();
    }

    #[test]
    fn transactional_update_preserves_user_data_and_replaces_binary() {
        let dir = tempdir().unwrap();
        let install = dir.path().join("install");
        fs::create_dir_all(install.join("instances").join("MyWorld")).unwrap();
        fs::create_dir_all(install.join("accounts")).unwrap();
        fs::write(install.join("MCW-Launcher.exe"), b"old-exe-v0.15").unwrap();
        fs::write(
            install.join("instances").join("MyWorld").join("level.dat"),
            b"world",
        )
        .unwrap();
        fs::write(install.join("accounts").join("accounts.json"), b"{}").unwrap();

        let staged = dir.path().join("update.zip");
        create_test_zip(
            &staged,
            &[
                (
                    "mcw-launcher-windows-x64/MCW-Launcher.exe",
                    b"new-exe-v1.0.0-rc.1",
                ),
                (
                    "mcw-launcher-windows-x64/instances/MyWorld/level.dat",
                    b"OVERWRITTEN",
                ),
            ],
        );

        let args = UpdaterArgs {
            wait_pid: 0,
            staged,
            install_dir: install.clone(),
            target_exe: install.join("MCW-Launcher.exe"),
            version: "1.0.0-rc.1".to_string(),
        };
        apply_transactional_update(&args, false, |_, _| {}).unwrap();

        assert_eq!(
            fs::read(install.join("MCW-Launcher.exe")).unwrap(),
            b"new-exe-v1.0.0-rc.1"
        );
        assert_eq!(
            fs::read(install.join("instances").join("MyWorld").join("level.dat")).unwrap(),
            b"world"
        );
    }

    #[test]
    fn transactional_update_rolls_back_on_error_preserving_previous_binary() {
        let dir = tempdir().unwrap();
        let install = dir.path().join("install");
        fs::create_dir_all(&install).unwrap();
        fs::write(install.join("MCW-Launcher.exe"), b"good-previous-exe").unwrap();

        let staged = dir.path().join("bad-update.zip");
        create_test_zip(
            &staged,
            &[
                ("MCW-Launcher.exe", b"broken-new-exe"),
                ("extra-new-file.dll", b"temp-dll"),
            ],
        );

        let args = UpdaterArgs {
            wait_pid: 0,
            staged,
            install_dir: install.clone(),
            target_exe: install.join("MCW-Launcher.exe"),
            version: "1.0.0-rc.1".to_string(),
        };
        let err = apply_transactional_update(&args, true, |_, _| {}).unwrap_err();
        assert!(err.contains("Safely rolled back"));
        assert_eq!(
            fs::read(install.join("MCW-Launcher.exe")).unwrap(),
            b"good-previous-exe"
        );
        assert!(!install.join("extra-new-file.dll").exists());
    }

    #[test]
    fn backup_and_move_preserves_worlds_and_detects_permission_errors() {
        assert!(is_permission_denied_err("Access is denied. (os error 5)"));
        assert!(is_permission_denied_err("Permission denied"));
        assert!(!is_permission_denied_err("Unexpected EOF"));

        let dir = tempdir().unwrap();
        let src = dir.path().join("old_c_drive");
        let dst = dir.path().join("safe_appdata");
        fs::create_dir_all(src.join("instances/Survival/saves/World1")).unwrap();
        fs::write(
            src.join("instances/Survival/saves/World1/level.dat"),
            b"world-nbt",
        )
        .unwrap();
        assert!(dir_has_user_data(&src));

        let backup_zip = create_user_data_backup_zip(&src, &dst).unwrap();
        assert!(backup_zip.is_file());
        copy_user_data_to_target(&src, &dst).unwrap();
        assert_eq!(
            fs::read(dst.join("instances/Survival/saves/World1/level.dat")).unwrap(),
            b"world-nbt"
        );
    }
}
