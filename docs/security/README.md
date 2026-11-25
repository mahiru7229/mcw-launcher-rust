# MCW Security System — Minecraft2Shell Hotfix Architecture

This directory documents the two-layer security architecture implemented in **MCW Launcher: Rust Edition (`v1.3.1+`)** to protect Minecraft: Java Edition players against the **Minecraft2Shell** (`bikini/minecraft2shell`) vulnerability without modifying Core instance/library ownership or breaking existing instances.

## Defense-in-Depth Architecture

1. **Layer 1 (Permanent Fix) — Official Mojang Library Synchronization ([LIBRARY_SYNC.md](LIBRARY_SYNC.md))**
   - Uses `VersionManifestService` and `LibraryManager` to query Mojang's official `version_manifest_v2.json`.
   - Detects in-place metadata updates where Mojang shipped patched `org.lwjgl:lwjgl-freetype:3.4.3-mojang-2.15.0` artifacts (containing FreeType `2.15.0+`).
   - Verifies SHA-1 checksums strictly, cleans stale native FreeType binaries, replaces **only** `lwjgl-freetype` artifacts and version metadata, and automatically rolls back if any download, checksum, or extraction step fails.
2. **Layer 2 (Temporary Runtime Mitigation) — MCW Security Agent ([SECURITY_AGENT.md](SECURITY_AGENT.md))**
   - An independent pure-Java `-javaagent` (`mcw-security-agent.jar`) attached only when an instance actually depends on an unpatched `lwjgl-freetype` library (e.g., `1.20.5`–`1.21.11`, `26.1`–`26.2`).
   - Intercepts LWJGL FreeType glyph loading (`nFT_Load_Glyph`, `nFT_Load_Char`, `FT_Load_Glyph`, `FT_Load_Char`) to enforce `FT_LOAD_NO_HINTING` (`0x2`), preventing execution of TrueType hinting bytecode (`Ins_SHZ`).
   - Validates Java compatibility, reports a verified runtime handshake (`INITIALIZED` -> `ACTIVE_VERIFIED` / `FAILED`), and is automatically omitted once an instance's libraries are confirmed patched by Mojang.
3. **MCW Security Manager & Signed Remote Security Policy**
   - `SecurityManager` evaluates each instance's **real merged dependencies, on-disk artifact checksums, Java runtime compatibility, agent runtime status, and signed policy** into 4 transparent states:
     - `PROTECTED`: Verified absent FreeType surface (`<= 1.20.4`) or verified patched Mojang FreeType `2.15.0+` artifacts on disk (`26.3+`).
     - `PARTIALLY_PROTECTED`: Unpatched `lwjgl-freetype` is present and actively mitigated at runtime by `MCW Security Agent` (`FT_LOAD_NO_HINTING`).
     - `UNPROTECTED`: Unpatched `lwjgl-freetype` is exposed because the agent is disabled, incompatible, or failed verification.
     - `UNKNOWN`: Incomplete metadata, missing/corrupt library files on disk, or unverifiable state.
   - Every UI badge, modal, and log entry pairs the status code with a clear human-readable description explaining why that status applies and what action (if any) is recommended.

## Document Index

- [THREAT_MODEL.md](THREAT_MODEL.md) — Vulnerability root cause, attack vectors, version status, and trust boundaries.
- [SECURITY_AGENT.md](SECURITY_AGENT.md) — Standalone agent design, status handshake, compatibility checks, and uninstallation.
- [LIBRARY_SYNC.md](LIBRARY_SYNC.md) — Official Mojang manifest refresh, targeted `lwjgl-freetype` verification, and rollback guarantees.
- [LIFECYCLE.md](LIFECYCLE.md) — 7-stage lifecycle and retirement criteria for MCW Security Agent.
- [TEST_PLAN.md](TEST_PLAN.md) — Safe verification matrix across Vanilla, mod loaders, offline/error scenarios, and font compatibility.
