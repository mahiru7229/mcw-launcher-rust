use crate::MinecraftError;
use flate2::read::GzDecoder;
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct WorldInfo {
    pub folder_name: String,
    pub display_name: String,
    pub game_mode: String,
    pub is_hardcore: bool,
    pub last_played_ms: u64,
    pub version_name: String,
    pub size_bytes: u64,
    pub folder_path: PathBuf,
}

/// Returns `true` if the Minecraft version supports Java Edition Quick Play (`>= 1.20` or snapshot `>= 23w14a`).
pub fn is_quick_play_supported(version_str: &str) -> bool {
    let v = version_str.trim();
    if v.is_empty() {
        return false;
    }
    if let Ok(re) = Regex::new(r"^(\d+)\.(\d+)(?:\.(\d+))?") {
        if let Some(caps) = re.captures(v) {
            let major: u32 = caps[1].parse().unwrap_or(0);
            let minor: u32 = caps[2].parse().unwrap_or(0);
            return major > 1 || (major == 1 && minor >= 20);
        }
    }
    if let Ok(re) = Regex::new(r"^(\d{2})w(\d{2})[a-z]$") {
        if let Some(caps) = re.captures(v) {
            let year: u32 = caps[1].parse().unwrap_or(0);
            let week: u32 = caps[2].parse().unwrap_or(0);
            return year > 23 || (year == 23 && week >= 14);
        }
    }
    false
}

pub struct WorldManager;

impl WorldManager {
    pub fn list_worlds(mc_dir: &Path) -> Vec<WorldInfo> {
        let saves_dir = mc_dir.join("saves");
        let Ok(entries) = fs::read_dir(&saves_dir) else {
            return Vec::new();
        };

        let mut worlds = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let folder_name = entry.file_name().to_string_lossy().to_string();
            if folder_name.starts_with('.') || folder_name.starts_with('_') {
                continue;
            }
            worlds.push(Self::read_world(&path, &folder_name));
        }

        worlds.sort_by_key(|a| std::cmp::Reverse(a.last_played_ms));
        worlds
    }

    pub fn read_world(folder: &Path, folder_name: &str) -> WorldInfo {
        let level_dat = folder.join("level.dat");
        let mut mtime_ms = folder
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let nbt = if level_dat.is_file() {
            if let Ok(m) = level_dat.metadata().and_then(|m| m.modified()) {
                if let Ok(d) = m.duration_since(UNIX_EPOCH) {
                    mtime_ms = d.as_millis() as u64;
                }
            }
            parse_level_dat(&level_dat).unwrap_or_default()
        } else {
            NbtLevelSummary::default()
        };

        let display_name = if nbt.level_name.is_empty() {
            folder_name.to_string()
        } else {
            nbt.level_name
        };

        let game_mode = if nbt.hardcore {
            "hardcore".to_string()
        } else {
            match nbt.game_type {
                1 => "creative",
                2 => "adventure",
                3 => "spectator",
                _ => "survival",
            }
            .to_string()
        };

        let last_played_ms = if nbt.last_played > 0 {
            nbt.last_played as u64
        } else {
            mtime_ms
        };

        let size_bytes: u64 = WalkDir::new(folder)
            .into_iter()
            .flatten()
            .filter_map(|e| e.metadata().ok())
            .filter(|m| m.is_file())
            .map(|m| m.len())
            .sum();

        WorldInfo {
            folder_name: folder_name.to_string(),
            display_name,
            game_mode,
            is_hardcore: nbt.hardcore,
            last_played_ms,
            version_name: nbt.version_name,
            size_bytes,
            folder_path: folder.to_path_buf(),
        }
    }

    pub fn duplicate_world(mc_dir: &Path, folder_name: &str) -> crate::Result<WorldInfo> {
        if folder_name.contains('/') || folder_name.contains('\\') || folder_name.contains("..") {
            return Err(MinecraftError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid world folder name",
            )));
        }
        let saves_dir = mc_dir.join("saves");
        let src = saves_dir.join(folder_name);
        if !src.is_dir() {
            return Err(MinecraftError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("World folder '{}' not found", folder_name),
            )));
        }

        let mut copy_name = format!("{folder_name} (Copy)");
        let mut idx = 2usize;
        while saves_dir.join(&copy_name).exists() {
            copy_name = format!("{folder_name} (Copy {idx})");
            idx += 1;
        }
        let dst = saves_dir.join(&copy_name);

        for entry in WalkDir::new(&src).into_iter().flatten() {
            let rel = match entry.path().strip_prefix(&src) {
                Ok(r) => r,
                Err(_) => continue,
            };
            // Never copy session.lock so the duplicated world isn't marked locked
            if rel.file_name().and_then(|n| n.to_str()) == Some("session.lock") {
                continue;
            }
            let target = dst.join(rel);
            if entry.file_type().is_dir() {
                fs::create_dir_all(&target)?;
            } else if entry.file_type().is_file() {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(entry.path(), &target)?;
            }
        }

        Ok(Self::read_world(&dst, &copy_name))
    }

    pub fn delete_world(mc_dir: &Path, folder_name: &str) -> crate::Result<()> {
        if folder_name.contains('/') || folder_name.contains('\\') || folder_name.contains("..") {
            return Err(MinecraftError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid world folder name",
            )));
        }
        let target = mc_dir.join("saves").join(folder_name);
        if target.is_dir() {
            fs::remove_dir_all(&target)?;
        }
        Ok(())
    }

    /// Back up a single world folder into `<target_dir>/world_<sanitized>_<epoch>.zip`
    /// (1:1 port of Python `WorldManager.backup_world`).
    pub fn backup_world(
        mc_dir: &Path,
        world_folder: &str,
        target_dir: Option<&Path>,
    ) -> crate::Result<PathBuf> {
        if world_folder.contains('/') || world_folder.contains('\\') || world_folder.contains("..")
        {
            return Err(MinecraftError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid world folder name",
            )));
        }
        let saves_dir = mc_dir.join("saves");
        let source = saves_dir.join(world_folder);
        if !source.is_dir() {
            return Err(MinecraftError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("World folder '{world_folder}' does not exist."),
            )));
        }

        let out_dir = target_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| mc_dir.join("backups"));
        fs::create_dir_all(&out_dir)?;

        let epoch_secs = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let sanitized: String = world_folder
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let archive_name = format!("world_{sanitized}_{epoch_secs}.zip");
        let archive_path = out_dir.join(archive_name);

        let file = fs::File::create(&archive_path)?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        for entry in WalkDir::new(&source).into_iter().flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(&saves_dir) else {
                continue;
            };
            if rel.file_name().and_then(|n| n.to_str()) == Some("session.lock") {
                continue;
            }
            let arc_name = rel.to_string_lossy().replace('\\', "/");
            zip.start_file(arc_name, opts)?;
            let mut src_f = fs::File::open(entry.path())?;
            std::io::copy(&mut src_f, &mut zip)?;
        }
        zip.finish()?;
        Ok(archive_path)
    }

    /// Restore a world backup `.zip` into `<mc_dir>/saves/` (1:1 port of Python `WorldManager.restore_world`).
    pub fn restore_world(mc_dir: &Path, backup_path: &Path) -> crate::Result<String> {
        Self::import_world(mc_dir, backup_path)
    }

    /// Import a world from either a directory or a `.zip` archive into `<mc_dir>/saves/`
    /// (1:1 port of Python `WorldManager.import_world`).
    pub fn import_world(mc_dir: &Path, source_path: &Path) -> crate::Result<String> {
        let saves_dir = mc_dir.join("saves");
        fs::create_dir_all(&saves_dir)?;

        if source_path.is_dir() {
            let base_name = source_path
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("Imported World");
            let mut target_name = base_name.to_string();
            let mut counter = 1usize;
            while saves_dir.join(&target_name).exists() {
                target_name = format!("{base_name} ({counter})");
                counter += 1;
            }
            let target = saves_dir.join(&target_name);
            for entry in WalkDir::new(source_path).into_iter().flatten() {
                let Ok(rel) = entry.path().strip_prefix(source_path) else {
                    continue;
                };
                if rel.file_name().and_then(|n| n.to_str()) == Some("session.lock") {
                    continue;
                }
                let dest = target.join(rel);
                if entry.file_type().is_dir() {
                    fs::create_dir_all(&dest)?;
                } else if entry.file_type().is_file() {
                    if let Some(p) = dest.parent() {
                        fs::create_dir_all(p)?;
                    }
                    fs::copy(entry.path(), &dest)?;
                }
            }
            return Ok(target_name);
        }

        if source_path.is_file() {
            let file = fs::File::open(source_path)?;
            let mut archive = zip::ZipArchive::new(file)?;
            let mut level_prefix: Option<String> = None;
            for i in 0..archive.len() {
                if let Ok(entry) = archive.by_index(i) {
                    let name = entry.name().replace('\\', "/");
                    if name.starts_with("__MACOSX") {
                        continue;
                    }
                    if name == "level.dat" || name.ends_with("/level.dat") {
                        let parent = name.strip_suffix("level.dat").unwrap_or("");
                        level_prefix = Some(parent.trim_end_matches('/').to_string());
                        break;
                    }
                }
            }

            let stem = source_path
                .file_stem()
                .and_then(|n| n.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("Imported World");
            let base_name = match level_prefix.as_deref() {
                Some(p) if !p.is_empty() && p != "." => Path::new(p)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(stem),
                _ => stem,
            };

            let mut target_name = base_name.to_string();
            let mut counter = 1usize;
            while saves_dir.join(&target_name).exists() {
                target_name = format!("{base_name} ({counter})");
                counter += 1;
            }
            let target_dir = saves_dir.join(&target_name);
            fs::create_dir_all(&target_dir)?;

            let strip_prefix = level_prefix
                .filter(|p| !p.is_empty() && p != ".")
                .map(|p| format!("{p}/"));

            for i in 0..archive.len() {
                let mut entry = archive.by_index(i)?;
                if entry.is_dir() {
                    continue;
                }
                let raw_name = entry.name().replace('\\', "/");
                if raw_name.starts_with("__MACOSX") {
                    continue;
                }
                let rel = if let Some(ref pref) = strip_prefix {
                    let Some(r) = raw_name.strip_prefix(pref) else {
                        continue;
                    };
                    r
                } else {
                    raw_name.as_str()
                };
                if rel.is_empty() || rel.contains("..") {
                    continue;
                }
                let dest = target_dir.join(rel);
                if let Some(p) = dest.parent() {
                    fs::create_dir_all(p)?;
                }
                let mut out_f = fs::File::create(&dest)?;
                std::io::copy(&mut entry, &mut out_f)?;
            }
            return Ok(target_name);
        }

        Err(MinecraftError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "Source '{}' is neither a folder nor a zip archive.",
                source_path.display()
            ),
        )))
    }
}

#[derive(Debug, Default)]
struct NbtLevelSummary {
    level_name: String,
    game_type: i32,
    hardcore: bool,
    last_played: i64,
    version_name: String,
}

#[derive(Debug)]
enum NbtValue {
    Byte(i8),
    Int(i32),
    Long(i64),
    String(String),
    Compound(HashMap<String, NbtValue>),
    Other,
}

fn parse_level_dat(path: &Path) -> Option<NbtLevelSummary> {
    let raw = fs::read(path).ok()?;
    if raw.is_empty() {
        return None;
    }
    let bytes = if raw.len() >= 2 && raw[0] == 0x1f && raw[1] == 0x8b {
        let mut decoder = GzDecoder::new(&raw[..]);
        let mut out = Vec::new();
        decoder.read_to_end(&mut out).ok()?;
        out
    } else {
        raw
    };

    let mut cursor = NbtCursor {
        buf: &bytes,
        pos: 0,
    };
    let root_tag = cursor.read_u8()?;
    if root_tag != 10 {
        return None;
    }
    let _root_name = cursor.read_string()?;
    let root = cursor.read_compound()?;

    let data_map = match root.get("Data") {
        Some(NbtValue::Compound(m)) => m,
        _ => &root,
    };

    let mut summary = NbtLevelSummary::default();
    if let Some(NbtValue::String(s)) = data_map.get("LevelName") {
        summary.level_name = s.clone();
    }
    if let Some(NbtValue::Int(gt)) = data_map.get("GameType") {
        summary.game_type = *gt;
    }
    if let Some(NbtValue::Byte(hc)) = data_map.get("hardcore") {
        summary.hardcore = *hc != 0;
    }
    if let Some(NbtValue::Long(lp)) = data_map.get("LastPlayed") {
        summary.last_played = *lp;
    }
    if let Some(NbtValue::Compound(ver)) = data_map.get("Version") {
        if let Some(NbtValue::String(vname)) = ver.get("Name") {
            summary.version_name = vname.clone();
        }
    }
    Some(summary)
}

struct NbtCursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> NbtCursor<'a> {
    fn read_u8(&mut self) -> Option<u8> {
        let b = *self.buf.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn read_i8(&mut self) -> Option<i8> {
        Some(self.read_u8()? as i8)
    }

    fn read_u16(&mut self) -> Option<u16> {
        let slice = self.buf.get(self.pos..self.pos + 2)?;
        self.pos += 2;
        Some(u16::from_be_bytes([slice[0], slice[1]]))
    }

    fn read_i32(&mut self) -> Option<i32> {
        let slice = self.buf.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(i32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    fn read_i64(&mut self) -> Option<i64> {
        let slice = self.buf.get(self.pos..self.pos + 8)?;
        self.pos += 8;
        Some(i64::from_be_bytes([
            slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
        ]))
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        if self.pos.checked_add(n)? > self.buf.len() {
            return None;
        }
        self.pos += n;
        Some(())
    }

    fn read_string(&mut self) -> Option<String> {
        let len = self.read_u16()? as usize;
        let slice = self.buf.get(self.pos..self.pos + len)?;
        self.pos += len;
        Some(String::from_utf8_lossy(slice).to_string())
    }

    fn read_compound(&mut self) -> Option<HashMap<String, NbtValue>> {
        let mut map = HashMap::new();
        loop {
            let tag = self.read_u8()?;
            if tag == 0 {
                break;
            }
            let name = self.read_string()?;
            let val = self.read_payload(tag)?;
            map.insert(name, val);
        }
        Some(map)
    }

    fn read_payload(&mut self, tag: u8) -> Option<NbtValue> {
        match tag {
            1 => Some(NbtValue::Byte(self.read_i8()?)),
            2 => {
                self.skip(2)?;
                Some(NbtValue::Other)
            }
            3 => Some(NbtValue::Int(self.read_i32()?)),
            4 => Some(NbtValue::Long(self.read_i64()?)),
            5 => {
                self.skip(4)?;
                Some(NbtValue::Other)
            }
            6 => {
                self.skip(8)?;
                Some(NbtValue::Other)
            }
            7 => {
                let len = self.read_i32()?.max(0) as usize;
                self.skip(len)?;
                Some(NbtValue::Other)
            }
            8 => Some(NbtValue::String(self.read_string()?)),
            9 => {
                let elem_tag = self.read_u8()?;
                let len = self.read_i32()?.max(0) as usize;
                for _ in 0..len {
                    self.read_payload(elem_tag)?;
                }
                Some(NbtValue::Other)
            }
            10 => Some(NbtValue::Compound(self.read_compound()?)),
            11 => {
                let len = self.read_i32()?.max(0) as usize;
                self.skip(len.checked_mul(4)?)?;
                Some(NbtValue::Other)
            }
            12 => {
                let len = self.read_i32()?.max(0) as usize;
                self.skip(len.checked_mul(8)?)?;
                Some(NbtValue::Other)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn detects_quick_play_support_for_releases_and_snapshots() {
        assert!(is_quick_play_supported("1.21.4"));
        assert!(is_quick_play_supported("1.20"));
        assert!(is_quick_play_supported("23w14a"));
        assert!(is_quick_play_supported("24w10a"));
        assert!(!is_quick_play_supported("1.19.4"));
        assert!(!is_quick_play_supported("1.12.2"));
        assert!(!is_quick_play_supported("23w13a"));
    }

    #[test]
    fn parses_gzipped_level_dat_and_duplicates_world() {
        let dir = tempdir().unwrap();
        let saves = dir.path().join("saves");
        let world_dir = saves.join("my_world");
        fs::create_dir_all(&world_dir).unwrap();

        // Build minimal gzipped NBT: Compound("") -> Compound("Data") -> LevelName, GameType, LastPlayed
        let mut nbt = Vec::new();
        nbt.push(10); // Root compound
        nbt.extend_from_slice(&0u16.to_be_bytes());
        nbt.push(10); // Data compound
        nbt.extend_from_slice(&4u16.to_be_bytes());
        nbt.extend_from_slice(b"Data");
        // LevelName = "Survival Base"
        nbt.push(8);
        nbt.extend_from_slice(&9u16.to_be_bytes());
        nbt.extend_from_slice(b"LevelName");
        nbt.extend_from_slice(&13u16.to_be_bytes());
        nbt.extend_from_slice(b"Survival Base");
        // GameType = 1 (creative)
        nbt.push(3);
        nbt.extend_from_slice(&8u16.to_be_bytes());
        nbt.extend_from_slice(b"GameType");
        nbt.extend_from_slice(&1i32.to_be_bytes());
        // LastPlayed = 1700000000000
        nbt.push(4);
        nbt.extend_from_slice(&10u16.to_be_bytes());
        nbt.extend_from_slice(b"LastPlayed");
        nbt.extend_from_slice(&1_700_000_000_000i64.to_be_bytes());
        nbt.push(0); // End Data
        nbt.push(0); // End Root

        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&nbt).unwrap();
        fs::write(world_dir.join("level.dat"), enc.finish().unwrap()).unwrap();
        fs::write(world_dir.join("session.lock"), b"lock").unwrap();

        let worlds = WorldManager::list_worlds(dir.path());
        assert_eq!(worlds.len(), 1);
        assert_eq!(worlds[0].display_name, "Survival Base");
        assert_eq!(worlds[0].game_mode, "creative");
        assert_eq!(worlds[0].last_played_ms, 1_700_000_000_000);

        let dup = WorldManager::duplicate_world(dir.path(), "my_world").unwrap();
        assert_eq!(dup.folder_name, "my_world (Copy)");
        assert!(!dup.folder_path.join("session.lock").exists());

        WorldManager::delete_world(dir.path(), "my_world (Copy)").unwrap();
        assert_eq!(WorldManager::list_worlds(dir.path()).len(), 1);

        // Backup & Restore / Import roundtrip
        let backup_zip = WorldManager::backup_world(dir.path(), "my_world", None).unwrap();
        assert!(backup_zip.is_file());
        let restored_name = WorldManager::restore_world(dir.path(), &backup_zip).unwrap();
        assert_eq!(restored_name, "my_world (1)");
        assert_eq!(WorldManager::list_worlds(dir.path()).len(), 2);
    }
}
