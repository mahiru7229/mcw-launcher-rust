# MCW Security Test Plan (`TEST_PLAN.md`)

All tests use safe synthetic metadata, checksum fixtures, and harmless dummy Java classes (no real exploit `.ttf` payloads are stored or executed).

## Verification Matrix

| Scenario | Expected Result | Automated Test Coverage |
|---|---|---|
| **Minecraft Vanilla (`<= 1.20.4`)** | No `lwjgl-freetype` dependency -> `PROTECTED` with clear description; `should_attach_agent = false`. | `mcw-minecraft`: `security::tests::assesses_unaffected_pre_1_20_5_as_protected` |
| **Minecraft Vanilla (`26.3` patched `3.4.3-mojang-2.15.0`)** | Verified safe artifact & native SHA-1 on disk -> `PROTECTED` with description; `should_attach_agent = false`. | `mcw-minecraft`: `security::tests::assesses_verified_mojang_2_15_as_protected_without_agent` |
| **Minecraft Vanilla (`1.20.5`–`1.21.11`, `26.1`–`26.2`)** | Unpatched `lwjgl-freetype:3.3.3`/`3.4.1` -> `PARTIALLY_PROTECTED` when agent enabled (`should_attach_agent = true`), `UNPROTECTED` when disabled. | `mcw-minecraft`: `security::tests::assesses_vulnerable_version_with_and_without_agent` |
| **Fabric / Forge / NeoForge / Quilt** | Merged metadata evaluated cleanly: patched base + loader override pinning `3.3.3` still flags `PARTIALLY_PROTECTED` / attaches agent. | `mcw-minecraft`: `security::tests::assesses_modded_loader_override_accurately` |
| **Multi-Java Compatibility & Agent Verification** | Java 8–25+ accepted; `< Java 8` -> `INCOMPATIBLE` / `UNPROTECTED`. Live JVM `-Xverify:all` smoke test verifies `DORMANT` vs `ACTIVE_VERIFIED` (`patched_methods = 4`). | `tools/mcw_security_agent/build.py` + `mcw-runtime`: `security_agent::tests` |
| **Missing or Corrupt FreeType Library** | Missing JAR or bad SHA-1 on disk on claimed `2.15.0` -> **never** returns `PROTECTED` (returns `UNKNOWN` + descriptive reason). | `mcw-minecraft`: `security::tests::rejects_missing_or_corrupt_freetype_on_disk` |
| **Agent Init Failure / `INITIALIZED` without Verification** | `INITIALIZED` alone does NOT claim verified runtime protection; `FAILED` status escalates assessment to `UNPROTECTED`. | `mcw-minecraft`: `security::tests::agent_failure_or_unverified_status_is_never_marked_verified` |
| **Offline Mode & Signed Policy Validation** | Offline falls back to cached/embedded baseline without blocking launches; tampered Ed25519 signature rejected; expired policy flagged cleanly. | `mcw-minecraft`: `security_policy::tests` |
| **Interrupted Library Sync & Rollback** | Failed download or checksum mismatch during `MojangLibrarySync` restores previous `version.json` and `natives/<id>/` byte-for-byte. | `mcw-minecraft`: `security::tests::sync_rollback_restores_original_files_on_failure` |
| **Resource Pack & Custom Font Compatibility** | `FT_LOAD_NO_HINTING` (`0x2`) preserves color/emoji/bitmap flags (`FT_LOAD_RENDER`, `FT_LOAD_COLOR`); checked via `0` and bitmask `4` -> `2` and `6`. | `tools/mcw_security_agent/build.py` |
