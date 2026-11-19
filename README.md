# MCW Launcher: Rust Edition

[![Status](https://img.shields.io/badge/status-v0.5.0--beta.1-10B981?style=flat-square)](#)
[![Rust](https://img.shields.io/badge/Rust-2021%20Edition-orange?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-blue?style=flat-square)](#)
[![License: MIT](https://img.shields.io/badge/license-MIT-green?style=flat-square)](LICENSE)

**MCW Launcher: Rust Edition** is a fast, lightweight, portable Minecraft launcher rewritten from the ground up in pure Rust (`egui` / `eframe` + `tokio`), designed for multi-instance modpack management, concurrent downloads, and cross-platform reliability on **Windows** and **Linux**.

---

## Key Features

- **Fast Native Desktop UI (`egui` / `eframe`)**
  - Clean Slate Dark (default) & Light themes via the **Launcher Settings** window (top-right gear icon).
  - Instantaneous instance switching with lazy async background scanning for `mods/` and `backups/`.
  - Concurrent multi-install support: install multiple Modpacks, Mods, and Java runtimes simultaneously with a live bottom progress bar (`ACTIVE INSTALLS`).
- **Full Mod Loader Support**
  - **Vanilla**, **Fabric**, **Quilt**, **Forge** (legacy `1.12.2` + modern `--installClient`), and **NeoForge**.
  - Parallel library & asset downloads (`mcw-net`) with SHA-1 verification and safe atomic writes.
- **Mod & Modpack Ecosystem**
  - **Mods**: Browse and install directly from **Modrinth** (with dependency resolution) and **CurseForge**.
  - **Modpacks**: Import `.mrpack` & CurseForge `.zip` archives, or search & install **CurseForge Modpacks**, **FTB**, and **ATLauncher**.
  - **Smart Manual Downloads Handling**: Modpacks with CurseForge opt-out mods are flagged as `Incomplete (N)` until completed; supports per-instance Manual Downloads windows with **Auto-scan** (every 2s) and **Manual Scan** that automatically match and import completed browser downloads.
- **Account Management & Security**
  - **Microsoft Account (MSA)** via Browser OAuth2 PKCE (`localhost:8400`) and Device Code flow.
  - **Offline** accounts with spec-compliant offline UUIDv3 generation.
  - Refresh tokens encrypted at rest (Windows DPAPI on Windows).
- **Java Runtime Provisioner & Process Supervisor**
  - Automatic Java discovery + managed **Eclipse Temurin** (Java 17 / 21) downloads via Adoptium API.
  - `.class` bytecode version inspection (`0xCAFEBABE`) and Win32 Job Object process supervision with live log streaming and crash vs. normal exit classification.
- **Data Safety & Maintenance**
  - Instance **Backup & Safe Rollback Restore** (`mcw-storage`).
  - Instance **Repair** (re-verifies client JAR, libraries, and assets).
  - Read-only **Migration** CLI from legacy MCW Launcher Python `1.8.x`.

---

## Workspace Architecture

```text
mcw-launcher-rust/
├── apps/
│   ├── desktop/          # Native GUI application (mcw-desktop / MCW Launcher: Rust Edition)
│   └── cli/              # Headless CLI tool (mcw-cli)
└── crates/
    ├── mcw-models/       # Shared domain models, progress events, app metadata
    ├── mcw-storage/      # Portable paths, atomic I/O, instance store, backups, 1.8.x migration
    ├── mcw-net/          # High-concurrency async download engine with SHA-1 verification
    ├── mcw-auth/         # Microsoft OAuth (PKCE + Device Code), Xbox/XSTS, Offline auth
    ├── mcw-runtime/      # Java discovery, Temurin provisioner, bytecode check, process supervisor
    ├── mcw-minecraft/    # Mojang manifest, libraries, classpath, Fabric/Quilt/Forge/NeoForge
    ├── mcw-mods/         # Parallel rayon .jar metadata scanner (Fabric/Forge/NeoForge/JiJ)
    ├── mcw-providers/    # Modrinth, CurseForge, FTB, ATLauncher, Modpack installer, SQLite WAL cache
    └── mcw-updater/      # Release checker with CDN + GitHub Releases failover
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

> `.env` is gitignored and never committed.

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

- **CI** (`.github/workflows/ci.yml`): Runs `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` on both `windows-latest` and `ubuntu-latest`.
- **Release** (`.github/workflows/release.yml`): Triggered automatically on version tags (`v*`, e.g. `v0.5.0-beta.1`) or manually via **Actions -> Release -> Run workflow**. Builds stripped, LTO-optimized Windows (`x64`) and Linux (`x64`) release archives with SHA-256 checksums.

---

## Documentation

- [CHANGELOG.md](CHANGELOG.md) — Version history and milestone summaries
- [CONTRIBUTING.md](CONTRIBUTING.md) — Development setup, testing, and PR guidelines
- [DESIGN.md](DESIGN.md) — Visual design system tokens and desktop UX rules
- [design-note.md](design-note.md) — Detailed engineering notes per feature

---

## License

This project is licensed under the **MIT License** — see [LICENSE](LICENSE) for details.
