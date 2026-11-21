use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadJournalEntry {
    pub id: String,
    pub url: String,
    pub destination: String,
    #[serde(default)]
    pub expected_sha1: Option<String>,
    #[serde(default)]
    pub expected_size: u64,
    #[serde(default)]
    pub completed: bool,
}

/// Persistent download journal (`cache/download_journal.json`) tracking pending and completed
/// artifact transfers across sessions, mirroring Python `core/network/download_journal.py`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DownloadJournal {
    #[serde(default)]
    pub entries: Vec<DownloadJournalEntry>,
}

impl DownloadJournal {
    pub fn file_path(cache_dir: &Path) -> PathBuf {
        cache_dir.join("download_journal.json")
    }

    pub fn load(cache_dir: &Path) -> Self {
        let path = Self::file_path(cache_dir);
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, cache_dir: &Path) -> std::io::Result<()> {
        let path = Self::file_path(cache_dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        let part = path.with_extension(format!("json.{}.part", std::process::id()));
        fs::write(&part, format!("{json}\n"))?;
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
        fs::rename(&part, &path)?;
        Ok(())
    }

    pub fn upsert_pending(&mut self, mut entry: DownloadJournalEntry) {
        entry.completed = false;
        if let Some(existing) = self.entries.iter_mut().find(|e| e.id == entry.id) {
            *existing = entry;
        } else {
            self.entries.push(entry);
        }
    }

    pub fn mark_completed(&mut self, id: &str) {
        if let Some(existing) = self.entries.iter_mut().find(|e| e.id == id) {
            existing.completed = true;
        }
    }

    pub fn pending_entries(&self) -> Vec<&DownloadJournalEntry> {
        self.entries.iter().filter(|e| !e.completed).collect()
    }

    pub fn prune_completed(&mut self) {
        self.entries.retain(|e| !e.completed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn roundtrips_and_prunes_journal_entries() {
        let dir = tempdir().unwrap();
        let mut journal = DownloadJournal::default();
        journal.upsert_pending(DownloadJournalEntry {
            id: "a".to_string(),
            url: "https://example.invalid/a.jar".to_string(),
            destination: "mods/a.jar".to_string(),
            expected_sha1: Some("deadbeef".to_string()),
            expected_size: 123,
            completed: false,
        });
        journal.upsert_pending(DownloadJournalEntry {
            id: "b".to_string(),
            url: "https://example.invalid/b.jar".to_string(),
            destination: "mods/b.jar".to_string(),
            expected_sha1: None,
            expected_size: 456,
            completed: false,
        });
        journal.mark_completed("a");
        journal.save(dir.path()).unwrap();

        let mut loaded = DownloadJournal::load(dir.path());
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(loaded.pending_entries().len(), 1);
        assert_eq!(loaded.pending_entries()[0].id, "b");

        loaded.prune_completed();
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].id, "b");
    }
}
