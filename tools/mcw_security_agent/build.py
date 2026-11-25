from __future__ import annotations

from pathlib import Path
import hashlib
import shutil
import subprocess
import sys
import tempfile
import textwrap
import zipfile


ROOT = Path(__file__).resolve().parents[2]
TOOL_ROOT = Path(__file__).resolve().parent
SOURCE_ROOT = TOOL_ROOT / "src"
OUTPUT_JAR = ROOT / "crates" / "mcw-runtime" / "assets" / "mcw-security-agent.jar"
MAIN_CLASS = "org.mcwlauncher.security.McwSecurityAgent"


def run(command: list[str], cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, cwd=cwd, check=True, capture_output=True, text=True)


def build() -> str:
    javac = shutil.which("javac")
    if javac is None:
        raise RuntimeError("javac was not found. Install a JDK to build the MCW Security Agent.")

    sources = sorted(SOURCE_ROOT.rglob("*.java"))
    if not sources:
        raise RuntimeError("No Java security agent sources were found.")

    OUTPUT_JAR.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="mcw-security-agent-") as temporary:
        temporary_root = Path(temporary)
        classes = temporary_root / "classes"
        classes.mkdir()
        run([javac, "--release", "8", "-encoding", "UTF-8", "-d", str(classes), *map(str, sources)])

        manifest = temporary_root / "MANIFEST.MF"
        manifest.write_text(
            "Manifest-Version: 1.0\n"
            f"Premain-Class: {MAIN_CLASS}\n"
            "Can-Redefine-Classes: false\n"
            "Can-Retransform-Classes: false\n\n",
            encoding="utf-8",
        )

        with zipfile.ZipFile(OUTPUT_JAR, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            write_deterministic(archive, "META-INF/MANIFEST.MF", manifest.read_bytes())
            for path in sorted(classes.rglob("*.class")):
                write_deterministic(archive, path.relative_to(classes).as_posix(), path.read_bytes())

    verify_agent(OUTPUT_JAR)
    digest = hashlib.sha256(OUTPUT_JAR.read_bytes()).hexdigest()
    print(f"Built {OUTPUT_JAR}")
    print(f"SHA-256: {digest}")
    return digest


def write_deterministic(archive: zipfile.ZipFile, name: str, payload: bytes) -> None:
    info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
    info.compress_type = zipfile.ZIP_DEFLATED
    info.external_attr = 0o644 << 16
    archive.writestr(info, payload, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)


def verify_agent(agent_jar: Path) -> None:
    java = shutil.which("java")
    javac = shutil.which("javac")
    if java is None or javac is None:
        raise RuntimeError("java and javac are required for the security agent verification test.")

    with tempfile.TemporaryDirectory(prefix="mcw-security-agent-test-") as temporary:
        root = Path(temporary)
        face_src = root / "org" / "lwjgl" / "util" / "freetype" / "FT_Face.java"
        face_src.parent.mkdir(parents=True)
        face_src.write_text(
            textwrap.dedent(
                """
                package org.lwjgl.util.freetype;

                public final class FT_Face {
                    public final long address;
                    public FT_Face(long address) {
                        this.address = address;
                    }
                }
                """
            ).strip()
            + "\n",
            encoding="utf-8",
        )

        ft_src = root / "org" / "lwjgl" / "util" / "freetype" / "FreeType.java"
        ft_src.write_text(
            textwrap.dedent(
                """
                package org.lwjgl.util.freetype;

                public final class FreeType {
                    public static boolean CHECKS = true;

                    public static int nFT_Load_Glyph(long face, int glyph_index, int load_flags) {
                        if (CHECKS && face == 0L) {
                            throw new IllegalArgumentException("null face");
                        }
                        try {
                            return load_flags | (glyph_index == -999 ? 0x100 : 0);
                        } catch (RuntimeException ex) {
                            return -1;
                        }
                    }

                    public static int FT_Load_Glyph(FT_Face face, int glyph_index, int load_flags) {
                        return nFT_Load_Glyph(face.address, glyph_index, load_flags);
                    }

                    public static int nFT_Load_Char(long face, long char_code, int load_flags) {
                        if (CHECKS && face == 0L) {
                            throw new IllegalArgumentException("null face");
                        }
                        return load_flags | (char_code == -999L ? 0x200 : 0);
                    }

                    public static int FT_Load_Char(FT_Face face, long char_code, int load_flags) {
                        return nFT_Load_Char(face.address, char_code, load_flags);
                    }
                }
                """
            ).strip()
            + "\n",
            encoding="utf-8",
        )

        main_src = root / "SecurityAgentSmokeTest.java"
        main_src.write_text(
            textwrap.dedent(
                """
                import org.lwjgl.util.freetype.FT_Face;
                import org.lwjgl.util.freetype.FreeType;

                public final class SecurityAgentSmokeTest {
                    public static void main(String[] args) {
                        FT_Face face = new FT_Face(1234L);
                        int a = FreeType.nFT_Load_Glyph(1234L, 1, 4);
                        int b = FreeType.FT_Load_Glyph(face, 1, 4);
                        int c = FreeType.nFT_Load_Char(1234L, 65L, 0);
                        int d = FreeType.FT_Load_Char(face, 65L, 8);
                        System.out.print(a + "," + b + "," + c + "," + d);
                    }
                }
                """
            ).strip()
            + "\n",
            encoding="utf-8",
        )

        run([javac, "-g", "--release", "8", "-d", str(root), str(face_src), str(ft_src), str(main_src)])

        normal = run([java, "-Xverify:all", "-cp", str(root), "SecurityAgentSmokeTest"])
        if normal.stdout.strip() != "4,4,0,8":
            raise RuntimeError(f"Unexpected unpatched output: {normal.stdout!r}")

        dormant = run(
            [
                java,
                "-Xverify:all",
                f"-javaagent:{agent_jar}",
                "-cp",
                str(root),
                "SecurityAgentSmokeTest",
            ]
        )
        if dormant.stdout.strip() != "4,4,0,8":
            raise RuntimeError(f"Agent should stay dormant unless property is true: {dormant.stdout!r}")

        patched = run(
            [
                java,
                "-Xverify:all",
                "-Dmcw.security.freetype_no_hinting=true",
                f"-javaagent:{agent_jar}",
                "-cp",
                str(root),
                "SecurityAgentSmokeTest",
            ]
        )
        if patched.stdout.strip() != "6,6,2,10":
            raise RuntimeError(
                f"Security agent failed smoke test: stdout={patched.stdout!r}, stderr={patched.stderr!r}"
            )
        if "Minecraft2Shell mitigation active: patched 4 method(s)" not in patched.stderr:
            raise RuntimeError(f"Unexpected agent stderr log: {patched.stderr!r}")


if __name__ == "__main__":
    build()
    sys.exit(0)
