use crate::launcher::LaunchSpec;
use crate::Result;
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tracing::info;

const LOG_TAIL_CAPACITY: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone)]
pub struct LogLine {
    pub stream: LogStream,
    pub text: String,
}

pub trait LogSink: Send + Sync {
    fn on_log(&self, line: LogLine);
}

#[derive(Debug, Default)]
pub struct NoopLogSink;

impl LogSink for NoopLogSink {
    fn on_log(&self, _line: LogLine) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitClassification {
    Normal,
    StoppedByUser,
    Crashed,
}

#[derive(Debug, Clone)]
pub struct ProcessExitReport {
    pub pid: u32,
    pub exit_code: Option<i32>,
    pub classification: ExitClassification,
    pub duration_secs: u64,
    pub unsupported_class_version: bool,
    pub tail_logs: Vec<String>,
}

#[cfg(windows)]
struct Win32JobGuard {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
unsafe impl Send for Win32JobGuard {}
#[cfg(windows)]
unsafe impl Sync for Win32JobGuard {}

#[cfg(windows)]
impl Win32JobGuard {
    fn attach_pid(pid: u32) -> Option<Self> {
        use std::ptr;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
        };

        unsafe {
            let job = CreateJobObjectW(ptr::null(), ptr::null());
            if job.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                CloseHandle(job);
                return None;
            }

            let proc_handle = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if proc_handle.is_null() {
                CloseHandle(job);
                return None;
            }
            let assigned = AssignProcessToJobObject(job, proc_handle);
            CloseHandle(proc_handle);
            if assigned == 0 {
                CloseHandle(job);
                return None;
            }
            Some(Self { handle: job })
        }
    }

    fn terminate_tree(&self) {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        unsafe {
            let _ = TerminateJobObject(self.handle, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for Win32JobGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

/// Thread-safe handle to request graceful/forced stop from another task (e.g. GUI Stop button or Ctrl+C).
#[derive(Clone)]
pub struct ProcessControl {
    stop_requested: Arc<AtomicBool>,
    #[cfg(windows)]
    job: Arc<Mutex<Option<Win32JobGuard>>>,
}

impl ProcessControl {
    /// Mark stop as user-initiated (prevents false Crash reporting) and terminate the process tree.
    pub fn request_stop(&self) {
        self.stop_requested.store(true, Ordering::SeqCst);
        #[cfg(windows)]
        {
            if let Ok(guard) = self.job.lock() {
                if let Some(ref job) = *guard {
                    job.terminate_tree();
                }
            }
        }
    }

    pub fn is_stop_requested(&self) -> bool {
        self.stop_requested.load(Ordering::SeqCst)
    }
}

pub struct SupervisedProcess {
    pid: u32,
    child: Child,
    started_at: Instant,
    control: ProcessControl,
    tail: Arc<Mutex<VecDeque<String>>>,
    stdout_task: Option<tokio::task::JoinHandle<()>>,
    stderr_task: Option<tokio::task::JoinHandle<()>>,
}

impl SupervisedProcess {
    pub fn spawn(
        spec: &LaunchSpec,
        log_file_path: Option<&Path>,
        sink: Arc<dyn LogSink>,
    ) -> Result<Self> {
        info!(
            java = %spec.java_executable.display(),
            cwd = %spec.working_dir.display(),
            main_class = %spec.main_class,
            "Spawning supervised Minecraft process"
        );

        let mut cmd = Command::new(&spec.java_executable);
        cmd.current_dir(&spec.working_dir)
            .args(spec.full_command_args())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let pid = child.id().unwrap_or(0);

        #[cfg(windows)]
        let job_guard = if pid > 0 {
            Win32JobGuard::attach_pid(pid)
        } else {
            None
        };

        let log_writer: Option<Arc<Mutex<BufWriter<File>>>> = log_file_path.and_then(|p| {
            if let Some(parent) = p.parent() {
                let _ = fs::create_dir_all(parent);
            }
            File::create(p)
                .ok()
                .map(|f| Arc::new(Mutex::new(BufWriter::new(f))))
        });

        let tail = Arc::new(Mutex::new(VecDeque::with_capacity(LOG_TAIL_CAPACITY)));

        let stdout_task = child.stdout.take().map(|stdout| {
            let sink = Arc::clone(&sink);
            let tail = Arc::clone(&tail);
            let writer = log_writer.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    record_log_line(&tail, &writer, &line);
                    sink.on_log(LogLine {
                        stream: LogStream::Stdout,
                        text: line,
                    });
                }
            })
        });

        let stderr_task = child.stderr.take().map(|stderr| {
            let sink = Arc::clone(&sink);
            let tail = Arc::clone(&tail);
            let writer = log_writer.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    record_log_line(&tail, &writer, &line);
                    sink.on_log(LogLine {
                        stream: LogStream::Stderr,
                        text: line,
                    });
                }
            })
        });

        let control = ProcessControl {
            stop_requested: Arc::new(AtomicBool::new(false)),
            #[cfg(windows)]
            job: Arc::new(Mutex::new(job_guard)),
        };

        Ok(Self {
            pid,
            child,
            started_at: Instant::now(),
            control,
            tail,
            stdout_task,
            stderr_task,
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn control(&self) -> ProcessControl {
        self.control.clone()
    }

    pub async fn wait(mut self) -> Result<ProcessExitReport> {
        let status = self.child.wait().await?;
        let duration_secs = self.started_at.elapsed().as_secs();

        if let Some(t) = self.stdout_task.take() {
            let _ = t.await;
        }
        if let Some(t) = self.stderr_task.take() {
            let _ = t.await;
        }

        let exit_code = status.code();
        let classification = if self.control.is_stop_requested() {
            ExitClassification::StoppedByUser
        } else if exit_code == Some(0) {
            ExitClassification::Normal
        } else {
            ExitClassification::Crashed
        };

        let tail_logs: Vec<String> = self
            .tail
            .lock()
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default();
        let unsupported_class_version = detect_unsupported_class_version(&tail_logs);

        Ok(ProcessExitReport {
            pid: self.pid,
            exit_code,
            classification,
            duration_secs,
            unsupported_class_version,
            tail_logs,
        })
    }
}

fn record_log_line(
    tail: &Arc<Mutex<VecDeque<String>>>,
    writer: &Option<Arc<Mutex<BufWriter<File>>>>,
    line: &str,
) {
    if let Ok(mut q) = tail.lock() {
        if q.len() >= LOG_TAIL_CAPACITY {
            q.pop_front();
        }
        q.push_back(line.to_string());
    }
    if let Some(ref w) = writer {
        if let Ok(mut bw) = w.lock() {
            let _ = writeln!(bw, "{line}");
            let _ = bw.flush();
        }
    }
}

pub fn detect_unsupported_class_version(lines: &[String]) -> bool {
    lines.iter().any(|l| {
        l.contains("UnsupportedClassVersionError")
            || l.contains("has been compiled by a more recent version of the Java Runtime")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    struct CollectingSink(Arc<Mutex<Vec<String>>>);
    impl LogSink for CollectingSink {
        fn on_log(&self, line: LogLine) {
            self.0.lock().unwrap().push(line.text);
        }
    }

    #[tokio::test]
    async fn supervises_process_and_streams_live_logs() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("latest.log");
        let captured = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::new(CollectingSink(Arc::clone(&captured)));

        let spec = if cfg!(windows) {
            LaunchSpec {
                java_executable: PathBuf::from("cmd"),
                working_dir: dir.path().to_path_buf(),
                jvm_args: vec!["/C".to_string()],
                main_class: "echo".to_string(),
                game_args: vec!["mcw-supervisor-ok".to_string()],
            }
        } else {
            LaunchSpec {
                java_executable: PathBuf::from("sh"),
                working_dir: dir.path().to_path_buf(),
                jvm_args: vec!["-c".to_string()],
                main_class: "echo mcw-supervisor-ok".to_string(),
                game_args: vec![],
            }
        };

        let proc = SupervisedProcess::spawn(&spec, Some(&log_path), sink).unwrap();
        let report = proc.wait().await.unwrap();

        assert_eq!(report.classification, ExitClassification::Normal);
        assert_eq!(report.exit_code, Some(0));
        assert!(captured
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.contains("mcw-supervisor-ok")));
        assert!(fs::read_to_string(&log_path)
            .unwrap()
            .contains("mcw-supervisor-ok"));
    }

    #[test]
    fn detects_unsupported_class_version_error() {
        let logs = vec![
            "Exception in thread \"main\" java.lang.UnsupportedClassVersionError: ...".to_string(),
        ];
        assert!(detect_unsupported_class_version(&logs));
    }
}
