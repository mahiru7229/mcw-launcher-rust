# Mojang Library Synchronization (`LIBRARY_SYNC.md`)

## 1. Purpose

When Mojang updates `org.lwjgl:lwjgl-freetype` inside an existing Minecraft version's metadata (for example, updating `26.3` in-place to ship `org.lwjgl:lwjgl-freetype:3.4.3-mojang-2.15.0`), cached local manifests (`version_manifest_v2.json` and `versions/<id>/<id>.json`) become stale. `MojangLibrarySync` detects and applies verified FreeType library updates safely without overwriting unrelated dependencies.

## 2. Synchronization Flow

Reuses existing `VersionManifestService` and `LibraryManager` components:

1. **Refresh Official Manifest**: Calls `VersionManifestService::find_version(base_version_id, true)` against `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json`.
   - If the launcher is offline, returns `LibrarySyncOutcome::OfflineSkipped` cleanly so cached instances continue launching without interruption.
2. **Compare Metadata Checksum**: Computes the SHA-1 of local `versions/<id>/<id>.json` and compares it with Mojang's `entry.sha1`.
   - If unchanged and local `lwjgl-freetype` artifacts already verify against their SHA-1s on disk, returns `AlreadyUpToDate`.
3. **Validate Candidate Metadata**:
   - Downloads Mojang's updated `<id>.json` to a temporary staging file and verifies its SHA-1 against `entry.sha1`.
   - Parses `VersionMetadata` and checks its `org.lwjgl:lwjgl-freetype` entries against `SecurityPolicy`: only proceeds with replacement if Mojang's updated metadata contains a **verified safe FreeType artifact** (`2.15.0+`).
   - Does **not** touch unrelated Minecraft libraries, client JARs, assets, or modloader profiles.
4. **Transactional Backup, Replacement & Rollback**:
   - Backs up existing `versions/<id>/<id>.json` and `natives/<id>/` to a rollback directory inside `cache/security/sync_backup_<id>/`.
   - Downloads only the updated `lwjgl-freetype` main JAR and OS/arch-matching native JAR via `DownloadClient` with strict SHA-1 + size verification.
   - Cleans stale FreeType natives (`*freetype*.dll`, `*freetype*.so`, `*freetype*.dylib` and their `.extracted` markers) in `natives/<id>/` so the JVM cannot accidentally load a pre-update binary, then extracts the newly verified native JAR.
   - Atomically replaces `versions/<id>/<id>.json` and cleans up the backup on success.
   - **Rollback Guarantee**: If any step fails (network drop, SHA-1 mismatch, corrupt zip, I/O error), `MojangLibrarySync` immediately restores the backed-up `version.json` and `natives/<id>/` directory before returning an error or warning.
