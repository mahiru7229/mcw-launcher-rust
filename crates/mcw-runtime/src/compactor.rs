/// Direct 1:1 port of `src/core/java/java_command_compactor.py` from MCW Launcher Python.
/// Compacts long `-cp` classpaths into a manifest-only `.mcw/launch/classpath-<hash>.jar`
/// when approaching Windows' 32,767-char `CreateProcess` command-line limit (`WinError 206`).
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use url::Url;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub const WINDOWS_COMMAND_LIMIT: usize = 32_767;
pub const SAFE_WINDOWS_COMMAND_LIMIT: usize = 30_000;
const CLASSPATH_FLAGS: &[&str] = &["-cp", "-classpath", "--class-path"];

pub struct JavaCommandCompactor;

impl JavaCommandCompactor {
    pub fn prepare(
        java: &Path,
        command: &[String],
        instance_dir: &Path,
        force: bool,
    ) -> Vec<String> {
        let is_windows = cfg!(target_os = "windows");
        if !force
            && (!is_windows || Self::command_length(java, command) < SAFE_WINDOWS_COMMAND_LIMIT)
        {
            return command.to_vec();
        }

        let Some(cp_index) = Self::classpath_index(command) else {
            return command.to_vec();
        };
        if cp_index + 1 >= command.len() {
            return command.to_vec();
        }
        let classpath = &command[cp_index + 1];
        if classpath.trim().is_empty() {
            return command.to_vec();
        }

        match Self::write_classpath_jar(classpath, instance_dir) {
            Ok(jar_path) => {
                let mut compacted = command.to_vec();
                compacted[cp_index + 1] = jar_path.display().to_string();
                compacted
            }
            Err(_) => command.to_vec(),
        }
    }

    fn command_length(java: &Path, command: &[String]) -> usize {
        let mut total = java.as_os_str().len() + 3;
        for arg in command {
            total += arg.len() + 3;
        }
        total
    }

    fn classpath_index(command: &[String]) -> Option<usize> {
        command
            .iter()
            .position(|arg| CLASSPATH_FLAGS.contains(&arg.as_str()))
    }

    fn write_classpath_jar(classpath: &str, instance_dir: &Path) -> std::io::Result<PathBuf> {
        let sep = if cfg!(target_os = "windows") {
            ';'
        } else {
            ':'
        };
        let entries: Vec<&str> = classpath.split(sep).filter(|s| !s.is_empty()).collect();
        if entries.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Minecraft classpath is empty and cannot be compacted",
            ));
        }

        let normalized: Vec<String> = entries
            .into_iter()
            .map(|e| Self::classpath_uri(e, instance_dir))
            .collect();

        let mut hasher = Sha256::new();
        hasher.update(normalized.join("\n").as_bytes());
        let hex_full = format!("{:x}", hasher.finalize());
        let digest = &hex_full[..16];

        let launch_dir = instance_dir.join(".mcw").join("launch");
        fs::create_dir_all(&launch_dir)?;
        let target = launch_dir.join(format!("classpath-{digest}.jar"));
        if target.is_file() && target.metadata().map(|m| m.len() > 0).unwrap_or(false) {
            return Ok(target);
        }

        let temporary = launch_dir.join(format!("classpath-{digest}.jar.tmp"));
        let manifest = Self::manifest_bytes(&normalized);
        {
            let file = File::create(&temporary)?;
            let mut zip = ZipWriter::new(file);
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zip.start_file("META-INF/MANIFEST.MF", opts)
                .map_err(std::io::Error::other)?;
            zip.write_all(&manifest)?;
            zip.finish().map_err(std::io::Error::other)?;
        }
        fs::rename(&temporary, &target)?;
        Ok(target)
    }

    fn classpath_uri(value: &str, instance_dir: &Path) -> String {
        let raw = Path::new(value);
        let abs = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            instance_dir.join(raw)
        };
        Url::from_file_path(&abs)
            .map(|u| u.to_string())
            .unwrap_or_else(|_| abs.display().to_string().replace('\\', "/"))
    }

    fn manifest_bytes(entries: &[String]) -> Vec<u8> {
        let logical_lines = [
            "Manifest-Version: 1.0".to_string(),
            "Created-By: MCW Launcher".to_string(),
            format!("Class-Path: {}", entries.join(" ")),
        ];
        let mut physical_lines = Vec::new();
        for line in &logical_lines {
            physical_lines.extend(Self::wrap_manifest_line(line));
        }
        format!("{}\r\n\r\n", physical_lines.join("\r\n")).into_bytes()
    }

    fn wrap_manifest_line(value: &str) -> Vec<String> {
        let bytes = value.as_bytes();
        if bytes.len() <= 70 {
            return vec![value.to_string()];
        }
        let mut lines = Vec::new();
        let mut remaining = bytes;
        let mut first = true;
        while !remaining.is_empty() {
            let limit = if first { 70 } else { 69 };
            let take = remaining.len().min(limit);
            let chunk = String::from_utf8_lossy(&remaining[..take]);
            remaining = &remaining[take..];
            if first {
                lines.push(chunk.into_owned());
                first = false;
            } else {
                lines.push(format!(" {chunk}"));
            }
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compacts_classpath_into_valid_manifest_jar_when_forced() {
        let dir = std::env::temp_dir().join(format!("mcw_compactor_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let jar_a = dir.join("a.jar");
        let jar_b = dir.join("b.jar");
        let sep = if cfg!(target_os = "windows") {
            ";"
        } else {
            ":"
        };
        let cp = format!("{}{sep}{}", jar_a.display(), jar_b.display());
        let cmd = vec![
            "-Xmx2048M".to_string(),
            "-cp".to_string(),
            cp.clone(),
            "net.minecraft.client.main.Main".to_string(),
        ];

        let compacted = JavaCommandCompactor::prepare(Path::new("javaw.exe"), &cmd, &dir, true);
        assert_ne!(compacted[2], cp);
        assert!(compacted[2].ends_with(".jar"));
        assert!(Path::new(&compacted[2]).is_file());

        let _ = fs::remove_dir_all(&dir);
    }
}
