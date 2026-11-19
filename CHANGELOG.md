# Changelog

All notable changes to **MCW Launcher: Rust Edition** are documented in this file.

## [0.5.0-beta.1] — 2026-11-19

### Added
- **Modpack Installer**: `.mrpack` (Modrinth) and CurseForge `.zip` archive import, plus CurseForge Modpacks, FTB, and ATLauncher search browsers inside the `Create Instance` window.
- **Forge & NeoForge Launch Pipeline**: Legacy Forge (`1.12.2` `install_profile.json` universal JAR extraction) and modern Forge / NeoForge (`--installClient`) wired directly into instance launching.
- **Concurrent Multi-Install & Global Live Progress Bar**: Non-blocking parallel installs for Modpacks, Mods (Modrinth/CurseForge), and managed Temurin Java runtimes with a live `ACTIVE INSTALLS (N)` bottom progress bar.
- **Incomplete Modpack Guard & Auto/Manual Download Scanner**:
  - Instances missing CurseForge opt-out manual mods are persistently flagged (`Incomplete (N)`) on the Sidebar and Detail Header.
  - Per-instance Manual Downloads windows with clear instance names, **Auto-scan** (every 2s), **Manual Scan**, browser `(1)` duplicate suffix matching, and direct CurseForge countdown links.
- **Launcher Settings Window**: Dedicated separate window accessible via top-right vector gear icon with `Appearance` tab (`Dark (Default)` / `Light` theme).
- **Backup & Safe Rollback Restore** (`mcw-storage::backup`) and **Instance Repair** (`mcw-minecraft::repair`).
- **Python 1.8.x Migration** (`mcw-storage::migration`, `mcw-cli migrate`) with `--dry-run` support.
- **GitHub Actions CI & Release Workflows** for automated Windows & Linux builds and size-optimized release binaries.

### Improved
- **Instant Instance Switching**: Replaced synchronous `mods/` & `backups/` scanning on click with lightweight cache sync + lazy async Tokio background loading.
- **Button Typography & Sizing**: Removed cramped `.small()` buttons globally and tuned `button_padding` so all controls auto-size cleanly around their text.
- **Download Reliability**: Deduplicated concurrent library download requests and switched temp files to unique `.{pid}-{seq}.part` names.

---

## [0.4.0-alpha.1]

### Added
- Initial native desktop GUI (`apps/desktop`, `egui`/`eframe`) implementing `DESIGN.md` (Dark Slate + Light theme, Emerald accent, Master–Detail layout).
- Microsoft OAuth PKCE & Device Code sign-in with cancellation support and Windows DPAPI encrypted refresh token storage.

---

## [0.3.0-alpha.1]

### Added
- `mcw-mods`: Parallel `rayon` `.jar` metadata scanner (Fabric, Forge, NeoForge, Jar-in-Jar) and mod enable/disable/delete manager.
- `mcw-providers`: SQLite WAL `ApiCache`, Modrinth v2 client, and CurseForge API v1 client.
- `mcw-updater`: Update checker with Cloudflare CDN + GitHub Releases failover.

---

## [0.2.0-alpha.1]

### Added
- Instance CRUD + `InstanceRunLock`.
- Microsoft OAuth (PKCE + Device Code + Xbox Live / XSTS / Minecraft token chain).
- Managed Adoptium Java Provisioner + `.class` bytecode version scanner.
- Fabric & Quilt mod loader integration.
- Win32 Job Object `ProcessSupervisor` with live log streaming and crash classification.

---

## [0.1.0-alpha.1]

### Added
- Initial Cargo workspace (`mcw-models`, `mcw-storage`, `mcw-net`, `mcw-auth`, `mcw-runtime`, `mcw-minecraft`, `apps/cli`).
- Full Vanilla Minecraft download & launch pipeline with SHA-1 verification.
