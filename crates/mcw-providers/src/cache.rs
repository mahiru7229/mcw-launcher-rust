use crate::Result;
use rusqlite::{params, Connection};
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// SQLite WAL-mode API response cache.
/// Lesson #5: Never fsync on cache reads; WAL + NORMAL sync keeps UI/API snappy.
pub struct ApiCache {
    conn: Mutex<Connection>,
}

impl ApiCache {
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA temp_store = MEMORY;
             CREATE TABLE IF NOT EXISTS api_cache (
                 cache_key TEXT PRIMARY KEY,
                 payload   TEXT NOT NULL,
                 expires_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_api_cache_expires ON api_cache(expires_at);",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let now = now_epoch_secs() as i64;
        let guard = self.conn.lock().ok()?;
        let mut stmt = guard
            .prepare_cached(
                "SELECT payload FROM api_cache WHERE cache_key = ?1 AND expires_at > ?2",
            )
            .ok()?;
        stmt.query_row(params![key, now], |row| row.get::<_, String>(0))
            .ok()
    }

    pub fn set(&self, key: &str, payload: &str, ttl_secs: u64) -> Result<()> {
        let expires_at = (now_epoch_secs().saturating_add(ttl_secs)) as i64;
        if let Ok(guard) = self.conn.lock() {
            guard.execute(
                "INSERT OR REPLACE INTO api_cache (cache_key, payload, expires_at) VALUES (?1, ?2, ?3)",
                params![key, payload, expires_at],
            )?;
        }
        Ok(())
    }

    pub fn prune_expired(&self) -> Result<usize> {
        let now = now_epoch_secs() as i64;
        if let Ok(guard) = self.conn.lock() {
            let removed = guard.execute(
                "DELETE FROM api_cache WHERE expires_at <= ?1",
                params![now],
            )?;
            return Ok(removed);
        }
        Ok(0)
    }

    pub fn clear(&self) -> Result<()> {
        if let Ok(guard) = self.conn.lock() {
            guard.execute("DELETE FROM api_cache", [])?;
        }
        Ok(())
    }
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn wal_cache_stores_reads_and_expires() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("provider_cache.sqlite");
        let cache = ApiCache::open(&db).unwrap();

        cache.set("k1", r#"{"ok":true}"#, 60).unwrap();
        assert_eq!(cache.get("k1").as_deref(), Some(r#"{"ok":true}"#));

        cache.set("expired", "old", 0).unwrap();
        assert_eq!(cache.get("expired"), None);
        assert!(cache.prune_expired().unwrap() >= 1);
    }
}
