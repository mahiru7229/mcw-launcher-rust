use crate::Result;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use tracing::info;

#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub java_executable: PathBuf,
    pub working_dir: PathBuf,
    pub jvm_args: Vec<String>,
    pub main_class: String,
    pub game_args: Vec<String>,
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
        let child = Command::new(&spec.java_executable)
            .current_dir(&spec.working_dir)
            .args(compacted_args)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(child)
    }
}
