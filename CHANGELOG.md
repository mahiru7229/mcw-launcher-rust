# Changelog

All notable changes to **MCW Launcher: Rust Edition** are documented in this file.

## [0.13.2-beta.1] - 2026-11-21

### Added
- **Content Browser Pagination (`Previous` / `Next`)**: Added full page controls (`Page X / Y (N results)`, `PageSize = 20`) inside the global and per-instance **Content Browser** across **Modrinth**, **CurseForge** (`CurseForgePagination`), **FTB**, and **ATLauncher** (`search_paged`), resetting to Page 1 whenever query/sort/loader/MC-version filters change.
- **Dedicated Step-2 `Install Destination` OS Window (`mcw_vp_install_target`)**: Clicking `Install` or `Install Selected Version` in global **Mod & Modpacks** Version Picker now opens a clean second OS window allowing users to choose an **Existing Instance** (strictly filtered by **exact matching Minecraft version** + **exact matching mod loader**, with Data Pack world selection) or **Create New Instance** (MC version & loader `ComboBox`), with `Back` support to return to Version Picker.
- **Dedicated `Java Runtimes` OS Window (`mcw_vp_java_manager`) & Dynamic Latest JDK Lookup**: Replaced inline stacked buttons and overflowing Java paths in *Launcher Settings -> Runtime* with a compact summary card (`Manage Java Runtimes...`, dynamic `Install latest JDK (Java {major})` queried from Adoptium `available_releases`, and `Scan Java installations`) and a dedicated scrollable OS window with version dropdown, Latest JDK button, truncated paths with hover tooltips, `Open Folder`, and `Uninstall`.
- **Native Dedicated GPU Detection (`GpuPreferenceManager::detect()`)**: Added Windows (`Win32_VideoController`) and Linux (`lspci`) dedicated vs. integrated adapter detection inside `FirstRunRecommendation`.

### Changed
- **Responsive Content Browser Header Pill Bar**: Split Title + `Close` (Row 1) from the left-to-right Category Pill Bar (`Mods`, `Modpacks`, `Resource packs`, `Shader packs`, `Data packs` on Row 2) so tabs never squeeze or wrap awkwardly onto a second line.
- **Smart Default Window Size**: Fresh launches automatically size to `1600x900` on `>= 1920x1080` displays and `1280x720` on `>= 1366x768` displays (DPI-clamped on high-DPI), tracking explicit user resizes via `window_size_customized` so prior auto-saved `1080x680` defaults upgrade cleanly.
- **Pure Black Default Background (`#000000`)**: Removed the procedural Horizon pixel-landscape fallback; Dark mode now uses pure `#000000` black by default when no custom user wallpaper is configured.
- **Python-Parity 6-Step First-Run Setup Wizard**: Rebuilt First-Run Setup (`Welcome` -> `Account` -> `Java` -> `Memory` -> `Graphics` -> `Finish`) matching MCW Launcher Python's `first_run_setup_dialog.py` workflow, i18n keys, account creation/OAuth helpers, Java auto-selection, RAM slider, GPU preference toggle, and extended configuration summary.

---

## [0.13.1-beta.1] - 2026-11-21

### Added
- **1-Step Global Content Install Target Dialog**: Browsing **Mod & Modpacks** (`Mods`, `Resource packs`, `Shader packs`, `Data packs`) from the top navigation opens a dedicated target selector directly inside the version picker (`Existing Instance` with Data Pack world selection, or `Create New Instance` pre-filled with the matching MC version and loader).
- **JVM Preset Selector in Modpack Install Dialog**: Added a `JVM Preset` `ComboBox` (`Default`, `Aikar`, `ZGC`, `Shenandoah`) directly inside the Modpack Install dialog, persisting selected flags into the created instance's `settings.json`.
- **Cross-Category Right-Pane Search in Launcher Settings**: Searching in `Launcher Settings` now filters setting cards across all 6 categories (`General`, `Downloads`, `Runtime`, `Appearance`, `Storage`, `Updates`) and displays matched cards with category headers in the right pane.
- **Secondary OS Viewport Re-Focus**: Re-triggering an already-open secondary OS window (`Create Instance`, `Export Modpack`, `Content Browser`, `Accounts`, `Repair Center`, `Global Logs`, `First-Run Setup`, `Manual Downloads`, `About`, `All Active Tasks`, `Rename`/`Clone`/`Delete`/`Group & Tags`) brings it back to the foreground.

### Changed
- **Navigation & Home Tab Restructure**: Renamed top-bar `Mods` to **Mod & Modpacks** (global browsing mode); moved **Servers & LAN** from the top bar into each instance's Home tab bar; replaced the Home quick-edit pencil with an 8-tooth **Gear** icon opening `Instance Settings` and removed the `Instance Settings` pill from the tab bar; removed the duplicate `MCW LAN Agent` block from `Instance Settings`.
- **Manual-Only Instant LAN Hosting**: Removed `e4mc` tunnel options and the `Apply LAN Hosting` button from **Servers & LAN**; toggling `Enable MCW LAN Agent` now saves `Manual (Local LAN / VPN)` mode immediately without touching user-installed tunnel mods.
- **Content Browser Vertical Card Layout & Global Filters**: Fixed horizontal stacking in the Content Browser result list by wrapping split columns in explicit top-down layouts, added a global MC Version filter `ComboBox`, and set Modpacks default loader filters to `Fabric` (Modrinth) and `Forge` (CurseForge, ATLauncher, FTB).
- **Versions Page Single Overall Scroll**: Made `Cached Versions` collapsible (`Show` / `Hide`) and removed the inner `max_height` scroll inside `Available Minecraft Versions` so the entire page scrolls smoothly together.
- **Origin-Aware Modpack Export Formats**: Restricted Export Modpack formats by detected instance origin (`Modrinth` -> `.mrpack` + `.mcwpack`; `CurseForge` -> `CurseForge Modpack (.zip)` + `.mcwpack`; `ATLauncher`/`FTB` -> `Provider Profile (.zip)` + `.mcwpack`; manual/custom -> `.mcwpack` only).
- **First-Run / Default Recommended RAM Formula**: Updated default max memory calculation to `max(4096 MB, 50% total RAM)` snapped to 256 MB steps (`<= total - 1024 MB` safety clamp on `<= 4 GB` PCs, `16384 MB` cap).
- **Display Polish**: Title-cased mod loader and version type labels (`Vanilla`, `Fabric`, `Quilt`, `Forge`, `NeoForge`, `Release`, `Snapshot`, `Old Beta`, `Old Alpha`, `All Loaders`) across the UI while keeping lowercase JSON slugs; replaced all Unicode em/en-dashes in UI strings and window titles with ASCII `-`.

### Fixed
- **Background Picker Hang**: Moved background picture file picking (`TopMost` Windows dialog) and PNG/JPEG/WebP image decoding onto background threads so clicking `Choose a picture...` never freezes the UI thread.
- **OptiFine / Import Browse Label Leak**: Fixed raw `"common.browse"` key display by switching to the verified `instance_defaults.java.browse` localization key.

---

## [0.13.0-beta.1] - 2026-11-21

### Added
- **Global `Ctrl+K` Quick Search (`quick_search.rs`)**: Top-bar compact `Search...  Ctrl K` trigger pill (`shell_nav.rs`) and centered in-app command palette overlay searching instances, pages (`Home`, `Servers`, `Versions`, `Logs`), Content Browser categories (`Mods`, `Modpacks`, `Resource packs`, `Shader packs`, `Data packs`), and quick actions with instant `Enter` selection and `Esc` / backdrop dismiss.

### Changed
- **Full UI v2.0 Dialog & Component Restyle**: Upgraded remaining sharp `CornerRadius::same(2..4)` tokens across Create Instance, Modpack Install, Version Picker, Accounts, Manual Downloads, First-Run Wizard, Update Dialog, Repair Center, Global Logs, About, LAN Agent Log, Launcher Settings cards, flyout cards, right-click context menu, thumbnails, account avatars, and group/tag badges to cohesive UI v2.0 radii (`6px-14px`), rounded Create Instance pill tabs, translucent bottom Tasks Bar (`p.glass(...)`), and consistent `Esc`-to-close across all secondary OS viewports.
- **Simple Animation Polish (`shell.rs`)**: Smoothed active task progress bars using `animate_value_with_time` (`self.motion_secs(0.15)`) and gated indeterminate spinner animation on `MotionMode::Off`.
- **SOLID & Maintenance Cleanup**: Extracted the in-app Launcher Settings modal into `apps/desktop/src/app/views/settings_modal.rs` (bringing `dialogs.rs` down to ~2.2k lines), removed dead mod-install events/helpers (`CurseForgeModInstallFinished`, `ModrinthInstallFinished`, `apply_theme`), cleaned stale `#[allow(dead_code)]` attributes, wired additional official i18n keys, and synced `README.md`, `DESIGN.md`, and `docs/BACKLOG.md`.

---

## [0.12.0-beta.1] - 2026-11-21

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

## [0.11.0-beta.1] - 2026-11-20

### Added
- **UI v2.0 foundation (Horizon-style, see DESIGN.md)**: full-window background picture (user PNG/JPEG/WebP or a bundled procedural pixel landscape) with *Panel opacity* and *Background dimming* sliders in Launcher Settings -> Appearance (no blur).
- **Theme presets**: Violet (new default), Sunrise, Neon, Ocean, plus the classic Emerald (`dark`) and Light; custom accent and `themes/*.json` keep working.
- **New shell**: floating top pill navigation (Home, Mods, Servers, Versions, Logs), slim instance icon rail with status dots and right-click menu, instance flyout (search, favorites, groups, tags).
- **Launcher Settings is now an in-app centered modal** with category list + search (Esc / click outside to close).
- Simple animations only (hover/fade, page and modal fade-in), honoring Motion Full / Reduced / Off.

### Changed
- Global corner radii enlarged; panels are translucent over the background.
- Interim nav wiring until v0.12 pages: Servers -> instance Settings tab, Versions -> Settings -> Storage, Logs -> Global Logs window.

---

## [0.10.0-beta.1] - 2026-11-20

### Added
- **Stage 5 Full Python Parity**: Completed final parity modules across the core workspace (`92df5aa`):
  - **Shared Content Store (`mcw-storage::content_store`) & Content Library (`mcw-mods::library`)**: Content-addressed SHA-256 hardlink deduplication across instances plus unified library scanning and pin flags.
  - **Managed OptiFine Lifecycle (`mcw-minecraft::optifine`)**: Transactional Forge-based OptiFine validation, install/rollback, registry tracking, and clean uninstall.
  - **Forge Preflight Analyzer (`mcw-minecraft::preflight`)**: Pre-launch mod compatibility confirmation policies and sanitized loader diagnostics `.zip` export.
  - **LAN & Legacy Compatibility (`mcw-minecraft::lan_targets`, `legacy_libloader`)**: Mojang/Tiny mappings parser, JVM bytecode method table resolver, and embedded legacy library loader extraction.
  - **Pack Registry & Java Diagnostics (`mcw-providers::pack_registry`, `mcw-runtime::java_diagnostics`, `mcw-models::policy`)**: FTB/ATLauncher persistent registries, export origin detection, optional-file inspection (`.mrpack` / CurseForge), detailed Java runtime diagnostics + managed uninstaller, and dedicated LAN Agent Log OS window (`stage5_dialogs.rs`).

---

## [0.9.0-beta.1] - 2026-11-20

### Added
- **Stage 4 Security, Compatibility & Diagnostics**:
  - **Token Integrity & Network Controls**: RFC-2104 HMAC-SHA256 verification on encrypted `AccountStore` refresh tokens (`mcw-auth::integrity`), global MB/s bandwidth limiter (`mcw-net::bandwidth`), and optional runtime CurseForge API key override.
  - **Mod Compatibility & Instance Health**: Loader mismatch, duplicate mod & missing dependency checks (`mcw-mods::compat`), instance health repair scanning + startup stale lock/staging orphan recovery (`mcw-storage::health`).
  - **GPU Preference & OS Windows**: Windows DirectX high-performance GPU preference registry support (`mcw-runtime::gpu`), native window activation (`mcw-runtime::window_activator`), and dedicated Repair Center, Global Logs & About OS windows (`stage4_dialogs.rs`).

### Changed
- **Updater Disabled (`mcw_updater::UPDATES_ENABLED = false`)**: Disabled in-app self-updates (`e262586`) and removed the legacy Python/Cloudflare update channel while preserving the Rust-Edition GitHub Releases checker/installer scaffold.

---

## [0.8.0-beta.1] - 2026-11-20

### Added
- **Stage 1 - Runtime, Cloning & Organization**: JVM Flag Presets (`Default`, `Aikar`, `ZGC`, `Shenandoah`), Windows classpath compactor (`compactor.rs`), instance cloning & runtime/loader switcher, sidebar search + Favorites/Groups/Tags with responsive auto-sized pill badges, and `.mcwpack` portable import/export.
- **Stage 2 - Providers, LAN & World Backups**: `ModProvenanceRegistry` & `ModUpdateManager` with version locking, recursive CurseForge dependencies, `LanHostingManager`, per-world backup/restore/import, `LaunchErrorPresenter` & `DiagnosticsExporter`, SHA-1 `VerificationCache`, async `SaveFileDialog`/`OpenFileDialog`, mod version channel/version/loader filters, and schema 2.1 diagnostics.
- **Stage 3 - Appearance, Hardware Scan & Maintenance**: Microsoft skin face avatars (`mcw-auth::skin`, `skin_render.rs`), Discord IPC Rich Presence (`mcw-runtime::discord_rpc`), initial `FirstRunRecommendation` hardware/RAM/Java wizard, `StorageCleanupService` inspector & unreferenced version pruner (`mcw-storage::cleanup`), external `themes/*.json` pack loading + custom accent hex picker, and `mcw-updater::installer` scaffold (`stage3_dialogs.rs`).

### Changed
- **SOLID Architectural Refactor**: Split monolithic GUI state (`e763106`) into cohesive `controllers.rs`, `platform.rs`, and focused view modules (`views/`) and introduced the `InstanceRepository` storage trait.

---

## [0.6.0-beta.1] - 2026-11-19

### Added
- **Worlds, Screenshots, Packs & Logs**: Worlds Manager with Quick Play (`--quickPlaySingleplayer`), Screenshots gallery manager, sanitized `mclo.gs` log uploads, and Resource Packs / Shader Packs management with Modrinth & CurseForge browsing.
- **Bilingual i18n & OptiFine / LAN Agent**: Ported official MCW Python `LanguageManager` reusing embedded `lang/en-US.json` and `lang/vi-VN.json`, added OptiFine installer support and embedded MCW LAN Agent (`mcw_lan_agent`), and rebuilt Launcher Settings sections from MCW Python.
- **Rich Modpack & Content UX**: Direct FTB / ATLauncher / Modrinth modpack installs, 3-mode Modpack Export (`.mrpack`, CurseForge `.zip`, `.mcwpack`), async preview thumbnails, Version Picker & Modpack Install config dialog (Beta/Alpha channels, RAM, Java, Open Web), live download speed readout, and inline Pause/Cancel controls.

### Changed
- Preferred `javaw.exe` (suppressing extra console windows on Windows), isolated sub-windows into centered OS viewports, and switched the GitHub Release workflow to manual-only (`be36bca`, `5cfae45`).

---

## [0.5.0-beta.1] - 2026-11-19

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
- **Compile-Time Encrypted CurseForge API Key (`build.rs`) & CI/Release Workflows**: Automated Windows & Linux GitHub Actions builds with per-build SHA-256 CTR obfuscation for embedded API keys (`cdd049a`) and size-optimized release profile.

### Changed
- **Instant Instance Switching**: Replaced synchronous `mods/` & `backups/` scanning on click with lightweight cache sync + lazy async Tokio background loading.
- **Button Typography & Sizing**: Removed cramped `.small()` buttons globally and tuned `button_padding` so all controls auto-size cleanly around their text.
- **Download Reliability**: Deduplicated concurrent library download requests and switched temp files to unique `.{pid}-{seq}.part` names.

---

## [0.4.0-alpha.1] - 2026-11-19

### Added
- Initial native desktop GUI (`apps/desktop`, `egui`/`eframe`) implementing `DESIGN.md` (Dark Slate + Light theme, Emerald accent, Master-Detail layout).
- Microsoft OAuth PKCE & Device Code sign-in with cancellation support and Windows DPAPI encrypted refresh token storage.

---

## [0.3.0-alpha.1] - 2026-11-19

### Added
- `mcw-mods`: Parallel `rayon` `.jar` metadata scanner (Fabric, Forge, NeoForge, Jar-in-Jar) and mod enable/disable/delete manager.
- `mcw-providers`: SQLite WAL `ApiCache`, Modrinth v2 client, and CurseForge API v1 client.
- `mcw-updater`: Update checker with Cloudflare CDN + GitHub Releases failover.

---

## [0.2.0-alpha.1] - 2026-11-19

### Added
- Instance CRUD + `InstanceRunLock`.
- Microsoft OAuth (PKCE + Device Code + Xbox Live / XSTS / Minecraft token chain).
- Managed Adoptium Java Provisioner + `.class` bytecode version scanner.
- Fabric & Quilt mod loader integration.
- Win32 Job Object `ProcessSupervisor` with live log streaming and crash classification.

---

## [0.1.0-alpha.1] - 2026-11-19

### Added
- Initial Cargo workspace (`mcw-models`, `mcw-storage`, `mcw-net`, `mcw-auth`, `mcw-runtime`, `mcw-minecraft`, `apps/cli`).
- Full Vanilla Minecraft download & launch pipeline with SHA-1 verification.
