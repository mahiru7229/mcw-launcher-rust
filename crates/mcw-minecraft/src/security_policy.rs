use crate::{MinecraftError, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use mcw_net::DownloadClient;
use mcw_storage::{atomic_write_text, LauncherPaths};
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const REMOTE_SECURITY_POLICY_URL: &str = "https://mcw-download.pages.dev/security/policy.json";

/// Embedded Ed25519 public key used to verify signed remote security policy envelopes.
/// Mirrors MCW's signed metadata distribution model; zero remote execution instructions exist
/// inside the policy schema.
pub const EMBEDDED_POLICY_ED25519_PUBKEY: [u8; 32] = [
    0x3b, 0x6a, 0x27, 0xbc, 0xce, 0xb6, 0xa4, 0x2d, 0x62, 0xa3, 0xa8, 0xd0, 0x2a, 0x6f, 0x0d, 0x73,
    0x65, 0x32, 0x15, 0x77, 0x1d, 0xe2, 0x43, 0xa6, 0x3a, 0xc0, 0x48, 0xa1, 0x8b, 0x59, 0xda, 0x29,
];

/// Verified Mojang `org.lwjgl:lwjgl-freetype:3.4.3-mojang-2.15.0` SHA-1 checksums from
/// official Mojang metadata (`26.3` & `26.4-pre-1`, FreeType 2.15.0+).
pub const VERIFIED_MOJANG_2_15_SHA1S: &[&str] = &[
    "134bc57b83e78a7f954a0f078093e319739d42ea", // lwjgl-freetype-3.4.3-mojang-2.15.0.jar
    "f85d0f5acaa594f9e93d657f8feff85d6a427b6a", // natives-windows
    "5f475c76490bb368f235017985d8d9dc6b5b5121", // natives-windows-arm64
    "f20ae220999240d484896d839694b02c2a655608", // natives-linux
    "6a46815640d9ce43533200986a330642a96e9590", // natives-macos
    "95ef1d3420179187f0b5e1ba947329b539e73412", // natives-macos-arm64
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLifecycleStage {
    Active,
    Deprecated,
    Retired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyValidity {
    Valid,
    Expired,
    SignatureInvalid,
    OfflineBaseline,
}

impl PolicyValidity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Valid => "Verified Signed Policy",
            Self::Expired => "Expired Signed Policy (Fallback Caution)",
            Self::SignatureInvalid => "Signature Rejected (Baseline Used)",
            Self::OfflineBaseline => "Embedded Baseline Policy",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedPolicyEnvelope {
    #[serde(default = "default_schema_v1")]
    pub schema_version: u32,
    #[serde(default)]
    pub key_id: String,
    pub payload_json: String,
    pub signature_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityPolicy {
    #[serde(default = "default_schema_v1")]
    pub schema_version: u32,
    pub policy_version: String,
    pub issued_at_epoch: u64,
    pub expires_at_epoch: u64,
    pub agent_lifecycle: AgentLifecycleStage,
    #[serde(default)]
    pub safe_freetype_version_markers: Vec<String>,
    #[serde(default)]
    pub safe_freetype_sha1s: Vec<String>,
    #[serde(default)]
    pub vulnerable_freetype_versions: Vec<String>,
    #[serde(default)]
    pub advisory_url: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedSecurityPolicy {
    pub policy: SecurityPolicy,
    pub validity: PolicyValidity,
    pub detail: String,
}

impl SecurityPolicy {
    /// Checks whether a Maven `org.lwjgl:lwjgl-freetype:<ver>` coordinate version is explicitly
    /// marked patched (e.g., `3.4.3-mojang-2.15.0` or FreeType `2.15.0+` LWJGL release).
    pub fn is_freetype_version_safe(&self, version: &str) -> bool {
        let lower = version.trim().to_ascii_lowercase();
        if lower.is_empty() {
            return false;
        }
        if self
            .vulnerable_freetype_versions
            .iter()
            .any(|v| lower == v.trim().to_ascii_lowercase())
        {
            return false;
        }
        self.safe_freetype_version_markers
            .iter()
            .any(|marker| lower.contains(&marker.trim().to_ascii_lowercase()))
    }

    /// Checks whether a library artifact SHA-1 is listed as a verified patched FreeType artifact.
    pub fn is_freetype_sha1_verified(&self, sha1: &str) -> bool {
        let lower = sha1.trim().to_ascii_lowercase();
        !lower.is_empty()
            && self
                .safe_freetype_sha1s
                .iter()
                .any(|known| known.trim().eq_ignore_ascii_case(&lower))
    }
}

fn default_schema_v1() -> u32 {
    1
}

pub fn unix_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub struct SecurityPolicyService;

impl SecurityPolicyService {
    pub fn cache_path(paths: &LauncherPaths) -> PathBuf {
        paths.cache_root.join("security").join("policy.json")
    }

    /// Deterministic offline baseline compiled into MCW Launcher, containing Mojang's verified
    /// `3.4.3-mojang-2.15.0` artifact SHA-1s (`26.3`, `26.4-pre-1`) as of 2026-11-25.
    pub fn embedded_baseline() -> ResolvedSecurityPolicy {
        ResolvedSecurityPolicy {
            policy: SecurityPolicy {
                schema_version: 1,
                policy_version: "2026.11.25-baseline".to_string(),
                issued_at_epoch: 1_764_000_000,
                expires_at_epoch: u64::MAX,
                agent_lifecycle: AgentLifecycleStage::Active,
                safe_freetype_version_markers: vec![
                    "-mojang-2.15.".to_string(),
                    "2.15.".to_string(),
                    "3.4.4".to_string(),
                    "3.5.".to_string(),
                ],
                safe_freetype_sha1s: VERIFIED_MOJANG_2_15_SHA1S
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect(),
                vulnerable_freetype_versions: vec![
                    "3.3.3".to_string(),
                    "3.3.4".to_string(),
                    "3.3.6".to_string(),
                    "3.4.0".to_string(),
                    "3.4.1".to_string(),
                    "3.4.2".to_string(),
                    "3.4.3".to_string(),
                ],
                advisory_url: "https://github.com/bikini/minecraft2shell".to_string(),
            },
            validity: PolicyValidity::OfflineBaseline,
            detail:
                "Using built-in offline security baseline (Mojang 3.4.3-mojang-2.15.0 verified)."
                    .to_string(),
        }
    }

    /// Verifies a signed Ed25519 envelope and evaluates expiration without executing any remote instructions.
    pub fn verify_signed_envelope(
        envelope: &SignedPolicyEnvelope,
        public_key_bytes: &[u8],
        now_epoch: u64,
    ) -> Result<ResolvedSecurityPolicy> {
        let sig_bytes = BASE64_STANDARD
            .decode(envelope.signature_base64.trim())
            .map_err(|e| {
                MinecraftError::InvalidMetadata(format!("Invalid policy signature encoding: {e}"))
            })?;
        let verifier = UnparsedPublicKey::new(&ED25519, public_key_bytes);
        verifier
            .verify(envelope.payload_json.as_bytes(), &sig_bytes)
            .map_err(|_| {
                MinecraftError::InvalidMetadata(
                    "Remote security policy Ed25519 signature verification failed".to_string(),
                )
            })?;

        let policy: SecurityPolicy = serde_json::from_str(&envelope.payload_json)?;
        if now_epoch > policy.expires_at_epoch {
            return Ok(ResolvedSecurityPolicy {
                validity: PolicyValidity::Expired,
                detail: format!(
                    "Signed policy '{}' expired at epoch {}; verify connection for fresh security metadata.",
                    policy.policy_version, policy.expires_at_epoch
                ),
                policy,
            });
        }

        Ok(ResolvedSecurityPolicy {
            detail: format!(
                "Verified signed security policy '{}' (key: {}).",
                policy.policy_version,
                if envelope.key_id.trim().is_empty() {
                    "default"
                } else {
                    envelope.key_id.trim()
                }
            ),
            policy,
            validity: PolicyValidity::Valid,
        })
    }

    /// Loads cached signed policy if valid; otherwise safely returns the embedded baseline.
    pub fn load_cached(paths: &LauncherPaths) -> ResolvedSecurityPolicy {
        let path = Self::cache_path(paths);
        let Ok(raw) = fs::read_to_string(&path) else {
            return Self::embedded_baseline();
        };
        let Ok(envelope) = serde_json::from_str::<SignedPolicyEnvelope>(&raw) else {
            let mut base = Self::embedded_baseline();
            base.detail =
                "Cached security policy was malformed; fell back cleanly to embedded baseline."
                    .to_string();
            return base;
        };

        match Self::verify_signed_envelope(
            &envelope,
            &EMBEDDED_POLICY_ED25519_PUBKEY,
            unix_epoch_secs(),
        ) {
            Ok(resolved) => resolved,
            Err(_) => {
                let mut base = Self::embedded_baseline();
                base.validity = PolicyValidity::SignatureInvalid;
                base.detail =
                    "Cached security policy failed Ed25519 signature verification and was ignored; using embedded baseline."
                        .to_string();
                base
            }
        }
    }

    /// Fetches, verifies, and caches a signed remote security policy.
    /// Rejects tampered/unsigned responses and degrades cleanly when offline.
    pub async fn refresh_from_remote(
        paths: &LauncherPaths,
        client: &DownloadClient,
        url: &str,
    ) -> ResolvedSecurityPolicy {
        let fallback = Self::load_cached(paths);
        let raw = match client.get_text(url).await {
            Ok(txt) => txt,
            Err(err) => {
                let mut out = fallback;
                out.detail = format!(
                    "Offline or policy endpoint unreachable ({err}); {}.",
                    out.detail
                );
                return out;
            }
        };

        let Ok(envelope) = serde_json::from_str::<SignedPolicyEnvelope>(&raw) else {
            let mut out = fallback;
            out.validity = PolicyValidity::SignatureInvalid;
            out.detail = "Remote security policy payload was not a valid signed envelope; kept local baseline.".to_string();
            return out;
        };

        match Self::verify_signed_envelope(
            &envelope,
            &EMBEDDED_POLICY_ED25519_PUBKEY,
            unix_epoch_secs(),
        ) {
            Ok(resolved) => {
                if resolved.validity == PolicyValidity::Valid {
                    let path = Self::cache_path(paths);
                    if let Some(parent) = path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    if let Ok(pretty) = serde_json::to_string_pretty(&envelope) {
                        let _ = atomic_write_text(&path, &pretty);
                    }
                }
                resolved
            }
            Err(_) => {
                let mut out = fallback;
                out.validity = PolicyValidity::SignatureInvalid;
                out.detail =
                    "Rejected remote security policy with invalid Ed25519 signature; kept local baseline."
                        .to_string();
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use tempfile::tempdir;

    #[test]
    fn verifies_signed_ed25519_policy_rejects_tamper_and_detects_expiry() {
        let rng = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let pubkey = key_pair.public_key().as_ref();

        let policy = SecurityPolicyService::embedded_baseline().policy;
        let payload_json = serde_json::to_string(&policy).unwrap();
        let sig = key_pair.sign(payload_json.as_bytes());
        let envelope = SignedPolicyEnvelope {
            schema_version: 1,
            key_id: "test-key-1".to_string(),
            payload_json,
            signature_base64: BASE64_STANDARD.encode(sig.as_ref()),
        };

        let verified =
            SecurityPolicyService::verify_signed_envelope(&envelope, pubkey, 1_764_000_500)
                .unwrap();
        assert_eq!(verified.validity, PolicyValidity::Valid);
        assert!(verified
            .policy
            .is_freetype_version_safe("3.4.3-mojang-2.15.0"));
        assert!(!verified.policy.is_freetype_version_safe("3.3.3"));
        assert!(!verified.policy.is_freetype_version_safe("3.4.1"));

        // Tamper payload -> must be rejected
        let mut tampered = envelope.clone();
        tampered.payload_json = tampered
            .payload_json
            .replace("2026.11.25-baseline", "2026.99.99-evil");
        assert!(
            SecurityPolicyService::verify_signed_envelope(&tampered, pubkey, 1_764_000_500)
                .is_err()
        );

        // Expired policy -> flagged PolicyValidity::Expired
        let mut exp_policy = policy;
        exp_policy.expires_at_epoch = 1_764_000_100;
        let exp_json = serde_json::to_string(&exp_policy).unwrap();
        let exp_sig = key_pair.sign(exp_json.as_bytes());
        let exp_env = SignedPolicyEnvelope {
            schema_version: 1,
            key_id: "test-key-1".to_string(),
            payload_json: exp_json,
            signature_base64: BASE64_STANDARD.encode(exp_sig.as_ref()),
        };
        let expired =
            SecurityPolicyService::verify_signed_envelope(&exp_env, pubkey, 1_764_000_500).unwrap();
        assert_eq!(expired.validity, PolicyValidity::Expired);
    }

    #[test]
    fn corrupt_cached_policy_never_crashes_and_falls_back_safely() {
        let tmp = tempdir().unwrap();
        let paths = LauncherPaths::portable(tmp.path());
        paths.initialize().unwrap();

        let cache = SecurityPolicyService::cache_path(&paths);
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(
            &cache,
            r#"{"schema_version":1,"key_id":"k","payload_json":"{}","signature_base64":"AAAA"}"#,
        )
        .unwrap();

        let loaded = SecurityPolicyService::load_cached(&paths);
        assert_eq!(loaded.validity, PolicyValidity::SignatureInvalid);
    }
}
