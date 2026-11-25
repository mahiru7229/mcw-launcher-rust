# MCW Launcher: Rust Edition

[![Status](https://img.shields.io/badge/status-v1.3.0--beta.4-7C3AED?style=flat-square)](#)
[![Rust](https://img.shields.io/badge/Rust-2021%20Edition-orange?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-blue?style=flat-square)](#)
[![License: MIT](https://img.shields.io/badge/license-MIT-green?style=flat-square)](LICENSE)

**MCW Launcher: Rust Edition** is a fast, lightweight, portable Minecraft launcher built in pure Rust (`egui` / `eframe` + `tokio`), engineered as an independent edition for multi-instance modpack management, concurrent downloads, safe migration from Python Launcher, and cross-platform reliability on **Windows** and **Linux**.

---

## Performance & Footprint (vs. MCW Launcher Python)

| Metric | MCW Launcher (Python / PySide6) | MCW Launcher: Rust Edition (`v1.1.0`) |
| --- | --- | --- |
| **Idle / Active RAM** | ~150 MB+ | **~45-70 MB idle / ~85-110 MB active thumbnail browsing** (64-entry thumbnail LRU + automatic idle texture release) |
| **Runtime Performance** | Baseline | **~2x faster** UI responsiveness, mod scanning, and concurrent downloads |
| **Package Size** | Larger (bundled Python + Qt runtime) | **Lighter (`mcw-desktop` + standalone `mcw-updater`)** - zero bundled Python/Qt runtime |

---

## Key Features

- **UI v2.0 Native Desktop Interface (`egui` / `eframe`)**
  - **Native App Icon, Polished Startup Splash & Typography Controls**: Embedded Windows `.ico` + window/taskbar icon (`assets/icons/`), `2.40s` animated startup splash screen (`MotionMode`-aware with pulsing glow rings, staggered title reveal, and click/key skip), pure `#000000` black backdrop by default in Dark mode, or asynchronous user PNG/JPEG/WebP wallpaper; 6 built-in presets (**Violet** default, **Sunrise**, **Neon**, **Ocean**, **Emerald**, **Light**), interactive **HSV Accent Color Picker Modal** + `#RRGGBB` input, `themes/*.json` pack loading + validation diagnostics, installed system font selector, staged **UI Scale** (`75%-175%`) & **Font Size** (`10-22 px`) with unified `Apply`/`Reset`, responsive `1024x640` minimum bounds, and DPI-aware screen sizing.
  - **Clean Shell, Nav History & In-App Centered Modals**: Clickable top-left MCW logo (`Home`), Back/Forward `<` / `>` history buttons (`Alt+Left` / `Alt+Right`) tracking pages, instances, detail tabs (including Gear -> Settings), global categories, and per-instance inline browsers; floating top pill bar (**Home**, **Mod & Modpacks**, **Versions**, **Logs**, **Changelog** loaded live from GitHub); quick top-bar **Account Switcher** (`ComboBox`); live **`Online`/`Offline`** connectivity pill; slim left instance rail; **`Ctrl+K` Global Quick Search**; **In-App Centered Modals** across all dialogs (including Screenshot Preview & Accent Color Picker) & **6-step First-Run Setup Wizard** (showing in-modal action notices and non-clipping controls), while **Global Logs** and **LAN Agent Log** keep dedicated OS viewports.
  - **Focused Instance Workspace, Library Grid & AIO Productivity**: Toggleable **Home View** (**Workspace View** or grouped, collapsible, sortable **Library Grid View** with quick `Play`/`Open`); Home hero header (`Play`, `...` Actions menu with **Create Desktop Shortcut**, 1-line **Instance Notes** preview, Gear **Instance Settings** toggle with Vanilla **Show Snapshots** support); Vanilla instances hide `Mods` and `Shaders` tabs automatically; item counts `(N)` render only on the active tab; **Sliding Mods Enable/Disable Switch + Bulk Mod Management & Filter**; Resource Pack & Shader **`pack.png` thumbnails** + **Modrinth pack updates**; clicking `+ Add Mod`, `+ Add Resource Pack`, or `+ Add Shader Pack` opens an **inline single-category browser** with **Modrinth-style Inline Detail View** (overview, gallery thumbnails, CurseForge authors, `Open Web Page`) and **Multi-Mod Download Cart (`+ Cart` -> `Review & Confirm`)** for batch dependency resolution & install; **Export Modlist...** (`.md`/`.txt`), **Instance Notes (`notes.txt`)**, visual **Screenshot Gallery** with LRU thumbnails & preview modal, world **`icon.png`**, **Copy Seed**, and **Export World (`.zip`)**, plus `--launch` / `--world` / `--server` direct launch CLI flags; **Crash Auto-Log**, **`INFO`/`WARN`/`ERROR` Log Level Filter**, and automatic **Crash Cause Summary** diagnostics.
- **Full Mod Loader Support, Networking & Protection**
  - **Vanilla**, **Fabric**, **Quilt**, **Forge** (legacy `1.12.2` + modern `--installClient`), **NeoForge**, and managed **OptiFine** validation/install.
  - Parallel library & asset downloads (`mcw-net`) with **HTTP & SOCKS5 Proxy** support (with optional authentication), SHA-1 `VerificationCache`, `DownloadJournal`, pause/resume/cancel, and atomic writes.
  - **Duplicate Mod Guard & Auto-Unique Naming**: Detects duplicate filenames, project IDs, and mod stems across local `.jar` imports and remote installs (`Overwrite Existing`, `Keep Both` with ` (2)` suffix, `Cancel`), and auto-suffixes duplicate instance/modpack names cleanly.
- **Mod & Modpack Ecosystem**
  - Browse & install paginated results (`PageSize = 20`) from **Modrinth**, **CurseForge**, **FTB**, **ATLauncher**, and **Technic Platform / Solder**; import `.mrpack`, `.mcwpack`, CurseForge `.zip`, & **Prism Launcher / MultiMC** `.zip` archives; export instances with a **Selective File Tree** (include/exclude specific folders or files per export).
  - **Protected Curated Modpacks & 5-Provider Updates**: Official and imported `.mcwpack` modpacks default to **`Disable Mod Repair & Mod Updates`** (toggable in `.mcwpack` Export & *Instance Settings*) so per-mod update checks and repair banners stay hidden; official online modpacks (**Modrinth**, **CurseForge**, **FTB**, **ATLauncher**, **Technic**) expose **Check Modpack Update** with an explicit 3-option backup prompt (**`Yes (Launcher Backup)`**, **`I know what I'm doing`**, **`Cancel`**) that preserves saves, configs, screenshots, and options.
- **Account Management & Security**
  - **Microsoft Account (MSA)** via Browser OAuth2 PKCE (`localhost:8400`) and Device Code flow with 2D skin avatars, spec-compliant **Offline** accounts, quick top-bar **Account Switcher**, and per-instance **Preferred Account** override.
  - Refresh tokens encrypted at rest via OS DPAPI + HMAC-SHA256 integrity checks (`SECURITY.md`).
- **Java Runtime Provisioner, Universal Importer & Updater (`auto_check_updates = true` by default)**
  - Automatic Java discovery + managed **Eclipse Temurin** (Java 8 / 17 / 21 / 25 + dynamic latest GA JDK), shared SHA-256 Content Store deduplication, **Universal Launcher Importer** (**Prism Launcher / MultiMC**, **Modrinth App**, **CurseForge App**, **Vanilla `.minecraft`**), **CLI & GUI Python Launcher Importer** (auto-detecting nested Python launcher root from parent folders), and **Auto-Updater enabled by default** via Cloudflare Edge CDN (`mcw-download.pages.dev`) + GitHub Releases failover with SHA-256 verification and transactional `mcw-updater`.

---

## Workspace Architecture

```text
mcw-launcher-rust/
├── apps/
│   ├── desktop/          # Native GUI application (mcw-desktop / MCW Launcher: Rust Edition)
│   ├── updater/          # Standalone transactional updater GUI (mcw-updater)
│   └── cli/              # Headless CLI tool (mcw-cli)
└── crates/
    ├── mcw-models/       # Shared domain models, progress events, app metadata
    ├── mcw-storage/      # Portable paths, atomic I/O, instance store, backups, cleanup, 1.8.x migration
    ├── mcw-net/          # High-concurrency async download engine with SHA-1 verification & bandwidth cap
    ├── mcw-auth/         # Microsoft OAuth (PKCE + Device Code), Xbox/XSTS, Offline auth
    ├── mcw-runtime/      # Java discovery, Temurin provisioner, bytecode check, LAN agent, process supervisor
    ├── mcw-minecraft/    # Mojang manifest, libraries, classpath, NBT servers, Fabric/Quilt/Forge/NeoForge/OptiFine
    ├── mcw-mods/         # Parallel rayon .jar metadata scanner (Fabric/Forge/NeoForge/JiJ) & content managers
    ├── mcw-providers/    # Modrinth, CurseForge, FTB, ATLauncher, Technic, Modpack & Prism .zip importer, SQLite WAL cache
    └── mcw-updater/      # Updater crate (UPDATES_ENABLED = true; Cloudflare Edge CDN + GitHub failover + SHA-256)
```

---

## Quick Start

### Prerequisites

- **Rust toolchain** (`stable`, Rust 1.80+): [rustup.rs](https://rustup.rs/)
- **Linux additional system packages** (Debian/Ubuntu):
  ```bash
  sudo apt-get update
  sudo apt-get install -y build-essential pkg-config libssl-dev \
    libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxkbcommon-dev libgtk-3-dev libgl1-mesa-dev
  ```

### Configure CurseForge API Key (Optional)

CurseForge Mods & Modpacks require a CurseForge Core API v1 key. Copy `.env.example` to `.env` in the launcher directory:

```bash
cp .env.example .env
```

Then edit `.env`:

```env
CURSEFORGE_API_KEY=your_curseforge_api_key_here
```

> `.env` is gitignored and never committed. You can also configure an override key at runtime inside **Launcher Settings → Downloads**.
>
> **Encrypted Embedded Key in Release Binaries**: At compile time, `crates/mcw-providers/build.rs` encrypts `CURSEFORGE_API_KEY` (from the build environment or `.env`) with a fresh per-build SHA-256 CTR keystream so **zero plaintext key bytes** are stored in the compiled `.exe` / Linux binary. For GitHub Actions releases, add `CURSEFORGE_API_KEY` under **Repository Settings -> Secrets and variables -> Actions**.

### Run in Development

```bash
# Desktop GUI
cargo run -p mcw-desktop

# CLI
cargo run -p mcw-cli -- --help
```

### Build Optimized Small Release Binary

`Cargo.toml` is configured with `[profile.release]` (`opt-level = "z"`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = "symbols"`) and `windows_subsystem = "windows"` on Windows for minimal executable size and no console popup:

```bash
cargo build --release -p mcw-desktop -p mcw-updater-app -p mcw-cli
```

Output binaries:
- **Windows**: `target/release/mcw-desktop.exe`, `target/release/mcw-updater.exe`, `target/release/mcw-cli.exe`
- **Linux**: `target/release/mcw-desktop`, `target/release/mcw-updater`, `target/release/mcw-cli`

---

## GitHub Actions CI & Releases

- **CI** (`.github/workflows/ci.yml`): Runs automatically on push/PR — `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` on both `windows-latest` and `ubuntu-latest`.
- **Release** (`.github/workflows/release.yml`): Does **not** trigger on `git push` or tag pushes. Triggers only when a GitHub Release is published (`release: [published]`) or via manual **Actions -> Release Build -> Run workflow** (`workflow_dispatch`) targeting an **already-created GitHub Release**. Validates the SemVer tag against `Cargo.toml`, builds stripped LTO Windows (`x64`) and Linux (`x64`) archives (bundling `themes/`), and uploads binaries + `SHA256SUMS.txt` onto that Release.

---

## Documentation

- [CHANGELOG.md](CHANGELOG.md) — Version history and milestone summaries
- [CONTRIBUTING.md](CONTRIBUTING.md) — Development setup, testing, and PR guidelines
- [DESIGN.md](DESIGN.md) — UI v2.0 visual design system tokens and desktop UX rules
- [SECURITY.md](SECURITY.md) — Security policy and private vulnerability reporting
- [docs/BACKLOG.md](docs/BACKLOG.md) — Remaining backlog items for future releases
- [design-note.md](design-note.md) — Detailed engineering notes per feature

---

## License & Third-Party Fonts

This project is licensed under the **MIT License** — see [LICENSE](LICENSE) for details.

- **JetBrains Mono** (`assets/fonts/JetBrainsMono-Regular.ttf`): Licensed under the **SIL Open Font License 1.1** — see [assets/fonts/JetBrainsMono-OFL.txt](assets/fonts/JetBrainsMono-OFL.txt). Copyright 2020 The JetBrains Mono Project Authors.
