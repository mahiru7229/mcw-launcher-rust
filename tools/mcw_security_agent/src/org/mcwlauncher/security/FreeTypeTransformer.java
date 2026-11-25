package org.mcwlauncher.security;

import java.lang.instrument.ClassFileTransformer;
import java.security.ProtectionDomain;

final class FreeTypeTransformer implements ClassFileTransformer {
    private static final String TARGET_INTERNAL_NAME = "org/lwjgl/util/freetype/FreeType";
    private final int javaMajor;

    FreeTypeTransformer(int javaMajor) {
        this.javaMajor = javaMajor;
    }

    @Override
    public byte[] transform(
        ClassLoader loader,
        String className,
        Class<?> classBeingRedefined,
        ProtectionDomain protectionDomain,
        byte[] classfileBuffer
    ) {
        if (!TARGET_INTERNAL_NAME.equals(className) || classfileBuffer == null) {
            return null;
        }

        try {
            FreeTypeMethodPatcher.PatchResult result = FreeTypeMethodPatcher.patch(classfileBuffer);
            if (result.patchedCount == 0) {
                String msg = "org.lwjgl.util.freetype.FreeType loaded without matching glyph loader methods";
                McwSecurityAgent.log("WARNING: " + msg);
                AgentStatusWriter.write("FAILED", javaMajor, 0, msg);
                return null;
            }
            String msg = "Minecraft2Shell mitigation active: patched "
                + result.patchedCount
                + " method(s) in org.lwjgl.util.freetype.FreeType -> FT_LOAD_NO_HINTING";
            McwSecurityAgent.log(msg);
            AgentStatusWriter.write("ACTIVE_VERIFIED", javaMajor, result.patchedCount, msg);
            return result.bytecode;
        } catch (Throwable error) {
            String msg = "Error patching FreeType class: " + error.getMessage();
            McwSecurityAgent.log("ERROR: " + msg);
            AgentStatusWriter.write("FAILED", javaMajor, 0, msg);
            return null;
        }
    }
}
