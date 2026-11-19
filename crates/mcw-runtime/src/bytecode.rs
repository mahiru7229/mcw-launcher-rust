use crate::java::JavaMajorPolicy;
use regex::Regex;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

const CLASS_MAGIC: u32 = 0xCAFE_BABE;
const MAX_CLASSES_PER_JAR: usize = 12;

/// Scan all `.jar` files in `mods_dir` and return the highest Java major version
/// required by their `fabric.mod.json` / `quilt.mod.json` or `.class` bytecode headers.
pub fn detect_mods_required_java(mods_dir: &Path) -> u32 {
    let Ok(entries) = fs::read_dir(mods_dir) else {
        return 8;
    };
    let max_supported = *JavaMajorPolicy::SUPPORTED_MAJORS.last().unwrap_or(&25);
    let mut detected = 8u32;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_jar = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("jar"))
            .unwrap_or(false);
        if !is_jar {
            continue;
        }
        let jar_major = detect_jar_required_java(&path);
        if jar_major > detected {
            detected = jar_major.min(max_supported);
        }
    }

    detected
}

/// Inspect a single mod `.jar` for Java version requirements:
/// 1. `fabric.mod.json` -> `depends.java`
/// 2. `quilt.mod.json` -> `quilt_loader.depends` (`id == "java"`)
/// 3. Up to 12 `.class` files' 8-byte headers (`0xCAFEBABE`, `major - 44`)
pub fn detect_jar_required_java(jar_path: &Path) -> u32 {
    let Ok(file) = File::open(jar_path) else {
        return 8;
    };
    let Ok(mut archive) = ZipArchive::new(file) else {
        return 8;
    };

    let mut detected = 8u32;

    if let Ok(mut fabric_entry) = archive.by_name("fabric.mod.json") {
        let mut raw = String::new();
        if fabric_entry.read_to_string(&mut raw).is_ok() {
            detected = detected.max(parse_fabric_java_depends(&raw));
        }
    }

    if let Ok(mut quilt_entry) = archive.by_name("quilt.mod.json") {
        let mut raw = String::new();
        if quilt_entry.read_to_string(&mut raw).is_ok() {
            detected = detected.max(parse_quilt_java_depends(&raw));
        }
    }

    let mut inspected = 0usize;
    for i in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(i) else {
            continue;
        };
        let name = entry.name();
        if !name.ends_with(".class") {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if lower == "module-info.class" || lower.starts_with("meta-inf/versions/") {
            continue;
        }

        let mut header = [0u8; 8];
        if entry.read_exact(&mut header).is_ok() {
            let magic = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
            let major = u16::from_be_bytes([header[6], header[7]]) as u32;
            if magic == CLASS_MAGIC && major >= 52 {
                detected = detected.max(major - 44);
            }
        }

        inspected += 1;
        if inspected >= MAX_CLASSES_PER_JAR {
            break;
        }
    }

    detected
}

fn parse_fabric_java_depends(raw: &str) -> u32 {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) else {
        return 8;
    };
    val.pointer("/depends/java")
        .map(extract_java_spec_major)
        .unwrap_or(8)
}

fn parse_quilt_java_depends(raw: &str) -> u32 {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) else {
        return 8;
    };
    let Some(depends) = val
        .pointer("/quilt_loader/depends")
        .and_then(|v| v.as_array())
    else {
        return 8;
    };
    let mut best = 8u32;
    for item in depends {
        if item.get("id").and_then(|v| v.as_str()) == Some("java") {
            if let Some(versions) = item.get("versions") {
                best = best.max(extract_java_spec_major(versions));
            }
        }
    }
    best
}

fn extract_java_spec_major(spec: &serde_json::Value) -> u32 {
    match spec {
        serde_json::Value::Number(n) => n.as_u64().map(|v| (v as u32).max(8)).unwrap_or(8),
        serde_json::Value::Array(arr) => arr.iter().map(extract_java_spec_major).min().unwrap_or(8),
        serde_json::Value::String(s) => {
            let re = Regex::new(r"(?:>=|>|=|~|\^)?\s*(?:1\.)?(\d+)").unwrap();
            re.captures_iter(s)
                .filter_map(|c| c.get(1)?.as_str().parse::<u32>().ok())
                .filter(|&v| v >= 8)
                .max()
                .unwrap_or(8)
        }
        _ => 8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    #[test]
    fn detects_java_17_from_class_bytecode_header() {
        let dir = tempdir().unwrap();
        let jar_path = dir.path().join("worldpreview.jar");
        let file = File::create(&jar_path).unwrap();
        let mut zip = ZipWriter::new(file);

        // Class file major 61 = Java 17 (61 - 44 = 17)
        let mut class_header = [0u8; 8];
        class_header[0..4].copy_from_slice(&0xCAFE_BABEu32.to_be_bytes());
        class_header[4..6].copy_from_slice(&0u16.to_be_bytes());
        class_header[6..8].copy_from_slice(&61u16.to_be_bytes());

        zip.start_file(
            "me/voidxwalker/worldpreview/Main.class",
            SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(&class_header).unwrap();
        zip.finish().unwrap();

        assert_eq!(detect_jar_required_java(&jar_path), 17);
        assert_eq!(detect_mods_required_java(dir.path()), 17);
    }

    #[test]
    fn detects_java_21_from_fabric_mod_json() {
        let dir = tempdir().unwrap();
        let jar_path = dir.path().join("modern-mod.jar");
        let file = File::create(&jar_path).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file("fabric.mod.json", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(br#"{"id":"test","depends":{"java":">=21"}}"#)
            .unwrap();
        zip.finish().unwrap();

        assert_eq!(detect_jar_required_java(&jar_path), 21);
    }
}
