# Threat Model — `bikini/minecraft2shell` (FreeType Native RCE)

## 1. Vulnerability Summary

- **Identifier**: `bikini/minecraft2shell` (public PoC disclosed November 22, 2026)
- **Component**: Native FreeType `< 2.15.0` bundled via `org.lwjgl:lwjgl-freetype` (2.13.x – 2.14.3 in LWJGL `3.3.3`–`3.4.3`)
- **Weakness**: Heap out-of-bounds write during execution of the TrueType hinting bytecode instruction **`SHZ` (`Ins_SHZ` / Shift Zone)** on specially crafted composite glyph outlines.
- **Upstream Fix**: FreeType commit `1803559c4ee4`, released in **FreeType 2.15.0**.

## 2. Attack Vectors & Impact

1. **Untrusted Multiplayer Servers with Auto-Accept Resource Packs (`acceptTextures = 1`)**
   - A malicious server sends a server resource pack containing a crafted `.ttf` TrueType font upon player join.
   - When the client renders text referencing that font (chat, scoreboard, action bar, title), FreeType interprets hinting bytecode -> heap OOB write -> native ASLR-bypass ROP chain -> **arbitrary native code execution (RCE)** on the player's OS.
2. **Untrusted Local Resource Packs / Modpacks**
   - Opening a manually downloaded untrusted resource pack or modpack containing a malicious `.ttf` font triggers the same native FreeType code path.

## 3. Affected Versions (Verified Against `piston-meta.mojang.com` on 2026-11-25)

| Minecraft Java Versions | Bundled `org.lwjgl:lwjgl-freetype` | Upstream Status | MCW Security Handling |
|---|---|---|---|
| `<= 1.20.4` | Not present (`lwjgl-stb` used) | Unaffected | `PROTECTED` (Agent not attached) |
| `1.20.5` (`24w03a`) – `1.21.11` | `org.lwjgl:lwjgl-freetype:3.3.3` | Vulnerable (FreeType `< 2.15.0`) | `PARTIALLY_PROTECTED` via Security Agent + Server RP warning |
| `26.1` – `26.2` | `org.lwjgl:lwjgl-freetype:3.4.1` | Vulnerable (FreeType `< 2.15.0`) | `PARTIALLY_PROTECTED` via Security Agent + Server RP warning |
| `26.3`, `26.4-pre-1` (updated metadata) | `org.lwjgl:lwjgl-freetype:3.4.3-mojang-2.15.0` | **Patched** (`FreeType 2.15.0`) | `PROTECTED` once SHA-1 verified on disk (Agent auto-disabled) |

## 4. Trust Boundaries & Security Principles

- **Mojang Metadata & Artifacts**: Treated as trusted only after TLS fetch from `piston-meta.mojang.com` / `libraries.minecraft.net` AND strict SHA-1 checksum verification of both `version.json` and downloaded artifacts.
- **Remote Security Policy**: Treated as untrusted until verified with an embedded **Ed25519** public key (`ring::signature::ED25519`), bounded by explicit expiration (`expires_at_epoch`). Policy files contain declarative metadata ONLY—remote servers can **never** instruct MCW Launcher to execute code or commands.
- **Pre-Launch Server Resource Pack Guard**: Because users may intentionally disable the runtime agent or face unpatched edge cases, MCW Launcher inspects `servers.dat` (`acceptTextures == Some(1)`) on any version using `lwjgl-freetype` and requires explicit user confirmation before launch.

## 5. Known Limitations

- Setting `FT_LOAD_NO_HINTING` prevents execution of the TrueType hinting bytecode interpreter (`Ins_SHZ` and related opcodes), blocking the `minecraft2shell` exploit chain cleanly. However, runtime flag forcing is not a full replacement for a patched native `FreeType >= 2.15.0` binary against hypothetical non-hinting FreeType bugs. Consequently, MCW reports mitigated unpatched versions honestly as **`PARTIALLY_PROTECTED`**, never `PROTECTED`.
- If a third-party modloader or custom `version.json` overrides `org.lwjgl:lwjgl-freetype` back to an older version, MCW Security Manager evaluates the **actual merged dependency list** rather than assuming safety from the base Minecraft version string.
