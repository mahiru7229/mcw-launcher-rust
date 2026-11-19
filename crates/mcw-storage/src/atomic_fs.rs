use crate::{Result, StorageError};
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::thread;
use std::time::Duration;
use tempfile::NamedTempFile;

const WINDOWS_RENAME_RETRIES: usize = 6;
const WINDOWS_RETRY_DELAY_MS: u64 = 25;

/// Atomically write bytes to `path` using a temporary file in the same directory
/// followed by rename, with short retries for Windows `PermissionDenied` locks.
pub fn atomic_write_bytes(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| StorageError::Io {
        path: parent.display().to_string(),
        source,
    })?;

    let mut temp = NamedTempFile::new_in(parent).map_err(|source| StorageError::Io {
        path: parent.display().to_string(),
        source,
    })?;

    temp.write_all(data).map_err(|source| StorageError::Io {
        path: path.display().to_string(),
        source,
    })?;
    temp.as_file()
        .sync_all()
        .map_err(|source| StorageError::Io {
            path: path.display().to_string(),
            source,
        })?;

    let mut temp_path = temp.into_temp_path();
    for attempt in 0..=WINDOWS_RENAME_RETRIES {
        match temp_path.persist(path) {
            Ok(_) => return Ok(()),
            Err(err) => {
                let kind = err.error.kind();
                let is_retryable = matches!(
                    kind,
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::AlreadyExists
                );
                if is_retryable && attempt < WINDOWS_RENAME_RETRIES {
                    // On Windows, antivirus or concurrent readers can briefly hold a handle.
                    temp_path = err.path;
                    thread::sleep(Duration::from_millis(
                        WINDOWS_RETRY_DELAY_MS * (attempt as u64 + 1),
                    ));
                    continue;
                }
                return Err(StorageError::Io {
                    path: path.display().to_string(),
                    source: err.error,
                });
            }
        }
    }

    unreachable!()
}

pub fn atomic_write_text(path: &Path, content: &str) -> Result<()> {
    atomic_write_bytes(path, content.as_bytes())
}

pub fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let serialized = serde_json::to_string_pretty(value).map_err(|source| StorageError::Json {
        path: path.display().to_string(),
        source,
    })?;
    atomic_write_text(path, &serialized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn atomic_write_creates_and_overwrites_file() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("nested").join("config.json");

        atomic_write_text(&target, r#"{"a":1}"#).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), r#"{"a":1}"#);

        atomic_write_text(&target, r#"{"a":2}"#).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), r#"{"a":2}"#);
    }
}
