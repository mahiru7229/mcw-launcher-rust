# Changelog

All notable changes to **MCW Launcher: Rust Edition** are documented in this file.

## [0.13.0-beta.1] — 2026-11-21

### Added
- **Global `Ctrl+K` Quick Search (`quick_search.rs`)**: Top-bar compact `Search...  Ctrl K` trigger pill (`shell_nav.rs`) and centered in-app command palette overlay searching instances, pages (`Home`, `Servers`, `Versions`, `Logs`), Content Browser categories (`Mods`, `Modpacks`, `Resource packs`, `Shader packs`, `Data packs`), and quick actions with instant `Enter` selection and `Esc` / backdrop dismiss.

### Changed
- **Full UI v2.0 Dialog & Component Restyle**: Upgraded remaining sharp `CornerRadius::same(2..4)` tokens across Create Instance, Modpack Install, Version Picker, Accounts, Manual Downloads, First-Run Wizard, Update Dialog, Repair Center, Global Logs, About, LAN Agent Log, Launcher Settings cards, flyout cards, right-click context menu, thumbnails, account avatars, and group/tag badges to cohesive UI v2.0 radii (`6px–14px`), rounded Create Instance pill tabs, translucent bottom Tasks Bar (`p.glass(...)`), and consistent `Esc`-to-close across all secondary OS viewports.
- **Simple Animation Polish (`shell.rs`)**: Smoothed active task progress bars using `animate_value_with_time` (`self.motion_secs(0.15)`) and gated indeterminate spinner animation on `MotionMode::Off`.
- **SOLID & Maintenance Cleanup**: Extracted the in-app Launcher Settings modal into `apps/desktop/src/app/views/settings_modal.rs` (bringing `dialogs.rs` down to ~2.2k lines), removed dead mod-install events/helpers (`CurseForgeModInstallFinished`, `ModrinthInstallFinished`, `apply_theme`), cleaned stale `#[allow(dead_code)]` attributes, wired additional official i18n keys, and synced `README.md`, `DESIGN.md`, and `docs/BACKLOG.md`.

---
## [0.12.0-beta.1] — 2026-11-21

### Added
- **Home / Instance Page (`home_page.rs`)**: Horizon-style hero header (icon, title, playtime/mod/world stats, responsive group/tag pills), large `Play` button, `•••` Actions menu (consolidating all instance management actions), quick vector icon actions (Add Content, Open Folder, Edit Settings), and one floating translucent tabbed panel (`Mods`, `Worlds`, `Resource packs`, `Shaders`, `Screenshots`, `Backups`, `Logs`, `Settings`).
- **Unified Content Browser OS Window (`content_browser.rs`)**: Centered OS viewport covering `Mods`, `Modpacks`, `Resource packs`, `Shader packs`, and `Data packs` (`mcw-mods::ContentKind::DataPack` + CurseForge class ID `6945`) with `ComboBox` filters (`Provider`, `Sort`, `Loader`, `Target Instance`), split scrollable list + right-hand detail panel (with per-world vs global target selector for Data Packs).
- **Versions Page (`versions_page.rs`)**: Dedicated management for cached Minecraft versions under `cache/versions/` (`StorageCleanupService::list_cached_versions` / `delete_cached_version`, usage tracking, `Prune Unused`) plus Mojang manifest browser (`Releases` / `Snapshots` / `All`).
- **Servers & LAN Page (`servers_page.rs`)**: Uncompressed NBT `servers.dat` reader/writer (`mcw-minecraft::ServerManager`) with Add/Remove/Copy IP, direct `--quickPlayMultiplayer` join on MC 1.20+, plus direct MCW LAN Agent & `e4mc` tunnel controls.
- **Logs Page (`logs_page.rs`)**: Unified in-app log inspector for both Global Launcher Events and per-instance live stdout/stderr streams (with Crash Report jump, `mclo.gs` upload, Diagnostic `.zip` export, and Pop Out Global Window).

### Changed
- Split desktop pages & unified Content Browser into focused modules per `SOLID.md`, trimming ~2,000 lines across `dialogs.rs` and `workspace.rs`.
- Replaced interim v0.11 top-bar destinations with dedicated `Home`, `Mods` (OS window), `Servers`, `Versions`, and `Logs` views.

---
## [0.11.0-beta.1] — 2026-11-20

### Added
- **UI v2.0 foundation (Horizon-style, see DESIGN.md)**: full-window background picture (user PNG/JPEG/WebP or a bundled procedural pixel landscape) with *Panel opacity* and *Background dimming* sliders in Launcher Settings → Appearance (no blur).
- **Theme presets**: Violet (new default), Sunrise, Neon, Ocean, plus the classic Emerald (`dark`) and Light; custom accent and `themes/*.json` keep working.
- **New shell**: floating top pill navigation (Home, Mods, Servers, Versions, Logs), slim instance icon rail with status dots and right-click menu, instance flyout (search, favorites, groups, tags).
- **Launcher Settings is now an in-app centered modal** with category list + search (Esc / click outside to close).
- Simple animations only (hover/fade, page and modal fade-in), honoring Motion Full / Reduced / Off.

### Changed
- Global corner radii enlarged; panels are translucent over the background.
- Interim nav wiring until v0.12 pages: Servers → instance Settings tab, Versions → Settings → Storage, Logs → Global Logs window.

---
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
