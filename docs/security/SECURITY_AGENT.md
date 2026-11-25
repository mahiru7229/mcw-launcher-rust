# MCW Security Agent — Design & Runtime Verification

## 1. Design Goals

- **Independent Artifact**: Packaged as a standalone pure-Java JAR (`mcw-security-agent.jar`) compiled with `--release 8` and zero third-party Java dependencies (no ASM, no Mixin, no reflection hacks).
- **Decoupled from MCW Core**: Can be updated, disabled via Launcher Settings -> Security, deactivated automatically per instance when Mojang's patched FreeType is verified, or completely uninstalled (`SecurityAgentManager::uninstall`) without breaking Core launching, Instance Manager, or Library Manager.
- **No Arbitrary Remote Code Execution**: Contains deterministic local bytecode instrumentation only; never downloads or executes code at runtime.

## 2. Mitigation Mechanism

`FreeTypeTransformer` inspects only `org/lwjgl/util/freetype/FreeType` during JVM class loading. `FreeTypeMethodPatcher` targets the non-native static glyph loading methods:

- `nFT_Load_Glyph(JII)I` (prepends `iload_3; iconst_2; ior; istore_3`)
- `nFT_Load_Char(JJI)I` (prepends `iload 4; iconst_2; ior; istore 4; nop; nop`)
- `FT_Load_Glyph(Lorg/lwjgl/util/freetype/FT_Face;II)I` (prepends `iload_2; iconst_2; ior; istore_2`)
- `FT_Load_Char(Lorg/lwjgl/util/freetype/FT_Face;JI)I` (prepends `iload_3; iconst_2; ior; istore_3`)

Setting bit `0x2` (`FT_LOAD_NO_HINTING`) preserves all color/emoji/bitmap rendering flags (`FT_LOAD_RENDER`, `FT_LOAD_COLOR`, etc.) while instructing FreeType to skip the TrueType hinting bytecode VM (`Ins_SHZ`). `FreeTypeMethodPatcher` updates `max_stack`, `code_length`, `exception_table`, `LineNumberTable`, `LocalVariableTable`, and `StackMapTable` first-frame `offset_delta` so `-Xverify:all` passes cleanly across Java 8 through Java 25+.

## 3. Compatibility & Verified Status Handshake

MCW Launcher **never** marks an instance protected merely because `-javaagent:...` was passed on the command line:

1. **Pre-Launch**:
   - `SecurityAgentManager::ensure_installed` verifies the extracted `mcw-security-agent.jar` matches the compiled SHA-256 digest (`8341...` updated per build).
   - `SecurityAgentManager::check_java_compatibility` verifies `java_major >= 8`.
   - `SecurityAgentManager::reset_status_file` deletes any stale `<instance_dir>/logs/mcw-security-agent-status.json` from prior runs.
2. **Runtime Handshake (`mcw-security-agent-status.json`)**:
   - `premain` checks `java.specification.version >= 8`; if invalid, writes `INCOMPATIBLE` and stops.
   - If `-Dmcw.security.freetype_no_hinting=true` is missing, writes `DORMANT` and stays inert.
   - Upon registering `FreeTypeTransformer`, writes `INITIALIZED` (`patched_methods = 0`).
   - Only when `org/lwjgl/util/freetype/FreeType` is loaded and `patchedCount > 0` does `AgentStatusWriter` atomically record **`ACTIVE_VERIFIED`** with exact `patched_methods` count.
   - If instrumentation fails or `patchedCount == 0`, it writes **`FAILED`** with error details and returns `null` so the JVM continues safely while MCW flags the session as `UNPROTECTED`.

## 4. Independent Disabling & Removal

Users or automated lifecycle transitions can:
- Toggle **Enable Minecraft2Shell mitigation on launch** off in *Launcher Settings -> Security*.
- Omit the agent automatically per instance once Mojang's patched `lwjgl-freetype` (`2.15.0+`) is verified.
- Remove the cached JAR completely via `SecurityAgentManager::uninstall` (exposed in Settings -> Security).
