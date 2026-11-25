package org.mcwlauncher.security;

import java.io.ByteArrayOutputStream;
import java.nio.charset.StandardCharsets;

/**
 * Zero-dependency JVM classfile rewriter that injects {@code load_flags |= 0x2}
 * ({@code FT_LOAD_NO_HINTING}) at entry of LWJGL's FreeType glyph loading methods.
 */
final class FreeTypeMethodPatcher {
    private static final int CLASS_MAGIC = 0xCAFEBABE;
    private static final int ACC_STATIC = 0x0008;
    private static final int ACC_NATIVE = 0x0100;

    // iload_2; iconst_2; ior; istore_2 (4 bytes, preserves 4-byte switch alignment)
    private static final byte[] PREFIX_ARG2 = new byte[] {
        0x1C, 0x05, (byte) 0x80, 0x3D
    };
    // iload_3; iconst_2; ior; istore_3 (4 bytes, preserves 4-byte switch alignment)
    private static final byte[] PREFIX_ARG3 = new byte[] {
        0x1D, 0x05, (byte) 0x80, 0x3E
    };
    // iload 4; iconst_2; ior; istore 4; nop; nop (8 bytes, preserves 4-byte switch alignment)
    private static final byte[] PREFIX_ARG4 = new byte[] {
        0x15, 0x04, 0x05, (byte) 0x80, 0x36, 0x04, 0x00, 0x00
    };

    private FreeTypeMethodPatcher() {
    }

    static final class PatchResult {
        final byte[] bytecode;
        final int patchedCount;

        PatchResult(byte[] bytecode, int patchedCount) {
            this.bytecode = bytecode;
            this.patchedCount = patchedCount;
        }
    }

    static PatchResult patch(byte[] original) {
        Cursor cursor = new Cursor(original);
        if (cursor.readU4() != CLASS_MAGIC) {
            throw new IllegalArgumentException("invalid class-file magic");
        }
        cursor.skip(4); // minor + major

        ConstantPool pool = ConstantPool.read(cursor);
        cursor.skip(6); // access_flags, this_class, super_class

        int interfaceCount = cursor.readU2();
        cursor.skip(interfaceCount * 2);
        skipMembers(cursor); // fields

        ByteArrayOutputStream out = new ByteArrayOutputStream(original.length + 128);
        int copyFrom = 0;
        int patchedCount = 0;

        int methodCount = cursor.readU2();
        for (int i = 0; i < methodCount; i++) {
            int accessFlags = cursor.readU2();
            String methodName = pool.utf8(cursor.readU2());
            String descriptor = pool.utf8(cursor.readU2());
            int attributeCount = cursor.readU2();

            byte[] prefix = resolvePrefix(accessFlags, methodName, descriptor);
            for (int attr = 0; attr < attributeCount; attr++) {
                String attrName = pool.utf8(cursor.readU2());
                int attrLen = cursor.readU4();
                int attrStart = cursor.position();
                int attrEnd = attrStart + attrLen;

                if (prefix != null && "Code".equals(attrName)) {
                    byte[] rebuiltCode = rebuildCodeAttribute(original, attrStart, attrLen, pool, prefix);
                    out.write(original, copyFrom, attrStart - 4 - copyFrom);
                    writeU4(out, rebuiltCode.length);
                    out.write(rebuiltCode, 0, rebuiltCode.length);
                    copyFrom = attrEnd;
                    patchedCount++;
                }

                cursor.moveTo(attrEnd);
            }
        }

        if (patchedCount == 0) {
            return new PatchResult(original, 0);
        }

        out.write(original, copyFrom, original.length - copyFrom);
        return new PatchResult(out.toByteArray(), patchedCount);
    }

    private static byte[] resolvePrefix(int accessFlags, String name, String descriptor) {
        if ((accessFlags & ACC_STATIC) == 0 || (accessFlags & ACC_NATIVE) != 0) {
            return null;
        }
        if ("nFT_Load_Glyph".equals(name) && "(JII)I".equals(descriptor)) {
            return PREFIX_ARG3;
        }
        if ("nFT_Load_Char".equals(name) && "(JJI)I".equals(descriptor)) {
            return PREFIX_ARG4;
        }
        if ("FT_Load_Glyph".equals(name) && "(Lorg/lwjgl/util/freetype/FT_Face;II)I".equals(descriptor)) {
            return PREFIX_ARG2;
        }
        if ("FT_Load_Char".equals(name) && "(Lorg/lwjgl/util/freetype/FT_Face;JI)I".equals(descriptor)) {
            return PREFIX_ARG3;
        }
        return null;
    }

    private static byte[] rebuildCodeAttribute(
        byte[] original,
        int attrStart,
        int attrLen,
        ConstantPool pool,
        byte[] prefix
    ) {
        int attrEnd = attrStart + attrLen;
        Cursor codeCursor = new Cursor(original, attrStart, attrEnd);
        int maxStack = codeCursor.readU2();
        int maxLocals = codeCursor.readU2();
        int codeLen = codeCursor.readU4();
        int codeStart = codeCursor.position();
        int codeEnd = codeStart + codeLen;
        if (codeEnd > attrEnd) {
            throw new IllegalArgumentException("malformed Code attribute");
        }

        int delta = prefix.length;
        ByteArrayOutputStream out = new ByteArrayOutputStream(attrLen + delta + 8);
        writeU2(out, Math.max(maxStack, 2));
        writeU2(out, maxLocals);
        writeU4(out, codeLen + delta);
        out.write(prefix, 0, delta);
        out.write(original, codeStart, codeLen);

        codeCursor.moveTo(codeEnd);
        int exCount = codeCursor.readU2();
        writeU2(out, exCount);
        for (int i = 0; i < exCount; i++) {
            int startPc = codeCursor.readU2();
            int endPc = codeCursor.readU2();
            int handlerPc = codeCursor.readU2();
            int catchType = codeCursor.readU2();
            writeU2(out, startPc + delta);
            writeU2(out, endPc + delta);
            writeU2(out, handlerPc + delta);
            writeU2(out, catchType);
        }

        int subAttrCount = codeCursor.readU2();
        writeU2(out, subAttrCount);
        for (int i = 0; i < subAttrCount; i++) {
            int subNameIdx = codeCursor.readU2();
            String subName = pool.utf8(subNameIdx);
            int subLen = codeCursor.readU4();
            int subStart = codeCursor.position();
            int subEnd = subStart + subLen;

            byte[] rewrittenSub = rewriteSubAttribute(original, subStart, subLen, subName, delta);
            writeU2(out, subNameIdx);
            writeU4(out, rewrittenSub.length);
            out.write(rewrittenSub, 0, rewrittenSub.length);

            codeCursor.moveTo(subEnd);
        }

        return out.toByteArray();
    }

    private static byte[] rewriteSubAttribute(
        byte[] original,
        int subStart,
        int subLen,
        String subName,
        int delta
    ) {
        if ("LineNumberTable".equals(subName)) {
            Cursor c = new Cursor(original, subStart, subStart + subLen);
            int count = c.readU2();
            ByteArrayOutputStream out = new ByteArrayOutputStream(subLen);
            writeU2(out, count);
            for (int i = 0; i < count; i++) {
                writeU2(out, c.readU2() + delta);
                writeU2(out, c.readU2());
            }
            return out.toByteArray();
        }

        if ("LocalVariableTable".equals(subName) || "LocalVariableTypeTable".equals(subName)) {
            Cursor c = new Cursor(original, subStart, subStart + subLen);
            int count = c.readU2();
            ByteArrayOutputStream out = new ByteArrayOutputStream(subLen);
            writeU2(out, count);
            for (int i = 0; i < count; i++) {
                int startPc = c.readU2();
                int length = c.readU2();
                int nameIdx = c.readU2();
                int descIdx = c.readU2();
                int index = c.readU2();
                if (startPc == 0) {
                    writeU2(out, 0);
                    writeU2(out, length + delta);
                } else {
                    writeU2(out, startPc + delta);
                    writeU2(out, length);
                }
                writeU2(out, nameIdx);
                writeU2(out, descIdx);
                writeU2(out, index);
            }
            return out.toByteArray();
        }

        if ("StackMapTable".equals(subName)) {
            return rewriteStackMapTable(original, subStart, subLen, delta);
        }

        byte[] copy = new byte[subLen];
        System.arraycopy(original, subStart, copy, 0, subLen);
        return copy;
    }

    private static byte[] rewriteStackMapTable(byte[] original, int subStart, int subLen, int delta) {
        if (subLen < 2) {
            byte[] copy = new byte[subLen];
            System.arraycopy(original, subStart, copy, 0, subLen);
            return copy;
        }
        Cursor c = new Cursor(original, subStart, subStart + subLen);
        int entryCount = c.readU2();
        if (entryCount == 0) {
            byte[] copy = new byte[subLen];
            System.arraycopy(original, subStart, copy, 0, subLen);
            return copy;
        }

        ByteArrayOutputStream out = new ByteArrayOutputStream(subLen + 4);
        writeU2(out, entryCount);

        // Shift ONLY the first stack_map_frame's offset_delta by +delta.
        // All subsequent frames store relative deltas between frames and stay valid.
        int frameType = c.readU1();
        if (frameType <= 63) {
            int newDelta = frameType + delta;
            if (newDelta <= 63) {
                out.write(newDelta);
            } else {
                out.write(251); // same_frame_extended
                writeU2(out, newDelta);
            }
        } else if (frameType <= 127) {
            int origDelta = frameType - 64;
            int newDelta = origDelta + delta;
            if (newDelta <= 63) {
                out.write(64 + newDelta);
            } else {
                out.write(247); // same_locals_1_stack_item_frame_extended
                writeU2(out, newDelta);
            }
        } else if (frameType == 247 || (frameType >= 248 && frameType <= 255)) {
            out.write(frameType);
            int origDelta = c.readU2();
            writeU2(out, origDelta + delta);
        } else {
            throw new IllegalArgumentException("unsupported StackMapTable frame_type: " + frameType);
        }

        int remainingStart = c.position();
        int remainingLen = (subStart + subLen) - remainingStart;
        if (remainingLen > 0) {
            out.write(original, remainingStart, remainingLen);
        }
        return out.toByteArray();
    }

    private static void skipMembers(Cursor cursor) {
        int count = cursor.readU2();
        for (int index = 0; index < count; index++) {
            cursor.skip(6);
            int attributes = cursor.readU2();
            for (int attribute = 0; attribute < attributes; attribute++) {
                cursor.skip(2);
                int length = cursor.readU4();
                cursor.skip(length);
            }
        }
    }

    private static void writeU2(ByteArrayOutputStream out, int value) {
        out.write((value >>> 8) & 0xFF);
        out.write(value & 0xFF);
    }

    private static void writeU4(ByteArrayOutputStream out, int value) {
        out.write((value >>> 24) & 0xFF);
        out.write((value >>> 16) & 0xFF);
        out.write((value >>> 8) & 0xFF);
        out.write(value & 0xFF);
    }

    private static final class ConstantPool {
        private final int[] tags;
        private final String[] utf8;

        private ConstantPool(int count) {
            this.tags = new int[count];
            this.utf8 = new String[count];
        }

        static ConstantPool read(Cursor cursor) {
            int count = cursor.readU2();
            ConstantPool pool = new ConstantPool(count);
            for (int index = 1; index < count; index++) {
                int tag = cursor.readU1();
                pool.tags[index] = tag;
                switch (tag) {
                    case 1: {
                        int length = cursor.readU2();
                        pool.utf8[index] = new String(cursor.readBytes(length), StandardCharsets.UTF_8);
                        break;
                    }
                    case 3:
                    case 4:
                        cursor.skip(4);
                        break;
                    case 5:
                    case 6:
                        cursor.skip(8);
                        index++;
                        break;
                    case 7:
                    case 8:
                    case 16:
                    case 19:
                    case 20:
                        cursor.skip(2);
                        break;
                    case 9:
                    case 10:
                    case 11:
                    case 12:
                    case 17:
                    case 18:
                        cursor.skip(4);
                        break;
                    case 15:
                        cursor.skip(3);
                        break;
                    default:
                        throw new IllegalArgumentException("unsupported constant-pool tag " + tag);
                }
            }
            return pool;
        }

        String utf8(int index) {
            if (index <= 0 || index >= utf8.length || tags[index] != 1) {
                throw new IllegalArgumentException("invalid UTF-8 constant-pool reference");
            }
            return utf8[index];
        }
    }

    private static final class Cursor {
        private final byte[] data;
        private final int limit;
        private int offset;

        Cursor(byte[] data) {
            this(data, 0, data.length);
        }

        Cursor(byte[] data, int offset, int limit) {
            this.data = data;
            this.offset = offset;
            this.limit = Math.min(limit, data.length);
        }

        int position() {
            return offset;
        }

        void moveTo(int newOffset) {
            if (newOffset < 0 || newOffset > limit) {
                throw new IllegalArgumentException("class-file seek outside bounds");
            }
            offset = newOffset;
        }

        int readU1() {
            require(1);
            return data[offset++] & 0xFF;
        }

        int readU2() {
            require(2);
            int value = ((data[offset] & 0xFF) << 8) | (data[offset + 1] & 0xFF);
            offset += 2;
            return value;
        }

        int readU4() {
            require(4);
            int value = ((data[offset] & 0xFF) << 24)
                | ((data[offset + 1] & 0xFF) << 16)
                | ((data[offset + 2] & 0xFF) << 8)
                | (data[offset + 3] & 0xFF);
            offset += 4;
            return value;
        }

        byte[] readBytes(int length) {
            require(length);
            byte[] slice = new byte[length];
            System.arraycopy(data, offset, slice, 0, length);
            offset += length;
            return slice;
        }

        void skip(int length) {
            require(length);
            offset += length;
        }

        private void require(int length) {
            if (length < 0 || offset + length > limit) {
                throw new IllegalArgumentException("truncated class file");
            }
        }
    }
}
