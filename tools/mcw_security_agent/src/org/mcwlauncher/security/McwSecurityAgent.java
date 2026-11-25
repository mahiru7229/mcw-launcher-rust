package org.mcwlauncher.security;

import java.lang.instrument.Instrumentation;

/**
 * Pure-Java security instrumentation agent bundled with MCW Launcher.
 *
 * <p>Currently mitigates the {@code bikini/minecraft2shell} vulnerability
 * (FreeType TrueType hinting bytecode {@code Ins_SHZ} heap OOB write -> RCE)
 * by forcing {@code FT_LOAD_NO_HINTING} on {@code org.lwjgl.util.freetype.FreeType}
 * when {@code -Dmcw.security.freetype_no_hinting=true} is set.</p>
 */
public final class McwSecurityAgent {
    static final String FREETYPE_NO_HINTING_PROPERTY = "mcw.security.freetype_no_hinting";

    private McwSecurityAgent() {
    }

    public static void premain(String agentArgs, Instrumentation instrumentation) {
        if (!Boolean.getBoolean(FREETYPE_NO_HINTING_PROPERTY)) {
            log("FreeType hinting guard inactive (" + FREETYPE_NO_HINTING_PROPERTY + " != true)");
            return;
        }

        instrumentation.addTransformer(new FreeTypeTransformer(), false);
        log("Registered Minecraft2Shell (FreeType SHZ) guard for org.lwjgl.util.freetype.FreeType");
    }

    static void log(String message) {
        System.err.println("[MCW Security Agent] " + message);
    }
}
