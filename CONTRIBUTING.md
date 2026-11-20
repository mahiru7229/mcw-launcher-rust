# Contributing to MCW Launcher: Rust Edition

Thank you for your interest in contributing to **MCW Launcher: Rust Edition**!

## Development Setup

1. Install the latest stable Rust toolchain via [rustup](https://rustup.rs/).
2. On **Linux (Debian/Ubuntu)**, install the required native GUI/TLS development packages:
   ```bash
   sudo apt-get update
   sudo apt-get install -y build-essential pkg-config libssl-dev \
     libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
     libxkbcommon-dev libgtk-3-dev libgl1-mesa-dev
   ```
3. (Optional) Copy `.env.example` to `.env` and add your `CURSEFORGE_API_KEY` for testing CurseForge mod/modpack search and installation.

## Quality Checks

Before opening a pull request, please ensure all checks pass with zero warnings:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Guidelines

- **Python Reference Parity (Mandatory Rule)**: Everything in this repository must follow **100% (or as closely as possible)** the codebase, business logic, schemas, translations (`lang/*.json`), and edge-cases of **MCW Launcher Python** (`d:\WORKBASE\mcw-launcher`). Always reuse existing Python logic/resources instead of inventing new implementations, unless a Python approach is unsuitable for Rust/native constraints (in which case preserve equivalent behavior).
- **Cross-platform compatibility**: Both Windows and Linux are first-class targets. Gate any OS-specific code cleanly with `#[cfg(windows)]` / `#[cfg(not(windows))]` or `#[cfg(target_os = "...")]`.
- **Non-blocking UI**: Never run heavy file I/O (e.g. `.jar` scanning, archive extraction, network calls) synchronously on the `egui` UI thread; offload to Tokio (`runtime.spawn` / `spawn_blocking`) and send results via `AppEvent`.
- **GUI Design System**: Follow [DESIGN.md](DESIGN.md) and [GUI_AGENTS.md](GUI_AGENTS.md) for visual consistency (Slate neutrals, Emerald accent, crisp `2–4px` radii, `1px` borders, auto-sized controls).
- **Never commit secrets**: `.env` and portable runtime folders (`accounts/`, `instances/`, `cache/`, etc.) must remain untracked.
