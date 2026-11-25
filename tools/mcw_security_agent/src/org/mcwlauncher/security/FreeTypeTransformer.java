package org.mcwlauncher.security;

import java.lang.instrument.ClassFileTransformer;
import java.security.ProtectionDomain;

final class FreeTypeTransformer implements ClassFileTransformer {
    private static final String TARGET_INTERNAL_NAME = "org/lwjgl/util/freetype/FreeType";

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
                McwSecurityAgent.log("WARNING: org.lwjgl.util.freetype.FreeType loaded without matching glyph loader methods");
                return null;
            }
            McwSecurityAgent.log(
                "Minecraft2Shell mitigation active: patched "
                    + result.patchedCount
                    + " method(s) in org.lwjgl.util.freetype.FreeType -> FT_LOAD_NO_HINTING"
            );
            return result.bytecode;
        } catch (Throwable error) {
            McwSecurityAgent.log("ERROR patching FreeType class: " + error.getMessage());
            return null;
        }
    }
}
