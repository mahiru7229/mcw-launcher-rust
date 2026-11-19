use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const KEYSTREAM_DOMAIN: &[u8] = b"mcw-launcher-cf-keystream-v1";

fn main() {
    println!("cargo:rerun-if-env-changed=CURSEFORGE_API_KEY");
    println!("cargo:rerun-if-changed=../../.env");

    let api_key = resolve_build_time_key();
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let dest = out_dir.join("embedded_cf_key.rs");

    let generated = match api_key {
        Some(secret) => {
            let salt = generate_build_salt();
            let ciphertext = xor_with_sha256_ctr(secret.as_bytes(), &salt);
            format!(
                "const EMBEDDED_CF_SALT: [u8; 32] = {:?};\nconst EMBEDDED_CF_CIPHERTEXT: &[u8] = &{:?};\n",
                salt, ciphertext
            )
        }
        None => {
            "const EMBEDDED_CF_SALT: [u8; 32] = [0u8; 32];\nconst EMBEDDED_CF_CIPHERTEXT: &[u8] = &[];\n"
                .to_string()
        }
    };

    fs::write(&dest, generated).expect("failed to write embedded_cf_key.rs");
}

fn resolve_build_time_key() -> Option<String> {
    if let Ok(val) = env::var("CURSEFORGE_API_KEY") {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").ok()?;
    let workspace_env = Path::new(&manifest_dir).join("..").join("..").join(".env");
    let content = fs::read_to_string(workspace_env).ok()?;
    parse_dotenv_key(&content, "CURSEFORGE_API_KEY")
}

fn parse_dotenv_key(content: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        if let Some(rest) = line.strip_prefix(&prefix) {
            let val = rest.trim().trim_matches('"').trim_matches('\'').trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

fn generate_build_salt() -> [u8; 32] {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let mut hasher = Sha256::new();
    hasher.update(b"mcw-launcher-cf-salt-v1");
    hasher.update(nanos.to_le_bytes());
    hasher.update(pid.to_le_bytes());
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn xor_with_sha256_ctr(input: &[u8], salt: &[u8; 32]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    for (block_idx, chunk) in input.chunks(32).enumerate() {
        let mut hasher = Sha256::new();
        hasher.update(KEYSTREAM_DOMAIN);
        hasher.update(salt);
        hasher.update((block_idx as u32).to_le_bytes());
        let keystream = hasher.finalize();
        for (b, k) in chunk.iter().zip(keystream.iter()) {
            output.push(b ^ k);
        }
    }
    output
}
