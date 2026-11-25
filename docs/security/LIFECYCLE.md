# MCW Security Agent Lifecycle & Retirement (`LIFECYCLE.md`)

`MCW Security Agent` is a temporary runtime defense layer designed to be safely deactivated and removed once Mojang updates affected Minecraft versions to `FreeType >= 2.15.0`.

## The 7 Lifecycle Stages

1. **Stage 1 — Agent Release**
   - Pure-Java `mcw-security-agent.jar` is built deterministically (`tools/mcw_security_agent/build.py`), verified with `-Xverify:all`, and pinned by SHA-256 in `SecurityAgentManager`.
2. **Stage 2 — Installation & Integrity Verification**
   - Extracted lazily to `<cache_root>/security_agent/mcw-security-agent.jar`. Before every launch where it is needed, SHA-256 is verified; any corrupted file is atomically replaced.
3. **Stage 3 — Conditional Protection Activation**
   - Attached **only** when `SecurityManager::assess_instance` inspects the instance's actual merged libraries and confirms an unpatched `lwjgl-freetype` dependency is present and the user has enabled mitigation.
4. **Stage 4 — Library & Policy Monitoring**
   - `SecurityPolicyService` periodically checks Ed25519-signed remote security policy metadata (`agent_lifecycle`: `Active` | `Deprecated` | `Retired`, `safe_freetype_sha1s`, `safe_freetype_version_markers`), while `MojangLibrarySync` checks `version_manifest_v2.json` for in-place Mojang FreeType updates.
5. **Stage 5 — Per-Instance Actual Library Verification**
   - **Critical Rule**: MCW never removes protection solely because Mojang published an announcement. Every version/instance must pass **on-disk artifact & native SHA-1 verification** on its actual merged `VersionMetadata` (`PROTECTED`) before the agent is omitted. Modded instances (Fabric/Forge/NeoForge/Quilt) whose profiles pin an older `lwjgl-freetype` stay `PARTIALLY_PROTECTED` with the agent active.
6. **Stage 6 — Per-Instance & Global Deactivation**
   - Instances verified at `PROTECTED` (`26.3+` with `3.4.3-mojang-2.15.0`, or `<= 1.20.4` without FreeType) automatically launch **without** `-javaagent:mcw-security-agent.jar`.
   - When a signed policy transitions `agent_lifecycle` to `Retired` (once all supported versions are verified upstream or archived), new launches skip agent injection globally.
7. **Stage 7 — Complete Removal**
   - `SecurityAgentManager::uninstall` deletes `mcw-security-agent.jar` and temporary files cleanly from cache without affecting any Minecraft instances, worlds, mods, or core launcher subsystems. When no legacy vulnerable versions remain supported in future MCW releases, the `tools/mcw_security_agent/` folder and `security_agent.rs` adapter can be deleted cleanly.
