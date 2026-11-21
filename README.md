# MCW Launcher: Rust Edition

[![Status](https://img.shields.io/badge/status-v0.13.2--beta.1-7C3AED?style=flat-square)](#)
[![Rust](https://img.shields.io/badge/Rust-2021%20Edition-orange?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-blue?style=flat-square)](#)
[![License: MIT](https://img.shields.io/badge/license-MIT-green?style=flat-square)](LICENSE)

**MCW Launcher: Rust Edition** is a fast, lightweight, portable Minecraft launcher rewritten from the ground up in pure Rust (`egui` / `eframe` + `tokio`), designed for multi-instance modpack management, concurrent downloads, full MCW Launcher Python feature parity, and cross-platform reliability on **Windows** and **Linux**.

---

## Performance & Footprint (vs. MCW Launcher Python)

| Metric | MCW Launcher (Python / PySide6) | MCW Launcher: Rust Edition (`v0.13.2-beta.1`) |
| --- | --- | --- |
| **Idle / Active RAM** | ~150 MB | **~60–70 MB** (~55–60% lower memory footprint) |
| **Runtime Performance** | Baseline | **~2× faster** UI responsiveness, mod scanning, and concurrent downloads |
| **Package Size** | Larger (bundled Python + Qt runtime) | **Lighter (9.72 MiB / 10.19 MB single binary)** — zero bundled Python/Qt runtime |

---

## Key Features

- **UI v2.0 Native Desktop Interface (`egui` / `eframe`)**
  - **Pure Black Default & Custom Background**: Pure `#000000` black backdrop by default in Dark mode, or asynchronous user PNG/JPEG/WebP wallpaper with *Panel transparency* and *Background dimming* sliders; 6 built-in presets (**Violet** default, **Sunrise**, **Neon**, **Ocean**, **Emerald**, **Light**), `#RRGGBB` custom accent override, and `themes/*.json` pack loading; screen-aware initial window sizing (`1600x900` on Full HD+, `1280x720` on `1366x768+`).
  - **Clean Shell & Navigation**: Floating top pill bar (**Home**, **Mod & Modpacks**, **Versions**, **Logs**), slim left icon rail with status indicators and context menu, slide-out Instance Flyout (search, favorites, responsive Group/Tag pills), **in-app Launcher Settings modal** with cross-category right-pane content search, and **`Ctrl+K` Global Quick Search**.
  - **Dedicated Pages & Floating Tabbed Home**: Home hero header (`Play`, `•••` Actions menu, quick Add Content / Open Folder / Gear **Instance Settings** icons, `Mods`/`Worlds`/`Servers & LAN`/`Resource packs`/`Shaders`/`Screenshots`/`Backups`/`Logs` tabs), **Servers & LAN** per-instance tab (`servers.dat` NBT + Quick Play + instant-apply Manual MCW LAN Agent), **Versions** (single vertical scroll covering collapsible `Cached Versions` + Mojang manifest), and unified **Logs** view.
  - **Dedicated Centered OS Windows**: Isolated OS viewports for paginated **Mod & Modpacks** (`Mods`, `Modpacks`, `Resource packs`, `Shader packs`, `Data packs` with responsive pill header, `Previous`/`Next` pagination, and 2-step **Install Destination** picker enforcing exact MC version + loader compatibility), dedicated **Java Runtimes** manager (with dynamic Adoptium Latest JDK lookup), Modpack Install (with `JVM Preset` selector), origin-aware Export Modpack, Accounts, Create Instance, Manual Downloads, Repair Center, Global Logs, Python-parity 6-step **First-Run Setup Wizard** (including native dedicated GPU detection), and LAN Agent Log.
  - **Simple Animations Only**: Hover/fade (`~120 ms`), modal/page fade-in (`~150 ms`), and smooth animated task progress bars honoring `MotionMode` (**Full** / **Reduced** / **Off**).
- **Full Mod Loader Support & Safety**
  - **Vanilla**, **Fabric**, **Quilt**, **Forge** (legacy `1.12.2` + modern `--installClient`), **NeoForge**, and managed **OptiFine** validation/install.
  - Parallel library & asset downloads (`mcw-net`) with SHA-1 `VerificationCache`, pause/resume/cancel, and safe atomic writes.
- **Mod & Modpack Ecosystem**
  - Browse & install paginated results (`PageSize = 20`) from **Modrinth** (with dependency resolution) and **CurseForge**; import `.mrpack`, `.mcwpack`, & CurseForge `.zip` archives; search & install **FTB** and **ATLauncher** packs with optional-file selection.
  - **Incomplete Modpack Guard & Manual Downloads**: Tracks CurseForge opt-out mods (`Incomplete (N)`), auto-scans `Downloads/` every 2s, and imports matching files cleanly.
- **Account Management & Security**
  - **Microsoft Account (MSA)** via Browser OAuth2 PKCE (`localhost:8400`) and Device Code flow with 2D skin avatars.
  - **Offline** accounts with spec-compliant offline UUIDv3 generation.
  - Refresh tokens encrypted at rest via OS DPAPI + HMAC-SHA256 integrity checks.
- **Java Runtime Provisioner & Process Supervisor**
  - Automatic Java discovery + managed **Eclipse Temurin** (Java 8 / 17 / 21 / 25 + dynamic latest GA JDK via Adoptium `available_releases` API) inside the isolated `Java Runtimes` OS window.
  - `.class` bytecode inspection (`0xCAFEBABE`), JVM flag presets, and Win32 Job Object process supervision with live log streaming, crash classification, `mclo.gs` upload, and diagnostic `.zip` export.
- **Storage, Maintenance & Updater Status**
  - **Shared Content Store** SHA-256 hardlink deduplication, **Backup & Safe Rollback Restore**, **Instance Repair & Mod Compatibility Analyzer**, and read-only **Python 1.8.x Migration** (`mcw-cli migrate`).
  - **Updater Status**: Launcher self-updates are currently **disabled** (`mcw_updater::UPDATES_ENABLED = false`) for MCW Launcher: Rust Edition until dedicated Rust Edition releases are published.

---

## Workspace Architecture

```text
mcw-launcher-rust/
├── apps/
│   ├── desktop/          # Native GUI application (mcw-desktop / MCW Launcher: Rust Edition)
│   └── cli/              # Headless CLI tool (mcw-cli)
└── crates/
    ├── mcw-models/       # Shared domain models, progress events, app metadata
    ├── mcw-storage/      # Portable paths, atomic I/O, instance store, backups, cleanup, 1.8.x migration
    ├── mcw-net/          # High-concurrency async download engine with SHA-1 verification & bandwidth cap
    ├── mcw-auth/         # Microsoft OAuth (PKCE + Device Code), Xbox/XSTS, Offline auth
    ├── mcw-runtime/      # Java discovery, Temurin provisioner, bytecode check, LAN agent, process supervisor
    ├── mcw-minecraft/    # Mojang manifest, libraries, classpath, NBT servers, Fabric/Quilt/Forge/NeoForge/OptiFine
    ├── mcw-mods/         # Parallel rayon .jar metadata scanner (Fabric/Forge/NeoForge/JiJ) & content managers
    ├── mcw-providers/    # Modrinth, CurseForge, FTB, ATLauncher, Modpack installer, SQLite WAL cache
    └── mcw-updater/      # Updater crate (UPDATES_ENABLED = false; Rust-Edition GitHub Releases scaffold)
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
cargo build --release -p mcw-desktop
```

Output binary:
- **Windows**: `target/release/mcw-desktop.exe`
- **Linux**: `target/release/mcw-desktop`

---

## GitHub Actions CI & Releases

- **CI** (`.github/workflows/ci.yml`): Runs automatically on push/PR — `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` on both `windows-latest` and `ubuntu-latest`.
- **Release** (`.github/workflows/release.yml`): **Manual-only** via **Actions -> Release Build -> Run workflow** (`workflow_dispatch`). Builds stripped, LTO-optimized Windows (`x64`) and Linux (`x64`) release archives with SHA-256 checksums.

---

## Documentation

- [CHANGELOG.md](CHANGELOG.md) — Version history and milestone summaries
- [CONTRIBUTING.md](CONTRIBUTING.md) — Development setup, testing, and PR guidelines
- [DESIGN.md](DESIGN.md) — UI v2.0 visual design system tokens and desktop UX rules
- [docs/BACKLOG.md](docs/BACKLOG.md) — Remaining backlog items for future releases
- [design-note.md](design-note.md) — Detailed engineering notes per feature

---

## License

This project is licensed under the **MIT License** — see [LICENSE](LICENSE) for details.
