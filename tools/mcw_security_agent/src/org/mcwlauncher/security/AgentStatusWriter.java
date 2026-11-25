package org.mcwlauncher.security;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardCopyOption;

/**
 * Writes structured JSON runtime status for {@link McwSecurityAgent} without
 * third-party dependencies so MCW Security Manager can distinguish mere JVM loading
 * ({@code INITIALIZED}) from verified FreeType bytecode instrumentation
 * ({@code ACTIVE_VERIFIED}) or failure states.
 */
final class AgentStatusWriter {
    static final String STATUS_FILE_PROPERTY = "mcw.security.status_file";
    private static final Object LOCK = new Object();

    private AgentStatusWriter() {
    }

    static void write(String state, int javaMajor, int patchedMethods, String detail) {
        String configured = System.getProperty(STATUS_FILE_PROPERTY, "").trim();
        if (configured.isEmpty()) {
            return;
        }
        String javaVersion = System.getProperty("java.version", "unknown");
        long nowMs = System.currentTimeMillis();
        String json = "{\n"
            + "  \"schema_version\": 1,\n"
            + "  \"state\": \"" + escapeJson(state) + "\",\n"
            + "  \"java_version\": \"" + escapeJson(javaVersion) + "\",\n"
            + "  \"java_major\": " + javaMajor + ",\n"
            + "  \"patched_methods\": " + patchedMethods + ",\n"
            + "  \"detail\": \"" + escapeJson(detail) + "\",\n"
            + "  \"updated_epoch_ms\": " + nowMs + "\n"
            + "}\n";

        synchronized (LOCK) {
            try {
                Path target = Paths.get(configured).toAbsolutePath().normalize();
                Path parent = target.getParent();
                if (parent != null) {
                    Files.createDirectories(parent);
                }
                Path tmp = target.resolveSibling(target.getFileName().toString() + ".tmp");
                Files.write(tmp, json.getBytes(StandardCharsets.UTF_8));
                try {
                    Files.move(tmp, target, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
                } catch (IOException atomicUnsupported) {
                    Files.move(tmp, target, StandardCopyOption.REPLACE_EXISTING);
                }
            } catch (IOException | RuntimeException err) {
                McwSecurityAgent.log("WARNING: could not write status file: " + err.getMessage());
            }
        }
    }

    private static String escapeJson(String input) {
        if (input == null) {
            return "";
        }
        StringBuilder sb = new StringBuilder(input.length() + 8);
        for (int i = 0; i < input.length(); i++) {
            char c = input.charAt(i);
            switch (c) {
                case '\\':
                    sb.append("\\\\");
                    break;
                case '"':
                    sb.append("\\\"");
                    break;
                case '\n':
                    sb.append("\\n");
                    break;
                case '\r':
                    sb.append("\\r");
                    break;
                case '\t':
                    sb.append("\\t");
                    break;
                default:
                    if (c < 0x20) {
                        sb.append(String.format("\\u%04x", (int) c));
                    } else {
                        sb.append(c);
                    }
                    break;
            }
        }
        return sb.toString();
    }
}
