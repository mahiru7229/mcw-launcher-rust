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
    pub seed: Option<i64>,
    pub icon_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct WorldDataPackInfo {
    pub file_name: String,
    pub display_name: String,
    pub path: PathBuf,
    pub enabled: bool,
    pub size_bytes: u64,
    pub is_directory: bool,
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

fn is_safe_single_component(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && trimmed != "."
        && trimmed != ".."
        && !trimmed.contains('/')
        && !trimmed.contains('\\')
        && !trimmed.contains("..")
}

pub struct WorldManager;

impl WorldManager {
    /// Lists all data packs (`.zip`, `.zip.disabled`, or directory packs) inside `saves/<world>/datapacks/`.
    pub fn list_datapacks(mc_dir: &Path, world_folder: &str) -> Vec<WorldDataPackInfo> {
        if !is_safe_single_component(world_folder) {
            return Vec::new();
        }
        let dp_dir = mc_dir.join("saves").join(world_folder).join("datapacks");
        let Ok(entries) = fs::read_dir(&dp_dir) else {
            return Vec::new();
        };

        let mut packs = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                if !path.join("pack.mcmeta").is_file() {
                    continue;
                }
                let size_bytes = dir_size(&path);
                packs.push(WorldDataPackInfo {
                    display_name: file_name.clone(),
                    file_name,
                    path,
                    enabled: true,
                    size_bytes,
                    is_directory: true,
                });
                continue;
            }
            if !path.is_file() {
                continue;
            }
            let lower = file_name.to_ascii_lowercase();
            let (enabled, display_name) = if lower.ends_with(".zip") {
                (true, file_name[..file_name.len() - 4].to_string())
            } else if lower.ends_with(".zip.disabled") {
                (false, file_name[..file_name.len() - 13].to_string())
            } else {
                continue;
            };
            let size_bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            packs.push(WorldDataPackInfo {
                file_name,
                display_name,
                path,
                enabled,
                size_bytes,
                is_directory: false,
            });
        }
        packs.sort_by(|a, b| {
            a.display_name
                .to_ascii_lowercase()
                .cmp(&b.display_name.to_ascii_lowercase())
        });
        packs
    }

    /// Enables or disables a `.zip` data pack by renaming `.zip` <-> `.zip.disabled`.
    pub fn toggle_datapack(
        mc_dir: &Path,
        world_folder: &str,
        file_name: &str,
        enable: bool,
    ) -> crate::Result<String> {
        if !is_safe_single_component(world_folder) || !is_safe_single_component(file_name) {
            return Err(MinecraftError::InvalidMetadata(
                "Invalid world or data pack name".to_string(),
            ));
        }
        let dp_dir = mc_dir.join("saves").join(world_folder).join("datapacks");
        let src = dp_dir.join(file_name);
        if !src.is_file() {
            return Err(MinecraftError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Data pack '{file_name}' not found"),
            )));
        }
        let target_name = if enable {
            file_name
                .strip_suffix(".disabled")
                .or_else(|| file_name.strip_suffix(".DISABLED"))
                .unwrap_or(file_name)
                .to_string()
        } else if file_name.to_ascii_lowercase().ends_with(".disabled") {
            file_name.to_string()
        } else {
            format!("{file_name}.disabled")
        };
        if target_name != file_name {
            let dst = dp_dir.join(&target_name);
            fs::rename(&src, &dst)?;
        }
        Ok(target_name)
    }

    /// Copies a local `.zip` data pack into `saves/<world>/datapacks/`.
    pub fn import_datapack(
        mc_dir: &Path,
        world_folder: &str,
        source_zip: &Path,
    ) -> crate::Result<String> {
        if !is_safe_single_component(world_folder) {
            return Err(MinecraftError::InvalidMetadata(
                "Invalid world folder name".to_string(),
            ));
        }
        let file_name = source_zip
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty() && n.to_ascii_lowercase().ends_with(".zip"))
            .ok_or_else(|| {
                MinecraftError::InvalidMetadata("Data pack file must be a .zip archive".to_string())
            })?
            .to_string();
        let dp_dir = mc_dir.join("saves").join(world_folder).join("datapacks");
        fs::create_dir_all(&dp_dir)?;
        let dest = dp_dir.join(&file_name);
        fs::copy(source_zip, &dest)?;
        Ok(file_name)
    }

    /// Removes a data pack file or directory from `saves/<world>/datapacks/`.
    pub fn remove_datapack(
        mc_dir: &Path,
        world_folder: &str,
        file_name: &str,
    ) -> crate::Result<()> {
        if !is_safe_single_component(world_folder) || !is_safe_single_component(file_name) {
            return Err(MinecraftError::InvalidMetadata(
                "Invalid world or data pack name".to_string(),
            ));
        }
        let target = mc_dir
            .join("saves")
            .join(world_folder)
            .join("datapacks")
            .join(file_name);
        if target.is_dir() {
            fs::remove_dir_all(&target)?;
        } else if target.is_file() {
            fs::remove_file(&target)?;
        }
        Ok(())
    }

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

        let size_bytes = dir_size(folder);

        let icon_file = folder.join("icon.png");
        let icon_path = icon_file.is_file().then_some(icon_file);

        WorldInfo {
            folder_name: folder_name.to_string(),
            display_name,
            game_mode,
            is_hardcore: nbt.hardcore,
            last_played_ms,
            version_name: nbt.version_name,
            size_bytes,
            folder_path: folder.to_path_buf(),
            seed: nbt.seed,
            icon_path,
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

        write_world_zip(&saves_dir, &source, &archive_path)?;
        Ok(archive_path)
    }

    /// Export a single world folder directly to a user-chosen `.zip` destination path.
    pub fn export_world(
        mc_dir: &Path,
        world_folder: &str,
        dest_zip: &Path,
    ) -> crate::Result<usize> {
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
        if let Some(parent) = dest_zip.parent() {
            fs::create_dir_all(parent)?;
        }
        write_world_zip(&saves_dir, &source, dest_zip)
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

fn dir_size(dir: &Path) -> u64 {
    WalkDir::new(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

#[derive(Debug, Default)]
struct NbtLevelSummary {
    level_name: String,
    game_type: i32,
    hardcore: bool,
    last_played: i64,
    version_name: String,
    seed: Option<i64>,
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
    if let Some(NbtValue::Compound(wgs)) = data_map.get("WorldGenSettings") {
        if let Some(NbtValue::Long(s)) = wgs.get("seed") {
            summary.seed = Some(*s);
        }
    }
    if summary.seed.is_none() {
        if let Some(NbtValue::Long(s)) = data_map.get("RandomSeed") {
            summary.seed = Some(*s);
        }
    }
    Some(summary)
}

fn write_world_zip(saves_dir: &Path, source: &Path, archive_path: &Path) -> crate::Result<usize> {
    let file = fs::File::create(archive_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut count = 0usize;

    for entry in WalkDir::new(source).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(saves_dir) else {
            continue;
        };
        if rel.file_name().and_then(|n| n.to_str()) == Some("session.lock") {
            continue;
        }
        let arc_name = rel.to_string_lossy().replace('\\', "/");
        zip.start_file(arc_name, opts)?;
        let mut src_f = fs::File::open(entry.path())?;
        std::io::copy(&mut src_f, &mut zip)?;
        count += 1;
    }
    zip.finish()?;
    Ok(count)
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
        // RandomSeed = -123456789012345
        nbt.push(4);
        nbt.extend_from_slice(&10u16.to_be_bytes());
        nbt.extend_from_slice(b"RandomSeed");
        nbt.extend_from_slice(&(-123_456_789_012_345i64).to_be_bytes());
        nbt.push(0); // End Data
        nbt.push(0); // End Root

        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&nbt).unwrap();
        fs::write(world_dir.join("level.dat"), enc.finish().unwrap()).unwrap();
        fs::write(world_dir.join("icon.png"), b"png").unwrap();
        fs::write(world_dir.join("session.lock"), b"lock").unwrap();

        let worlds = WorldManager::list_worlds(dir.path());
        assert_eq!(worlds.len(), 1);
        assert_eq!(worlds[0].display_name, "Survival Base");
        assert_eq!(worlds[0].game_mode, "creative");
        assert_eq!(worlds[0].last_played_ms, 1_700_000_000_000);
        assert_eq!(worlds[0].seed, Some(-123_456_789_012_345));
        assert!(worlds[0].icon_path.is_some());

        let exported_zip = dir.path().join("exported.zip");
        let files_written =
            WorldManager::export_world(dir.path(), "my_world", &exported_zip).unwrap();
        assert!(files_written >= 2);
        assert!(exported_zip.is_file());

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

    #[test]
    fn manages_world_datapacks_list_toggle_import_and_remove() {
        let dir = tempdir().unwrap();
        let src_zip = dir.path().join("terralith.zip");
        fs::write(&src_zip, b"PK\x03\x04fake-zip").unwrap();

        let imported = WorldManager::import_datapack(dir.path(), "my_world", &src_zip).unwrap();
        assert_eq!(imported, "terralith.zip");

        let packs = WorldManager::list_datapacks(dir.path(), "my_world");
        assert_eq!(packs.len(), 1);
        assert!(packs[0].enabled);

        let disabled_name =
            WorldManager::toggle_datapack(dir.path(), "my_world", "terralith.zip", false).unwrap();
        assert_eq!(disabled_name, "terralith.zip.disabled");
        let packs_disabled = WorldManager::list_datapacks(dir.path(), "my_world");
        assert!(!packs_disabled[0].enabled);

        let reenabled =
            WorldManager::toggle_datapack(dir.path(), "my_world", &disabled_name, true).unwrap();
        assert_eq!(reenabled, "terralith.zip");

        WorldManager::remove_datapack(dir.path(), "my_world", &reenabled).unwrap();
        assert!(WorldManager::list_datapacks(dir.path(), "my_world").is_empty());
    }
}
