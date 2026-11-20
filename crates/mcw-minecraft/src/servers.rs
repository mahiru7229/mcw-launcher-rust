use crate::{MinecraftError, Result};
use mcw_storage::atomic_write_bytes;
use std::fs;
use std::path::Path;

const TAG_END: u8 = 0;
const TAG_BYTE: u8 = 1;
const TAG_SHORT: u8 = 2;
const TAG_INT: u8 = 3;
const TAG_LONG: u8 = 4;
const TAG_FLOAT: u8 = 5;
const TAG_DOUBLE: u8 = 6;
const TAG_BYTE_ARRAY: u8 = 7;
const TAG_STRING: u8 = 8;
const TAG_LIST: u8 = 9;
const TAG_COMPOUND: u8 = 10;
const TAG_INT_ARRAY: u8 = 11;
const TAG_LONG_ARRAY: u8 = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedServer {
    pub name: String,
    pub ip: String,
    pub accept_textures: Option<u8>,
    pub hidden: bool,
}

/// Manages Minecraft Java Edition `<mc_dir>/servers.dat` (uncompressed big-endian NBT).
pub struct ServerManager;

impl ServerManager {
    /// Reads all entries from `<mc_dir>/servers.dat`. Returns empty on missing or malformed files.
    pub fn list_servers(mc_dir: &Path) -> Vec<SavedServer> {
        let path = mc_dir.join("servers.dat");
        let Ok(bytes) = fs::read(&path) else {
            return Vec::new();
        };
        parse_servers_dat(&bytes).unwrap_or_default()
    }

    /// Appends a new server entry to `<mc_dir>/servers.dat` and writes it atomically.
    pub fn add_server(mc_dir: &Path, name: &str, ip: &str) -> Result<Vec<SavedServer>> {
        let clean_ip = ip.trim();
        if clean_ip.is_empty() {
            return Err(MinecraftError::InvalidMetadata(
                "Server address cannot be empty".to_string(),
            ));
        }
        let clean_name = if name.trim().is_empty() {
            clean_ip.to_string()
        } else {
            name.trim().to_string()
        };

        let mut servers = Self::list_servers(mc_dir);
        servers.push(SavedServer {
            name: clean_name,
            ip: clean_ip.to_string(),
            accept_textures: None,
            hidden: false,
        });
        Self::write_servers(mc_dir, &servers)?;
        Ok(servers)
    }

    /// Removes the server entry at `index` from `<mc_dir>/servers.dat` and writes it atomically.
    pub fn remove_server(mc_dir: &Path, index: usize) -> Result<Vec<SavedServer>> {
        let mut servers = Self::list_servers(mc_dir);
        if index >= servers.len() {
            return Err(MinecraftError::InvalidMetadata(format!(
                "Server index {index} is out of range"
            )));
        }
        servers.remove(index);
        Self::write_servers(mc_dir, &servers)?;
        Ok(servers)
    }

    fn write_servers(mc_dir: &Path, servers: &[SavedServer]) -> Result<()> {
        fs::create_dir_all(mc_dir)?;
        let encoded = encode_servers_dat(servers);
        let path = mc_dir.join("servers.dat");
        atomic_write_bytes(&path, &encoded)
            .map_err(|e| MinecraftError::Io(std::io::Error::other(e.to_string())))?;
        Ok(())
    }
}

fn parse_servers_dat(bytes: &[u8]) -> Option<Vec<SavedServer>> {
    let mut r = NbtCursor::new(bytes);
    if r.read_u8()? != TAG_COMPOUND {
        return None;
    }
    let _root_name = r.read_string()?;
    let mut servers = None;

    while let Some(tag) = r.read_u8() {
        if tag == TAG_END {
            break;
        }
        let key = r.read_string()?;
        if key == "servers" && tag == TAG_LIST {
            servers = Some(read_servers_list(&mut r)?);
        } else {
            r.skip_payload(tag, 0)?;
        }
    }

    Some(servers.unwrap_or_default())
}

fn read_servers_list(r: &mut NbtCursor<'_>) -> Option<Vec<SavedServer>> {
    let elem_tag = r.read_u8()?;
    let count = r.read_i32()?;
    if count <= 0 {
        return Some(Vec::new());
    }
    let count = count as usize;
    if elem_tag != TAG_COMPOUND {
        for _ in 0..count {
            r.skip_payload(elem_tag, 0)?;
        }
        return Some(Vec::new());
    }

    let mut list = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        let mut entry = SavedServer {
            name: String::new(),
            ip: String::new(),
            accept_textures: None,
            hidden: false,
        };
        while let Some(tag) = r.read_u8() {
            if tag == TAG_END {
                break;
            }
            let key = r.read_string()?;
            match (key.as_str(), tag) {
                ("name", TAG_STRING) => entry.name = r.read_string()?,
                ("ip", TAG_STRING) => entry.ip = r.read_string()?,
                ("acceptTextures", TAG_BYTE) => entry.accept_textures = Some(r.read_u8()?),
                ("hidden", TAG_BYTE) => entry.hidden = r.read_u8()? != 0,
                _ => r.skip_payload(tag, 0)?,
            }
        }
        if !entry.ip.trim().is_empty() || !entry.name.trim().is_empty() {
            list.push(entry);
        }
    }
    Some(list)
}

fn encode_servers_dat(servers: &[SavedServer]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64 + servers.len() * 64);
    buf.push(TAG_COMPOUND);
    write_string(&mut buf, "");

    buf.push(TAG_LIST);
    write_string(&mut buf, "servers");
    buf.push(TAG_COMPOUND);
    buf.extend_from_slice(&(servers.len() as i32).to_be_bytes());

    for srv in servers {
        buf.push(TAG_STRING);
        write_string(&mut buf, "name");
        write_string(&mut buf, &srv.name);

        buf.push(TAG_STRING);
        write_string(&mut buf, "ip");
        write_string(&mut buf, &srv.ip);

        if let Some(accept) = srv.accept_textures {
            buf.push(TAG_BYTE);
            write_string(&mut buf, "acceptTextures");
            buf.push(accept);
        }
        if srv.hidden {
            buf.push(TAG_BYTE);
            write_string(&mut buf, "hidden");
            buf.push(1);
        }
        buf.push(TAG_END);
    }

    buf.push(TAG_END);
    buf
}

fn write_string(buf: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    let len = bytes.len().min(u16::MAX as usize) as u16;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&bytes[..len as usize]);
}

struct NbtCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> NbtCursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        if end > self.data.len() {
            return None;
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Some(slice)
    }

    fn read_u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn read_u16(&mut self) -> Option<u16> {
        let b = self.take(2)?;
        Some(u16::from_be_bytes([b[0], b[1]]))
    }

    fn read_i32(&mut self) -> Option<i32> {
        let b = self.take(4)?;
        Some(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_string(&mut self) -> Option<String> {
        let len = self.read_u16()? as usize;
        let bytes = self.take(len)?;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }

    fn skip_payload(&mut self, tag: u8, depth: usize) -> Option<()> {
        if depth > 64 {
            return None;
        }
        match tag {
            TAG_END => Some(()),
            TAG_BYTE => self.take(1).map(|_| ()),
            TAG_SHORT => self.take(2).map(|_| ()),
            TAG_INT | TAG_FLOAT => self.take(4).map(|_| ()),
            TAG_LONG | TAG_DOUBLE => self.take(8).map(|_| ()),
            TAG_BYTE_ARRAY => {
                let len = usize::try_from(self.read_i32()?).ok()?;
                self.take(len).map(|_| ())
            }
            TAG_STRING => {
                let len = self.read_u16()? as usize;
                self.take(len).map(|_| ())
            }
            TAG_LIST => {
                let elem = self.read_u8()?;
                let count = self.read_i32()?;
                if count <= 0 {
                    return Some(());
                }
                for _ in 0..count {
                    self.skip_payload(elem, depth + 1)?;
                }
                Some(())
            }
            TAG_COMPOUND => {
                while let Some(child) = self.read_u8() {
                    if child == TAG_END {
                        return Some(());
                    }
                    let _ = self.read_string()?;
                    self.skip_payload(child, depth + 1)?;
                }
                None
            }
            TAG_INT_ARRAY => {
                let count = usize::try_from(self.read_i32()?).ok()?;
                let bytes = count.checked_mul(4)?;
                self.take(bytes).map(|_| ())
            }
            TAG_LONG_ARRAY => {
                let count = usize::try_from(self.read_i32()?).ok()?;
                let bytes = count.checked_mul(8)?;
                self.take(bytes).map(|_| ())
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn adds_lists_and_removes_servers_in_nbt_servers_dat() {
        let dir = tempdir().unwrap();
        let mc_dir = dir.path();

        let after_first = ServerManager::add_server(mc_dir, "Hypixel", "mc.hypixel.net").unwrap();
        assert_eq!(after_first.len(), 1);
        assert_eq!(after_first[0].name, "Hypixel");
        assert_eq!(after_first[0].ip, "mc.hypixel.net");

        // Append extra server preserving acceptTextures/hidden when persisted manually
        let mut with_opts = after_first;
        with_opts.push(SavedServer {
            name: "Local LAN".to_string(),
            ip: "192.168.1.50:25565".to_string(),
            accept_textures: Some(1),
            hidden: false,
        });
        ServerManager::write_servers(mc_dir, &with_opts).unwrap();

        let listed = ServerManager::list_servers(mc_dir);
        assert_eq!(listed, with_opts);

        let after_remove = ServerManager::remove_server(mc_dir, 0).unwrap();
        assert_eq!(after_remove.len(), 1);
        assert_eq!(after_remove[0].name, "Local LAN");
        assert_eq!(after_remove[0].accept_textures, Some(1));
    }

    #[test]
    fn handles_missing_or_malformed_servers_dat_gracefully() {
        let dir = tempdir().unwrap();
        assert!(ServerManager::list_servers(dir.path()).is_empty());

        fs::write(dir.path().join("servers.dat"), b"not-valid-nbt-payload").unwrap();
        assert!(ServerManager::list_servers(dir.path()).is_empty());
    }
}
