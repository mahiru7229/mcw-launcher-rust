use crate::Result;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use tracing::{info, warn};

#[derive(Debug, Clone, Default)]
pub struct LaunchSpec {
    pub java_executable: PathBuf,
    pub working_dir: PathBuf,
    pub jvm_args: Vec<String>,
    pub main_class: String,
    pub game_args: Vec<String>,
    pub wrapper_command: Option<String>,
    pub env_vars: Vec<(String, String)>,
    pub enable_gamemode: bool,
    pub enable_mangohud: bool,
}

impl LaunchSpec {
    pub fn full_command_args(&self) -> Vec<String> {
        let mut args = Vec::with_capacity(self.jvm_args.len() + 1 + self.game_args.len());
        args.extend(self.jvm_args.iter().cloned());
        args.push(self.main_class.clone());
        args.extend(self.game_args.iter().cloned());
        args
    }
}

/// Split a shell command string into arguments while preserving `'single'` and `"double"` quotes
/// and backslash escapes (outside single quotes).
pub fn split_shell_words(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut has_token = false;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_single {
            if ch == '\'' {
                in_single = false;
            } else {
                current.push(ch);
            }
            has_token = true;
            continue;
        }
        if in_double {
            if ch == '"' {
                in_double = false;
            } else if ch == '\\' {
                if let Some(&next) = chars.peek() {
                    if next == '"' || next == '\\' {
                        current.push(chars.next().unwrap());
                    } else {
                        current.push(ch);
                    }
                } else {
                    current.push(ch);
                }
            } else {
                current.push(ch);
            }
            has_token = true;
            continue;
        }
        match ch {
            '\'' => {
                in_single = true;
                has_token = true;
            }
            '"' => {
                in_double = true;
                has_token = true;
            }
            '\\' => {
                if let Some(next) = chars.next() {
                    current.push(next);
                    has_token = true;
                }
            }
            c if c.is_whitespace() => {
                if has_token {
                    words.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            c => {
                current.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        words.push(current);
    }
    words
}

fn wrap_program_prefix(program: &mut PathBuf, args: &mut Vec<String>, prefix_bin: &str) {
    let prev = std::mem::replace(program, PathBuf::from(prefix_bin));
    let mut next_args = Vec::with_capacity(args.len() + 1);
    next_args.push(prev.to_string_lossy().into_owned());
    next_args.append(args);
    *args = next_args;
}

/// Resolve final `(program, args, env_vars)` applying `wrapper_command`, optional Linux
/// `mangohud` / `gamemoderun` wrappers, and custom environment variables.
pub fn prepare_invocation_with_os(
    spec: &LaunchSpec,
    resolved_java: &Path,
    compacted_args: Vec<String>,
    is_linux: bool,
) -> (PathBuf, Vec<String>, Vec<(String, String)>) {
    let mut program = resolved_java.to_path_buf();
    let mut args = compacted_args;

    if let Some(wrapper) = spec
        .wrapper_command
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let tokens = split_shell_words(wrapper);
        if let Some(first) = tokens.first() {
            let mut wrapped_args = tokens[1..].to_vec();
            wrapped_args.push(program.to_string_lossy().into_owned());
            wrapped_args.extend(args);
            program = PathBuf::from(first);
            args = wrapped_args;
        }
    }

    if is_linux {
        let already_has = |bin: &str, prog: &Path, argv: &[String]| {
            prog.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n == bin)
                .unwrap_or(false)
                || argv.iter().any(|a| a == bin)
        };
        if spec.enable_mangohud && !already_has("mangohud", &program, &args) {
            wrap_program_prefix(&mut program, &mut args, "mangohud");
        }
        if spec.enable_gamemode && !already_has("gamemoderun", &program, &args) {
            wrap_program_prefix(&mut program, &mut args, "gamemoderun");
        }
    }

    let env_vars = spec
        .env_vars
        .iter()
        .filter(|(k, _)| !k.trim().is_empty())
        .map(|(k, v)| (k.trim().to_string(), v.clone()))
        .collect();

    (program, args, env_vars)
}

/// Host-OS convenience wrapper around [`prepare_invocation_with_os`].
pub fn prepare_command_invocation(
    spec: &LaunchSpec,
    resolved_java: &Path,
    compacted_args: Vec<String>,
) -> (PathBuf, Vec<String>, Vec<(String, String)>) {
    prepare_invocation_with_os(
        spec,
        resolved_java,
        compacted_args,
        cfg!(target_os = "linux"),
    )
}

/// Run a per-instance pre-launch or post-exit lifecycle hook command in `working_dir` with `env_vars`.
pub fn run_lifecycle_hook(
    command: &str,
    working_dir: &Path,
    env_vars: &[(String, String)],
    hook_name: &str,
) -> std::result::Result<Option<i32>, String> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    info!(hook = hook_name, cwd = %working_dir.display(), cmd = %trimmed, "Running lifecycle hook command");

    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut c = Command::new("cmd");
        c.args(["/C", trimmed]).creation_flags(CREATE_NO_WINDOW);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("sh");
        c.args(["-c", trimmed]);
        c
    };

    cmd.current_dir(working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    for (k, v) in env_vars {
        let key = k.trim();
        if !key.is_empty() {
            cmd.env(key, v);
        }
    }

    let output = cmd
        .output()
        .map_err(|e| format!("{hook_name} command failed to start: {e}"))?;
    let code = output.status.code().unwrap_or(-1);
    if output.status.success() {
        Ok(Some(code))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            format!("{hook_name} command exited with code {code}")
        } else {
            format!("{hook_name} command exited with code {code}: {stderr}")
        };
        warn!(hook = hook_name, code, "{detail}");
        Err(detail)
    }
}

pub struct ProcessLauncher;

impl ProcessLauncher {
    pub fn spawn_inherited(spec: &LaunchSpec) -> Result<Child> {
        info!(
            java = %spec.java_executable.display(),
            cwd = %spec.working_dir.display(),
            main_class = %spec.main_class,
            "Spawning Minecraft process"
        );
        let instance_dir = spec.working_dir.parent().unwrap_or(&spec.working_dir);
        let raw_args = spec.full_command_args();
        let compacted_args = crate::compactor::JavaCommandCompactor::prepare(
            &spec.java_executable,
            &raw_args,
            instance_dir,
            false,
        );
        let (program, final_args, env_vars) =
            prepare_command_invocation(spec, &spec.java_executable, compacted_args);
        let mut cmd = Command::new(&program);
        cmd.current_dir(&spec.working_dir)
            .args(final_args)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        for (k, v) in env_vars {
            cmd.env(k, v);
        }
        let child = cmd.spawn()?;
        Ok(child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn splits_shell_words_with_quotes_and_escapes() {
        assert_eq!(
            split_shell_words(r#""/opt/my wrapper.sh" --flag 'hello world' plain\ arg"#),
            vec!["/opt/my wrapper.sh", "--flag", "hello world", "plain arg"]
        );
    }

    #[test]
    fn prepares_wrapper_linux_prefixes_and_env_vars() {
        let spec = LaunchSpec {
            wrapper_command: Some("env FOO=bar".to_string()),
            env_vars: vec![
                ("INST_NAME".to_string(), "TestPack".to_string()),
                ("   ".to_string(), "ignored".to_string()),
            ],
            enable_gamemode: true,
            enable_mangohud: true,
            ..Default::default()
        };
        let (prog, args, env) = prepare_invocation_with_os(
            &spec,
            Path::new("/usr/bin/java"),
            vec!["-Xmx2G".to_string(), "net.minecraft.Main".to_string()],
            true,
        );
        assert_eq!(prog, PathBuf::from("gamemoderun"));
        assert_eq!(
            args,
            vec![
                "mangohud",
                "env",
                "FOO=bar",
                "/usr/bin/java",
                "-Xmx2G",
                "net.minecraft.Main"
            ]
        );
        assert_eq!(env, vec![("INST_NAME".to_string(), "TestPack".to_string())]);
    }

    #[test]
    fn executes_lifecycle_hook_command() {
        let dir = tempdir().unwrap();
        assert_eq!(
            run_lifecycle_hook("   ", dir.path(), &[], "pre-launch").unwrap(),
            None
        );
        let code = run_lifecycle_hook(
            "echo hook-ok",
            dir.path(),
            &[("MCW_TEST_VAR".to_string(), "123".to_string())],
            "pre-launch",
        )
        .unwrap();
        assert_eq!(code, Some(0));
    }
}
