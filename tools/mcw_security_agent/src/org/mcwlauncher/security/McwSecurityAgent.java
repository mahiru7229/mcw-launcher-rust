package org.mcwlauncher.security;

import java.lang.instrument.Instrumentation;

/**
 * Pure-Java security instrumentation agent bundled with MCW Launcher.
 *
 * <p>Decoupled from MCW Core and removable cleanly once Mojang ships patched FreeType
 * (>= 2.15.0) across all maintained Minecraft versions. Explicitly separates JVM load
 * ({@code INITIALIZED}) from verified FreeType patching ({@code ACTIVE_VERIFIED}).</p>
 */
public final class McwSecurityAgent {
    static final String FREETYPE_NO_HINTING_PROPERTY = "mcw.security.freetype_no_hinting";
    static final String JAVA_MAJOR_PROPERTY = "mcw.security.java_major";
    private static int detectedJavaMajor = 8;

    private McwSecurityAgent() {
    }

    public static void premain(String agentArgs, Instrumentation instrumentation) {
        detectedJavaMajor = resolveJavaMajor();
        if (detectedJavaMajor < 8) {
            String msg = "Incompatible Java runtime major version (" + detectedJavaMajor + "); requires Java 8+";
            log("ERROR: " + msg);
            AgentStatusWriter.write("INCOMPATIBLE", detectedJavaMajor, 0, msg);
            return;
        }

        if (!Boolean.getBoolean(FREETYPE_NO_HINTING_PROPERTY)) {
            String msg = "FreeType hinting guard dormant (" + FREETYPE_NO_HINTING_PROPERTY + " != true)";
            log(msg);
            AgentStatusWriter.write("DORMANT", detectedJavaMajor, 0, msg);
            return;
        }

        try {
            instrumentation.addTransformer(new FreeTypeTransformer(detectedJavaMajor), false);
            String msg = "Agent initialized (Java " + detectedJavaMajor + "); awaiting org.lwjgl.util.freetype.FreeType class load";
            log(msg);
            AgentStatusWriter.write("INITIALIZED", detectedJavaMajor, 0, msg);
        } catch (Throwable err) {
            String msg = "Failed to register FreeType transformer: " + err.getMessage();
            log("ERROR: " + msg);
            AgentStatusWriter.write("FAILED", detectedJavaMajor, 0, msg);
        }
    }

    static int resolveJavaMajor() {
        String prop = System.getProperty(JAVA_MAJOR_PROPERTY, "").trim();
        if (!prop.isEmpty()) {
            try {
                int parsed = Integer.parseInt(prop);
                if (parsed > 0) {
                    return parsed;
                }
            } catch (NumberFormatException ignored) {
            }
        }
        return parseJavaMajor(System.getProperty("java.specification.version", "1.8"));
    }

    static int parseJavaMajor(String spec) {
        if (spec == null || spec.trim().isEmpty()) {
            return 0;
        }
        String clean = spec.trim();
        if (clean.startsWith("1.")) {
            clean = clean.substring(2);
        }
        int dot = clean.indexOf('.');
        if (dot > 0) {
            clean = clean.substring(0, dot);
        }
        try {
            return Integer.parseInt(clean);
        } catch (NumberFormatException ignored) {
            return 0;
        }
    }

    static void log(String message) {
        System.err.println("[MCW Security Agent] " + message);
    }
}
