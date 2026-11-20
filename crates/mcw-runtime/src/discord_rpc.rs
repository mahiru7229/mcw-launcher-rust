use serde_json::json;
use std::io::{Read, Write};
use std::sync::mpsc::{self, Sender};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

pub const DEFAULT_DISCORD_CLIENT_ID: &str = "762726289341677668";

pub const OP_HANDSHAKE: u32 = 0;
pub const OP_FRAME: u32 = 1;
pub const OP_CLOSE: u32 = 2;

pub fn encode_ipc_frame(op: u32, payload: &serde_json::Value) -> Vec<u8> {
    let body = serde_json::to_vec(payload).unwrap_or_default();
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

pub fn decode_ipc_header(header: [u8; 8]) -> (u32, u32) {
    let op = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    (op, len)
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordActivity {
    pub details: String,
    pub state: String,
    pub large_image: String,
    pub large_text: String,
    pub start_timestamp: Option<u64>,
}

impl DiscordActivity {
    pub fn for_launcher() -> Self {
        Self {
            details: "Browsing instances".to_string(),
            state: "MCW Launcher".to_string(),
            large_image: "default".to_string(),
            large_text: "MCW Launcher".to_string(),
            start_timestamp: Some(unix_now_secs()),
        }
    }

    pub fn for_game(instance_name: &str, version_id: &str, loader: &str) -> Self {
        let loader_clean = loader.trim();
        let details = if loader_clean.is_empty() || loader_clean.eq_ignore_ascii_case("vanilla") {
            format!("Minecraft {}", version_id.trim())
        } else {
            let mut chars = loader_clean.chars();
            let cap = match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            };
            format!("{cap} {}", version_id.trim())
        };
        let inst_clean = if instance_name.trim().is_empty() {
            "Minecraft"
        } else {
            instance_name.trim()
        };
        Self {
            details,
            state: format!("Instance: {inst_clean}"),
            large_image: "default".to_string(),
            large_text: format!("MCW Launcher — {inst_clean}"),
            start_timestamp: Some(unix_now_secs()),
        }
    }

    pub fn to_set_activity_payload(&self, pid: u32, nonce: &str) -> serde_json::Value {
        let mut activity = json!({
            "details": self.details,
            "state": self.state,
            "assets": {
                "large_image": self.large_image,
                "large_text": self.large_text,
            }
        });
        if let Some(ts) = self.start_timestamp {
            activity["timestamps"] = json!({ "start": ts });
        }
        json!({
            "cmd": "SET_ACTIVITY",
            "args": {
                "pid": pid,
                "activity": activity,
            },
            "nonce": nonce,
        })
    }
}

#[cfg(windows)]
type IpcStream = std::fs::File;
#[cfg(not(windows))]
type IpcStream = std::os::unix::net::UnixStream;

struct DiscordIpcTransport {
    stream: Option<IpcStream>,
    nonce: u64,
}

impl DiscordIpcTransport {
    fn new() -> Self {
        Self {
            stream: None,
            nonce: 1,
        }
    }

    fn connect(&mut self, client_id: &str) -> bool {
        self.disconnect();
        let id = if client_id.trim().is_empty() {
            DEFAULT_DISCORD_CLIENT_ID
        } else {
            client_id.trim()
        };

        #[cfg(windows)]
        let opened: Option<IpcStream> = (0..=9).find_map(|i| {
            let path = format!(r"\\.\pipe\discord-ipc-{i}");
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .ok()
        });

        #[cfg(not(windows))]
        let opened: Option<IpcStream> = {
            let mut dirs = Vec::new();
            for var in ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"] {
                if let Ok(v) = std::env::var(var) {
                    if !v.trim().is_empty() {
                        dirs.push(v);
                    }
                }
            }
            dirs.push("/tmp".to_string());
            let mut found = None;
            'outer: for dir in dirs {
                for i in 0..=9 {
                    let path = std::path::Path::new(&dir).join(format!("discord-ipc-{i}"));
                    if let Ok(sock) = std::os::unix::net::UnixStream::connect(&path) {
                        let _ = sock.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                        let _ = sock.set_write_timeout(Some(std::time::Duration::from_secs(2)));
                        found = Some(sock);
                        break 'outer;
                    }
                }
            }
            found
        };

        let Some(mut stream) = opened else {
            return false;
        };

        let handshake = encode_ipc_frame(OP_HANDSHAKE, &json!({ "v": 1, "client_id": id }));
        if stream.write_all(&handshake).is_err() || stream.flush().is_err() {
            return false;
        }
        if Self::read_frame(&mut stream).is_err() {
            return false;
        }

        self.stream = Some(stream);
        true
    }

    fn read_frame(stream: &mut IpcStream) -> std::io::Result<(u32, serde_json::Value)> {
        let mut hdr = [0u8; 8];
        stream.read_exact(&mut hdr)?;
        let (op, len) = decode_ipc_header(hdr);
        let safe_len = len.min(1024 * 1024) as usize;
        let mut buf = vec![0u8; safe_len];
        if safe_len > 0 {
            stream.read_exact(&mut buf)?;
        }
        let val = serde_json::from_slice(&buf).unwrap_or(serde_json::Value::Null);
        Ok((op, val))
    }

    fn send_activity(&mut self, client_id: &str, activity: Option<&DiscordActivity>) -> bool {
        if self.stream.is_none() && !self.connect(client_id) {
            return false;
        }
        self.nonce = self.nonce.wrapping_add(1);
        let nonce_str = format!("mcw-{}", self.nonce);
        let pid = std::process::id();
        let payload = match activity {
            Some(act) => act.to_set_activity_payload(pid, &nonce_str),
            None => json!({
                "cmd": "SET_ACTIVITY",
                "args": { "pid": pid },
                "nonce": nonce_str,
            }),
        };
        let frame = encode_ipc_frame(OP_FRAME, &payload);
        if let Some(ref mut stream) = self.stream {
            if stream.write_all(&frame).is_ok()
                && stream.flush().is_ok()
                && Self::read_frame(stream).is_ok()
            {
                return true;
            }
        }
        self.disconnect();
        false
    }

    fn disconnect(&mut self) {
        if let Some(mut stream) = self.stream.take() {
            let close_frame = encode_ipc_frame(OP_CLOSE, &json!({}));
            let _ = stream.write_all(&close_frame);
            let _ = stream.flush();
        }
    }
}

enum RpcCommand {
    Configure {
        enabled: bool,
        client_id: String,
    },
    SetGame {
        instance_name: String,
        version_id: String,
        loader: String,
    },
    SetIdle,
    Shutdown,
}

/// Non-blocking Discord Rich Presence service running on a dedicated lightweight worker thread.
pub struct DiscordRpcService {
    tx: Sender<RpcCommand>,
}

impl DiscordRpcService {
    pub fn new(enabled: bool, client_id: String) -> Self {
        let (tx, rx) = mpsc::channel::<RpcCommand>();
        let init_enabled = enabled;
        let init_id = client_id;
        let _ = std::thread::Builder::new()
            .name("mcw-discord-rpc".to_string())
            .spawn(move || {
                let mut transport = DiscordIpcTransport::new();
                let mut is_enabled = init_enabled;
                let mut current_client_id = init_id;
                let mut current_activity = DiscordActivity::for_launcher();

                if is_enabled
                    && !transport.send_activity(&current_client_id, Some(&current_activity))
                {
                    debug!("Discord desktop client not detected at startup; RPC idle");
                }

                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        RpcCommand::Configure { enabled, client_id } => {
                            let client_changed = client_id.trim() != current_client_id.trim();
                            is_enabled = enabled;
                            current_client_id = client_id;
                            if !is_enabled {
                                let _ = transport.send_activity(&current_client_id, None);
                                transport.disconnect();
                            } else {
                                if client_changed {
                                    transport.disconnect();
                                }
                                let _ = transport
                                    .send_activity(&current_client_id, Some(&current_activity));
                            }
                        }
                        RpcCommand::SetGame {
                            instance_name,
                            version_id,
                            loader,
                        } => {
                            current_activity =
                                DiscordActivity::for_game(&instance_name, &version_id, &loader);
                            if is_enabled {
                                let _ = transport
                                    .send_activity(&current_client_id, Some(&current_activity));
                            }
                        }
                        RpcCommand::SetIdle => {
                            current_activity = DiscordActivity::for_launcher();
                            if is_enabled {
                                let _ = transport
                                    .send_activity(&current_client_id, Some(&current_activity));
                            }
                        }
                        RpcCommand::Shutdown => {
                            let _ = transport.send_activity(&current_client_id, None);
                            transport.disconnect();
                            break;
                        }
                    }
                }
            });
        Self { tx }
    }

    pub fn configure(&self, enabled: bool, client_id: String) {
        let _ = self.tx.send(RpcCommand::Configure { enabled, client_id });
    }

    pub fn on_game_started(&self, instance_name: String, version_id: String, loader: String) {
        let _ = self.tx.send(RpcCommand::SetGame {
            instance_name,
            version_id,
            loader,
        });
    }

    pub fn on_game_stopped(&self) {
        let _ = self.tx.send(RpcCommand::SetIdle);
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(RpcCommand::Shutdown);
    }
}

impl Drop for DiscordRpcService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_and_decodes_discord_ipc_frame_header() {
        let payload = json!({ "v": 1, "client_id": DEFAULT_DISCORD_CLIENT_ID });
        let frame = encode_ipc_frame(OP_HANDSHAKE, &payload);
        assert!(frame.len() > 8);
        let mut hdr = [0u8; 8];
        hdr.copy_from_slice(&frame[..8]);
        let (op, len) = decode_ipc_header(hdr);
        assert_eq!(op, OP_HANDSHAKE);
        assert_eq!(len as usize, frame.len() - 8);
    }

    #[test]
    fn builds_vanilla_and_modded_game_activities() {
        let vanilla = DiscordActivity::for_game("Survival", "1.21.4", "vanilla");
        assert_eq!(vanilla.details, "Minecraft 1.21.4");
        assert_eq!(vanilla.state, "Instance: Survival");

        let modded = DiscordActivity::for_game("RLCraft", "1.12.2", "forge");
        assert_eq!(modded.details, "Forge 1.12.2");
        assert_eq!(modded.state, "Instance: RLCraft");

        let payload = modded.to_set_activity_payload(1234, "nonce-1");
        assert_eq!(payload["cmd"], "SET_ACTIVITY");
        assert_eq!(payload["args"]["pid"], 1234);
        assert_eq!(payload["args"]["activity"]["details"], "Forge 1.12.2");
    }
}
