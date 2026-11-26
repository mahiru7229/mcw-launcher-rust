use crate::libraries::LibraryManager;
use crate::manifest::VersionManifestService;
use crate::rules::LibraryRules;
use crate::security_policy::{AgentLifecycleStage, PolicyValidity, ResolvedSecurityPolicy};
use crate::{MinecraftError, Result};
use mcw_models::minecraft::{LibraryEntry, VersionManifestEntry, VersionMetadata};
use mcw_net::{DownloadClient, DownloadRequest};
use mcw_runtime::{SecurityAgentManager, SecurityAgentRuntimeState};
use mcw_storage::{atomic_write_text, LauncherPaths};
use sha1::{Digest, Sha1};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityStatus {
    Protected,
    PartiallyProtected,
    Unprotected,
    Unknown,
}

impl SecurityStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Protected => "PROTECTED",
            Self::PartiallyProtected => "PARTIALLY_PROTECTED",
            Self::Unprotected => "UNPROTECTED",
            Self::Unknown => "UNKNOWN",
        }
    }

    pub fn display_title(self) -> &'static str {
        match self {
            Self::Protected => "PROTECTED",
            Self::PartiallyProtected => "PARTIALLY PROTECTED",
            Self::Unprotected => "UNPROTECTED",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreetypeLibraryStatus {
    NotUsed,
    PatchedVerified {
        version: String,
        verified_artifacts: usize,
    },
    Vulnerable {
        version: String,
    },
    MissingOrCorrupt {
        version: String,
        detail: String,
    },
    Unknown,
}

#[derive(Debug, Clone)]
pub struct InstanceSecurityAssessment {
    pub status: SecurityStatus,
    pub status_label: String,
    pub headline: String,
    pub description: String,
    pub freetype_status: FreetypeLibraryStatus,
    pub should_attach_agent: bool,
    pub agent_runtime_state: SecurityAgentRuntimeState,
    pub policy_validity: PolicyValidity,
    pub reasons: Vec<String>,
    pub recommended_action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibrarySyncOutcome {
    AlreadyUpToDate {
        version_id: String,
        description: String,
    },
    UpdatedPatchedFreetype {
        version_id: String,
        new_freetype_version: String,
        description: String,
    },
    NoPatchAvailable {
        version_id: String,
        current_freetype: Option<String>,
        description: String,
    },
    OfflineSkipped {
        version_id: String,
        description: String,
    },
    RolledBack {
        version_id: String,
        reason: String,
    },
}

pub struct SecurityManager;

impl SecurityManager {
    /// Evaluates an instance's real merged dependencies, on-disk library checksums,
    /// Java runtime compatibility, agent runtime status, and signed policy.
    ///
    /// Never assumes safety from Minecraft's version string alone and always produces
    /// a detailed human-readable description alongside the status badge.
    pub fn assess_instance(
        paths: &LauncherPaths,
        merged_metadata: &VersionMetadata,
        instance_id: &str,
        agent_enabled: bool,
        java_major: u32,
        resolved_policy: &ResolvedSecurityPolicy,
    ) -> InstanceSecurityAssessment {
        let client = DownloadClient::new();
        let instance_dir = paths.instance_dir(instance_id);
        let runtime_report = SecurityAgentManager::read_runtime_status(&instance_dir);
        let runtime_state = runtime_report
            .as_ref()
            .map(|r| r.state)
            .unwrap_or(SecurityAgentRuntimeState::NotLoaded);
        let _ = client;

        let freetype_libs: Vec<&LibraryEntry> = merged_metadata
            .libraries
            .iter()
            .filter(|lib| {
                LibraryRules::is_allowed(&lib.rules)
                    && lib.clientreq != Some(false)
                    && is_freetype_coordinate(&lib.name)
            })
            .collect();

        let mut reasons = Vec::new();
        if resolved_policy.validity != PolicyValidity::Valid {
            reasons.push(resolved_policy.detail.clone());
        }

        if freetype_libs.is_empty() {
            return InstanceSecurityAssessment {
                status: SecurityStatus::Protected,
                status_label: SecurityStatus::Protected.as_str().to_string(),
                headline: "Protected — No FreeType Dependency".to_string(),
                description: format!(
                    "Resolved dependencies for '{}' do not use org.lwjgl:lwjgl-freetype (classic STB font path). MCW Security Agent is unnecessary and automatically omitted.",
                    merged_metadata.id
                ),
                freetype_status: FreetypeLibraryStatus::NotUsed,
                should_attach_agent: false,
                agent_runtime_state: SecurityAgentRuntimeState::NotLoaded,
                policy_validity: resolved_policy.validity,
                reasons,
                recommended_action: None,
            };
        }

        let detected_version = freetype_libs
            .iter()
            .filter_map(|lib| extract_maven_version(&lib.name))
            .next()
            .unwrap_or_else(|| "unknown".to_string());

        let libs_dir = paths.libraries_dir();

        let mut all_safe_by_policy = true;
        let mut all_present_and_checksum_ok = true;
        let mut verified_artifacts = 0usize;
        let mut missing_details = Vec::new();
        let current_os = LibraryRules::current_os();
        let arch_bits = LibraryRules::current_arch_bits();

        for lib in &freetype_libs {
            let ver = extract_maven_version(&lib.name).unwrap_or_default();
            let mut lib_safe = resolved_policy.policy.is_freetype_version_safe(&ver);

            if let Some(ref downloads) = lib.downloads {
                if let Some(ref art) = downloads.artifact {
                    match LibraryManager::verify_artifact_on_disk_at(&libs_dir, art) {
                        Ok(true) => {
                            verified_artifacts += 1;
                            if let Some(ref s) = art.sha1 {
                                if resolved_policy.policy.is_freetype_sha1_verified(s) {
                                    lib_safe = true;
                                }
                            }
                        }
                        Ok(false) => {
                            all_present_and_checksum_ok = false;
                            missing_details.push(format!(
                                "{} (missing or SHA-1 mismatch)",
                                art.path.as_deref().unwrap_or(&lib.name)
                            ));
                        }
                        Err(e) => {
                            all_present_and_checksum_ok = false;
                            missing_details.push(format!("{}: {e}", lib.name));
                        }
                    }
                }
                if let Some(ref natives_map) = lib.natives {
                    if let Some(template) = natives_map.get(current_os) {
                        let classifier = template.replace("${arch}", arch_bits);
                        if let Some(nat) = downloads.classifiers.get(&classifier) {
                            match LibraryManager::verify_artifact_on_disk_at(&libs_dir, nat) {
                                Ok(true) => {
                                    verified_artifacts += 1;
                                    if let Some(ref s) = nat.sha1 {
                                        if resolved_policy.policy.is_freetype_sha1_verified(s) {
                                            lib_safe = true;
                                        }
                                    }
                                }
                                Ok(false) => {
                                    all_present_and_checksum_ok = false;
                                    missing_details.push(format!(
                                        "{} (native missing or SHA-1 mismatch)",
                                        nat.path.as_deref().unwrap_or(&lib.name)
                                    ));
                                }
                                Err(e) => {
                                    all_present_and_checksum_ok = false;
                                    missing_details.push(format!("{}: {e}", lib.name));
                                }
                            }
                        }
                    }
                }
            } else {
                all_present_and_checksum_ok = false;
                missing_details.push(format!("{} (no artifact metadata)", lib.name));
            }

            if !lib_safe {
                all_safe_by_policy = false;
            }
        }

        if all_safe_by_policy && all_present_and_checksum_ok && verified_artifacts > 0 {
            if resolved_policy.validity == PolicyValidity::Expired {
                reasons.push(
                    "Local FreeType 2.15.0+ checksums matched baseline, though remote policy is expired."
                        .to_string(),
                );
            }
            return InstanceSecurityAssessment {
                status: SecurityStatus::Protected,
                status_label: SecurityStatus::Protected.as_str().to_string(),
                headline: format!("Protected — Verified Patched FreeType ({detected_version})"),
                description: format!(
                    "Verified {verified_artifacts} on-disk artifact/native checksum(s) for org.lwjgl:lwjgl-freetype:{detected_version} (FreeType 2.15.0+). The Minecraft2Shell (Ins_SHZ) bug is fixed natively; temporary MCW Security Agent is automatically disabled."
                ),
                freetype_status: FreetypeLibraryStatus::PatchedVerified {
                    version: detected_version,
                    verified_artifacts,
                },
                should_attach_agent: false,
                agent_runtime_state: SecurityAgentRuntimeState::NotLoaded,
                policy_validity: resolved_policy.validity,
                reasons,
                recommended_action: None,
            };
        }

        // If files claim a safe version OR are missing/corrupt on disk, never claim Protected!
        if !all_present_and_checksum_ok && verified_artifacts == 0 {
            let detail = missing_details.join("; ");
            let lifecycle_allows =
                resolved_policy.policy.agent_lifecycle != AgentLifecycleStage::Retired;
            let java_ok = SecurityAgentManager::check_java_compatibility(java_major).is_ok();
            reasons.push(format!("Unverified FreeType artifacts: {detail}"));
            return InstanceSecurityAssessment {
                status: SecurityStatus::Unknown,
                status_label: SecurityStatus::Unknown.as_str().to_string(),
                headline: format!("Unknown — Missing or Unverified FreeType ({detected_version})"),
                description: format!(
                    "Dependency org.lwjgl:lwjgl-freetype:{detected_version} is not yet present or failed SHA-1 verification on disk ({detail}). MCW cannot confirm library safety until files are downloaded and verified."
                ),
                freetype_status: FreetypeLibraryStatus::MissingOrCorrupt {
                    version: detected_version,
                    detail,
                },
                should_attach_agent: agent_enabled && lifecycle_allows && java_ok,
                agent_runtime_state: runtime_state,
                policy_validity: resolved_policy.validity,
                reasons,
                recommended_action: Some(
                    "Sync Mojang Security Libraries or launch once to download and verify library checksums."
                        .to_string(),
                ),
            };
        }

        // Unpatched FreeType version detected (`3.3.3`, `3.4.1`, etc.)
        let lifecycle_allows =
            resolved_policy.policy.agent_lifecycle != AgentLifecycleStage::Retired;
        let java_compat = SecurityAgentManager::check_java_compatibility(java_major);

        if let Some(ref rt) = runtime_report {
            match rt.state {
                SecurityAgentRuntimeState::Failed | SecurityAgentRuntimeState::Incompatible => {
                    reasons.push(format!("Agent reported {:?}: {}", rt.state, rt.detail));
                    return InstanceSecurityAssessment {
                        status: SecurityStatus::Unprotected,
                        status_label: SecurityStatus::Unprotected.as_str().to_string(),
                        headline: format!(
                            "Unprotected — Security Agent Failed ({detected_version})"
                        ),
                        description: format!(
                            "org.lwjgl:lwjgl-freetype:{detected_version} is vulnerable to Minecraft2Shell (FreeType < 2.15.0) and MCW Security Agent reported an error during runtime instrumentation: {}. Avoid untrusted servers or resource packs.",
                            rt.detail
                        ),
                        freetype_status: FreetypeLibraryStatus::Vulnerable {
                            version: detected_version,
                        },
                        should_attach_agent: agent_enabled && lifecycle_allows && java_compat.is_ok(),
                        agent_runtime_state: rt.state,
                        policy_validity: resolved_policy.validity,
                        reasons,
                        recommended_action: Some(
                            "Verify Java 8+ runtime, update MCW Launcher, and keep untrusted server resource packs disabled."
                                .to_string(),
                        ),
                    };
                }
                _ => {}
            }
        }

        if !agent_enabled || !lifecycle_allows {
            let reason = if !agent_enabled {
                "MCW Security Agent is disabled in Launcher Settings -> Security.".to_string()
            } else {
                "MCW Security Agent is marked retired by policy.".to_string()
            };
            reasons.push(reason.clone());
            return InstanceSecurityAssessment {
                status: SecurityStatus::Unprotected,
                status_label: SecurityStatus::Unprotected.as_str().to_string(),
                headline: format!(
                    "Unprotected — Vulnerable FreeType ({detected_version})"
                ),
                description: format!(
                    "Resolved library org.lwjgl:lwjgl-freetype:{detected_version} contains the FreeType < 2.15.0 Ins_SHZ RCE flaw, and runtime protection is OFF ({reason}). Malicious server or local .ttf resource packs can execute native code."
                ),
                freetype_status: FreetypeLibraryStatus::Vulnerable {
                    version: detected_version,
                },
                should_attach_agent: false,
                agent_runtime_state: runtime_state,
                policy_validity: resolved_policy.validity,
                reasons,
                recommended_action: Some(
                    "Enable Minecraft2Shell mitigation in Launcher Settings -> Security or upgrade to a patched Minecraft release (26.3+)."
                        .to_string(),
                ),
            };
        }

        if let Err(err) = java_compat {
            reasons.push(err.clone());
            return InstanceSecurityAssessment {
                status: SecurityStatus::Unprotected,
                status_label: SecurityStatus::Unprotected.as_str().to_string(),
                headline: format!("Unprotected — Incompatible Java Runtime ({java_major})"),
                description: format!(
                    "org.lwjgl:lwjgl-freetype:{detected_version} is vulnerable and the configured Java runtime cannot load MCW Security Agent ({err})."
                ),
                freetype_status: FreetypeLibraryStatus::Vulnerable {
                    version: detected_version,
                },
                should_attach_agent: false,
                agent_runtime_state: SecurityAgentRuntimeState::Incompatible,
                policy_validity: resolved_policy.validity,
                reasons,
                recommended_action: Some("Switch this instance to Java 8 or newer.".to_string()),
            };
        }

        let runtime_note = match runtime_report.as_ref().map(|r| (r.state, r.patched_methods)) {
            Some((SecurityAgentRuntimeState::ActiveVerified, count)) => format!(
                "Last run verified {count} FreeType method(s) patched (ACTIVE_VERIFIED -> FT_LOAD_NO_HINTING)."
            ),
            Some((SecurityAgentRuntimeState::Initialized, _)) => {
                "Agent initialized in JVM; waiting for FreeType class load to confirm patching (INITIALIZED only is not treated as runtime proof)."
                    .to_string()
            }
            _ => "MCW Security Agent will attach at launch and enforce FT_LOAD_NO_HINTING (Ins_SHZ bytecode interpreter blocked) while awaiting an official Mojang FreeType 2.15.0+ update for this version."
                .to_string(),
        };

        reasons.push(format!(
            "Using unpatched org.lwjgl:lwjgl-freetype:{detected_version}."
        ));
        reasons.push(runtime_note.clone());

        InstanceSecurityAssessment {
            status: SecurityStatus::PartiallyProtected,
            status_label: SecurityStatus::PartiallyProtected.as_str().to_string(),
            headline: format!(
                "Partially Protected — Runtime Hinting Guard ({detected_version})"
            ),
            description: format!(
                "This instance uses unpatched org.lwjgl:lwjgl-freetype:{detected_version} (FreeType < 2.15.0). {runtime_note} Keep Server Resource Packs cautious until Mojang ships FreeType 2.15.0+ for this Minecraft branch."
            ),
            freetype_status: FreetypeLibraryStatus::Vulnerable {
                version: detected_version,
            },
            should_attach_agent: true,
            agent_runtime_state: runtime_state,
            policy_validity: resolved_policy.validity,
            reasons,
            recommended_action: Some(
                "Avoid untrusted Server Resource Packs, or sync/upgrade to Minecraft 26.3+ with Mojang FreeType 2.15.0+."
                    .to_string(),
            ),
        }
    }
}

pub struct MojangLibrarySync;

impl MojangLibrarySync {
    /// Refreshes Mojang's official manifest and safely updates ONLY `org.lwjgl:lwjgl-freetype`
    /// artifacts & `version.json` if Mojang has shipped a verified patched version (`2.15.0+`),
    /// with full checksum verification and automatic rollback on failure.
    pub async fn sync_freetype_for_version(
        paths: &LauncherPaths,
        client: &DownloadClient,
        base_version_id: &str,
        resolved_policy: &ResolvedSecurityPolicy,
    ) -> Result<LibrarySyncOutcome> {
        let manifest_svc = VersionManifestService::new(paths, client);
        let entry = match manifest_svc.find_version(base_version_id, true).await {
            Ok(e) => e,
            Err(MinecraftError::Net(err)) => {
                return Ok(LibrarySyncOutcome::OfflineSkipped {
                    version_id: base_version_id.to_string(),
                    description: format!(
                        "Offline or Mojang manifest unreachable ({err}); keeping cached libraries intact."
                    ),
                });
            }
            Err(MinecraftError::VersionNotFound(_)) => {
                return Ok(LibrarySyncOutcome::NoPatchAvailable {
                    version_id: base_version_id.to_string(),
                    current_freetype: None,
                    description: format!(
                        "Version '{base_version_id}' is not an official Mojang manifest entry; skipping upstream FreeType sync."
                    ),
                });
            }
            Err(e) => return Err(e),
        };

        Self::sync_from_manifest_entry(paths, client, &entry, resolved_policy).await
    }

    pub async fn sync_from_manifest_entry(
        paths: &LauncherPaths,
        client: &DownloadClient,
        entry: &VersionManifestEntry,
        resolved_policy: &ResolvedSecurityPolicy,
    ) -> Result<LibrarySyncOutcome> {
        let version_json_path = paths.version_json(&entry.id);
        let staging_dir = paths.cache_root.join("security").join("sync_staging");
        fs::create_dir_all(&staging_dir)?;
        let candidate_json_path = staging_dir.join(format!("{}.candidate.json", entry.id));
        let _ = fs::remove_file(&candidate_json_path);

        // If local version.json already matches Mojang manifest SHA-1, check if local FreeType
        // artifacts are intact before deciding everything is up to date.
        let local_matches_manifest = if version_json_path.is_file() {
            match entry
                .sha1
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                Some(expected) => file_sha1_hex(&version_json_path)
                    .map(|actual| actual.eq_ignore_ascii_case(expected))
                    .unwrap_or(false),
                None => false,
            }
        } else {
            false
        };

        if local_matches_manifest {
            if let Ok(raw) = fs::read_to_string(&version_json_path) {
                if let Ok(meta) = serde_json::from_str::<VersionMetadata>(&raw) {
                    let lib_mgr = LibraryManager::new(paths, client);
                    let ft_libs: Vec<&LibraryEntry> = meta
                        .libraries
                        .iter()
                        .filter(|l| {
                            LibraryRules::is_allowed(&l.rules)
                                && l.clientreq != Some(false)
                                && is_freetype_coordinate(&l.name)
                        })
                        .collect();
                    let all_ok = ft_libs.iter().all(|lib| verify_lib_entry(&lib_mgr, lib));
                    if all_ok {
                        return Ok(LibrarySyncOutcome::AlreadyUpToDate {
                            version_id: entry.id.clone(),
                            description: format!(
                                "Version '{}' metadata matches Mojang manifest SHA-1 and all FreeType artifacts are verified on disk.",
                                entry.id
                            ),
                        });
                    }
                }
            }
        }

        // Download candidate metadata to staging with SHA-1 verification before modifying anything.
        let meta_req = DownloadRequest::new(&entry.url, &candidate_json_path)
            .with_sha1(entry.sha1.clone())
            .with_size(entry.size.unwrap_or(0));
        if let Err(err) = client.download_file(&meta_req).await {
            let _ = fs::remove_file(&candidate_json_path);
            return Ok(LibrarySyncOutcome::OfflineSkipped {
                version_id: entry.id.clone(),
                description: format!(
                    "Could not fetch or verify Mojang metadata for '{}': {err}. Existing files unchanged.",
                    entry.id
                ),
            });
        }

        let candidate_raw = fs::read_to_string(&candidate_json_path)?;
        let candidate_meta: VersionMetadata = serde_json::from_str(&candidate_raw)?;
        let candidate_ft: Vec<&LibraryEntry> = candidate_meta
            .libraries
            .iter()
            .filter(|l| {
                LibraryRules::is_allowed(&l.rules)
                    && l.clientreq != Some(false)
                    && is_freetype_coordinate(&l.name)
            })
            .collect();

        if candidate_ft.is_empty() {
            let _ = fs::remove_file(&candidate_json_path);
            return Ok(LibrarySyncOutcome::AlreadyUpToDate {
                version_id: entry.id.clone(),
                description: format!(
                    "Version '{}' does not use lwjgl-freetype (not vulnerable to Minecraft2Shell).",
                    entry.id
                ),
            });
        }

        let candidate_ver = candidate_ft
            .iter()
            .filter_map(|l| extract_maven_version(&l.name))
            .next()
            .unwrap_or_else(|| "unknown".to_string());

        let has_verified_patch = candidate_ft.iter().any(|lib| {
            let ver = extract_maven_version(&lib.name).unwrap_or_default();
            let sha_ok = lib
                .downloads
                .as_ref()
                .and_then(|d| d.artifact.as_ref())
                .and_then(|a| a.sha1.as_deref())
                .is_some_and(|s| resolved_policy.policy.is_freetype_sha1_verified(s));
            resolved_policy.policy.is_freetype_version_safe(&ver) || sha_ok
        });

        if !has_verified_patch {
            let _ = fs::remove_file(&candidate_json_path);
            return Ok(LibrarySyncOutcome::NoPatchAvailable {
                version_id: entry.id.clone(),
                current_freetype: Some(candidate_ver.clone()),
                description: format!(
                    "Mojang metadata for '{}' still specifies unpatched lwjgl-freetype:{candidate_ver}. Keeping existing libraries; MCW Security Agent provides runtime hinting mitigation.",
                    entry.id
                ),
            });
        }

        // Snapshot backup of existing version.json and natives/<id>/ before applying targeted sync.
        let natives_dir = paths.natives_dir(&entry.id);
        let backup_dir = staging_dir.join(format!("backup_{}", entry.id));
        if backup_dir.exists() {
            let _ = fs::remove_dir_all(&backup_dir);
        }
        fs::create_dir_all(&backup_dir)?;

        let backup_json = backup_dir.join("version.json.bak");
        let had_version_json = version_json_path.is_file();
        if had_version_json {
            fs::copy(&version_json_path, &backup_json)?;
        }
        let backup_natives = backup_dir.join("natives");
        let had_natives = natives_dir.is_dir();
        if had_natives {
            copy_dir_recursive(&natives_dir, &backup_natives)?;
        }

        // Transactional replacement of ONLY lwjgl-freetype artifacts + natives + version.json.
        let apply_result: Result<()> = async {
            let libs_dir = paths.libraries_dir();
            let current_os = LibraryRules::current_os();
            let arch_bits = LibraryRules::current_arch_bits();
            let mut native_jars_to_extract: Vec<(PathBuf, String)> = Vec::new();

            for lib in &candidate_ft {
                let Some(ref downloads) = lib.downloads else {
                    continue;
                };
                if let Some(ref art) = downloads.artifact {
                    if let Some(ref rel) = art.path {
                        if !art.url.trim().is_empty() {
                            let dest = LauncherPaths::safe_join_relative(&libs_dir, rel)?;
                            let req = DownloadRequest::new(&art.url, &dest)
                                .with_sha1(art.sha1.clone())
                                .with_size(art.size);
                            client.download_file(&req).await?;
                        }
                    }
                }
                if let Some(ref natives_map) = lib.natives {
                    if let Some(template) = natives_map.get(current_os) {
                        let classifier = template.replace("${arch}", arch_bits);
                        if let Some(nat) = downloads.classifiers.get(&classifier) {
                            if let Some(ref rel) = nat.path {
                                if !nat.url.trim().is_empty() {
                                    let dest = LauncherPaths::safe_join_relative(&libs_dir, rel)?;
                                    let req = DownloadRequest::new(&nat.url, &dest)
                                        .with_sha1(nat.sha1.clone())
                                        .with_size(nat.size);
                                    client.download_file(&req).await?;
                                    let marker = nat
                                        .sha1
                                        .clone()
                                        .filter(|s| !s.trim().is_empty())
                                        .unwrap_or_else(|| rel.replace(['/', '\\'], "_"));
                                    native_jars_to_extract.push((dest, marker));
                                }
                            }
                        }
                    }
                }
            }

            if !native_jars_to_extract.is_empty() {
                fs::create_dir_all(&natives_dir)?;
                LibraryManager::clean_freetype_natives(&natives_dir)?;
                for (jar_path, marker) in native_jars_to_extract {
                    LibraryManager::extract_native(&jar_path, &natives_dir, &marker)?;
                }
            }

            if let Some(parent) = version_json_path.parent() {
                fs::create_dir_all(parent)?;
            }
            atomic_write_text(&version_json_path, &candidate_raw)?;
            Ok(())
        }
        .await;

        let _ = fs::remove_file(&candidate_json_path);

        match apply_result {
            Ok(()) => {
                let _ = fs::remove_dir_all(&backup_dir);
                Ok(LibrarySyncOutcome::UpdatedPatchedFreetype {
                    version_id: entry.id.clone(),
                    new_freetype_version: candidate_ver.clone(),
                    description: format!(
                        "Updated '{}' to Mojang verified lwjgl-freetype:{candidate_ver} (FreeType 2.15.0+) with SHA-1 verification.",
                        entry.id
                    ),
                })
            }
            Err(err) => {
                // Automatic transactional rollback so existing instances are never broken.
                if had_version_json && backup_json.is_file() {
                    let _ = fs::copy(&backup_json, &version_json_path);
                } else {
                    let _ = fs::remove_file(&version_json_path);
                }
                if had_natives && backup_natives.is_dir() {
                    let _ = fs::remove_dir_all(&natives_dir);
                    let _ = copy_dir_recursive(&backup_natives, &natives_dir);
                } else if natives_dir.is_dir() {
                    let _ = fs::remove_dir_all(&natives_dir);
                }
                let _ = fs::remove_dir_all(&backup_dir);
                Ok(LibrarySyncOutcome::RolledBack {
                    version_id: entry.id.clone(),
                    reason: format!(
                        "FreeType sync failed ({err}); rolled back '{}' metadata and natives cleanly.",
                        entry.id
                    ),
                })
            }
        }
    }
}

fn verify_lib_entry(lib_mgr: &LibraryManager<'_>, lib: &LibraryEntry) -> bool {
    let Some(ref downloads) = lib.downloads else {
        return false;
    };
    if let Some(ref art) = downloads.artifact {
        if !lib_mgr.verify_artifact_on_disk(art).unwrap_or(false) {
            return false;
        }
    }
    let current_os = LibraryRules::current_os();
    let arch_bits = LibraryRules::current_arch_bits();
    if let Some(ref natives_map) = lib.natives {
        if let Some(template) = natives_map.get(current_os) {
            let classifier = template.replace("${arch}", arch_bits);
            if let Some(nat) = downloads.classifiers.get(&classifier) {
                if !lib_mgr.verify_artifact_on_disk(nat).unwrap_or(false) {
                    return false;
                }
            }
        }
    }
    true
}

fn is_freetype_coordinate(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    lower.starts_with("org.lwjgl:lwjgl-freetype:") || lower.contains("org.lwjgl:lwjgl-freetype")
}

fn extract_maven_version(coord: &str) -> Option<String> {
    let mut parts = coord.trim().split(':');
    let _group = parts.next()?;
    let _artifact = parts.next()?;
    let ver = parts.next()?.trim();
    if ver.is_empty() {
        None
    } else {
        Some(ver.to_string())
    }
}

fn file_sha1_hex(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha1::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(40);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    Ok(out)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security_policy::SecurityPolicyService;
    use mcw_models::minecraft::{DownloadArtifact, LibraryDownloads};
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use tempfile::tempdir;

    fn sha1_hex_bytes(data: &[u8]) -> String {
        let mut hasher = Sha1::new();
        hasher.update(data);
        let digest = hasher.finalize();
        let mut out = String::with_capacity(40);
        for b in digest {
            out.push_str(&format!("{b:02x}"));
        }
        out
    }

    fn make_metadata(id: &str, libs: Vec<LibraryEntry>) -> VersionMetadata {
        VersionMetadata {
            id: id.to_string(),
            version_type: "release".to_string(),
            main_class: "net.minecraft.client.main.Main".to_string(),
            inherits_from: None,
            asset_index: None,
            assets: None,
            downloads: None,
            libraries: libs,
            java_version: None,
            arguments: None,
            minecraft_arguments: None,
        }
    }

    fn make_freetype_lib(
        paths: &LauncherPaths,
        ver: &str,
        payload: &[u8],
        write_disk: bool,
    ) -> LibraryEntry {
        let rel = format!("org/lwjgl/lwjgl-freetype/{ver}/lwjgl-freetype-{ver}.jar");
        let sha1 = sha1_hex_bytes(payload);
        if write_disk {
            let dest = LauncherPaths::safe_join_relative(&paths.libraries_dir(), &rel).unwrap();
            fs::create_dir_all(dest.parent().unwrap()).unwrap();
            fs::write(&dest, payload).unwrap();
        }
        LibraryEntry {
            name: format!("org.lwjgl:lwjgl-freetype:{ver}"),
            url: None,
            downloads: Some(LibraryDownloads {
                artifact: Some(DownloadArtifact {
                    path: Some(rel),
                    sha1: Some(sha1),
                    size: payload.len() as u64,
                    url: "https://libraries.minecraft.net/fake.jar".to_string(),
                }),
                classifiers: HashMap::new(),
            }),
            rules: vec![],
            natives: None,
            clientreq: None,
        }
    }

    #[test]
    fn assesses_unaffected_pre_1_20_5_as_protected() {
        let tmp = tempdir().unwrap();
        let paths = LauncherPaths::portable(tmp.path());
        paths.initialize().unwrap();
        let policy = SecurityPolicyService::embedded_baseline();

        let meta = make_metadata("1.20.4", vec![]);
        let res =
            SecurityManager::assess_instance(&paths, &meta, "vanilla_1204", true, 17, &policy);
        assert_eq!(res.status, SecurityStatus::Protected);
        assert!(!res.should_attach_agent);
        assert!(!res.description.is_empty());
    }

    #[test]
    fn assesses_verified_mojang_2_15_as_protected_without_agent() {
        let tmp = tempdir().unwrap();
        let paths = LauncherPaths::portable(tmp.path());
        paths.initialize().unwrap();
        let policy = SecurityPolicyService::embedded_baseline();

        let lib = make_freetype_lib(&paths, "3.4.3-mojang-2.15.0", b"patched-jar", true);
        let meta = make_metadata("26.3", vec![lib]);
        let res = SecurityManager::assess_instance(&paths, &meta, "mc26_3", true, 21, &policy);
        assert_eq!(res.status, SecurityStatus::Protected);
        assert!(!res.should_attach_agent);
        assert!(res.description.contains("3.4.3-mojang-2.15.0"));
    }

    #[test]
    fn assesses_vulnerable_version_with_and_without_agent_and_modloader_overrides() {
        let tmp = tempdir().unwrap();
        let paths = LauncherPaths::portable(tmp.path());
        paths.initialize().unwrap();
        let policy = SecurityPolicyService::embedded_baseline();

        let lib = make_freetype_lib(&paths, "3.3.3", b"old-jar", true);
        let meta = make_metadata("1.21.4-fabric-0.16.10", vec![lib]);

        let with_agent =
            SecurityManager::assess_instance(&paths, &meta, "fabric_1214", true, 21, &policy);
        assert_eq!(with_agent.status, SecurityStatus::PartiallyProtected);
        assert!(with_agent.should_attach_agent);
        assert!(with_agent.description.contains("3.3.3"));
        assert!(with_agent.description.contains("FT_LOAD_NO_HINTING"));

        let without_agent =
            SecurityManager::assess_instance(&paths, &meta, "fabric_1214", false, 21, &policy);
        assert_eq!(without_agent.status, SecurityStatus::Unprotected);
        assert!(!without_agent.should_attach_agent);
    }

    #[test]
    fn rejects_missing_or_corrupt_freetype_on_disk() {
        let tmp = tempdir().unwrap();
        let paths = LauncherPaths::portable(tmp.path());
        paths.initialize().unwrap();
        let policy = SecurityPolicyService::embedded_baseline();

        // Claims patched 3.4.3-mojang-2.15.0 in metadata, but file is missing on disk!
        let missing = make_freetype_lib(&paths, "3.4.3-mojang-2.15.0", b"patched-jar", false);
        let meta = make_metadata("26.3", vec![missing]);
        let res = SecurityManager::assess_instance(&paths, &meta, "broken_263", true, 21, &policy);
        assert_ne!(res.status, SecurityStatus::Protected);
        assert_eq!(res.status, SecurityStatus::Unknown);
    }

    #[test]
    fn agent_failure_or_unverified_status_is_never_marked_verified() {
        let tmp = tempdir().unwrap();
        let paths = LauncherPaths::portable(tmp.path());
        paths.initialize().unwrap();
        let policy = SecurityPolicyService::embedded_baseline();

        let lib = make_freetype_lib(&paths, "3.4.1", b"old-341", true);
        let meta = make_metadata("26.2", vec![lib]);

        let inst_dir = paths.instance_dir("mc26_2");
        let status_file = SecurityAgentManager::status_file_path(&inst_dir);
        fs::create_dir_all(status_file.parent().unwrap()).unwrap();

        // INITIALIZED alone must NOT show ACTIVE_VERIFIED
        fs::write(
            &status_file,
            r#"{"schema_version":1,"state":"INITIALIZED","patched_methods":0,"detail":"loaded"}"#,
        )
        .unwrap();
        let init_only =
            SecurityManager::assess_instance(&paths, &meta, "mc26_2", true, 21, &policy);
        assert_eq!(
            init_only.agent_runtime_state,
            SecurityAgentRuntimeState::Initialized
        );
        assert!(init_only
            .description
            .contains("not treated as runtime proof"));

        // FAILED runtime report -> UNPROTECTED
        fs::write(
            &status_file,
            r#"{"schema_version":1,"state":"FAILED","patched_methods":0,"detail":"bad classfile"}"#,
        )
        .unwrap();
        let failed = SecurityManager::assess_instance(&paths, &meta, "mc26_2", true, 21, &policy);
        assert_eq!(failed.status, SecurityStatus::Unprotected);
    }

    #[tokio::test]
    async fn sync_rollback_restores_original_files_on_failure() {
        let tmp = tempdir().unwrap();
        let paths = LauncherPaths::portable(tmp.path());
        paths.initialize().unwrap();
        let policy = SecurityPolicyService::embedded_baseline();

        // Existing instance metadata and native file that MUST survive a failed sync intact.
        let ver_id = "26.3-test";
        let ver_json = paths.version_json(ver_id);
        fs::create_dir_all(ver_json.parent().unwrap()).unwrap();
        let original_meta = br#"{"id":"26.3-test","original":true}"#;
        fs::write(&ver_json, original_meta).unwrap();

        let natives_dir = paths.natives_dir(ver_id);
        fs::create_dir_all(&natives_dir).unwrap();
        fs::write(natives_dir.join("freetype.dll"), b"original-native").unwrap();

        // Spin up a tiny mock server serving valid candidate metadata whose JAR download returns 404/bad SHA-1.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let candidate_json = serde_json::json!({
            "id": ver_id,
            "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "libraries": [{
                "name": "org.lwjgl:lwjgl-freetype:3.4.3-mojang-2.15.0",
                "downloads": {
                    "artifact": {
                        "path": "org/lwjgl/lwjgl-freetype/3.4.3-mojang-2.15.0/lwjgl-freetype-3.4.3-mojang-2.15.0.jar",
                        "sha1": "0000000000000000000000000000000000000000",
                        "size": 12,
                        "url": format!("http://{addr}/missing-jar")
                    }
                }
            }]
        })
        .to_string();
        let candidate_sha1 = sha1_hex_bytes(candidate_json.as_bytes());
        let candidate_len = candidate_json.len() as u64;

        let handle = std::thread::spawn(move || {
            for _ in 0..8 {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut buf = [0u8; 1024];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                if req.starts_with("GET /meta.json ") {
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        candidate_json.len(),
                        candidate_json
                    );
                    let _ = stream.write_all(resp.as_bytes());
                } else {
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found",
                    );
                }
            }
        });

        let client = DownloadClient::new().unwrap();
        let entry = VersionManifestEntry {
            id: ver_id.to_string(),
            version_type: "release".to_string(),
            url: format!("http://{addr}/meta.json"),
            release_time: String::new(),
            sha1: Some(candidate_sha1),
            size: Some(candidate_len),
        };

        let outcome = MojangLibrarySync::sync_from_manifest_entry(&paths, &client, &entry, &policy)
            .await
            .unwrap();
        drop(handle);

        match outcome {
            LibrarySyncOutcome::RolledBack { .. } => {}
            other => panic!("Expected RolledBack, got {other:?}"),
        }
        assert_eq!(fs::read(&ver_json).unwrap(), original_meta);
        assert_eq!(
            fs::read(natives_dir.join("freetype.dll")).unwrap(),
            b"original-native"
        );
    }
}
