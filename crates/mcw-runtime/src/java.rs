use crate::{Result, RuntimeError};
use regex::Regex;
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct JavaMajorPolicy;

impl JavaMajorPolicy {
    pub const SUPPORTED_MAJORS: [u32; 4] = [8, 17, 21, 25];

    pub fn required_for_minecraft(game_version: &str, metadata_major: Option<u32>) -> Result<u32> {
        if let Some(major) = metadata_major {
            if major < 1 {
                return Err(RuntimeError::InvalidJavaMajor(major.to_string()));
            }
            return Ok(major);
        }

        let trimmed = game_version.trim();
        let re = Regex::new(r"^1\.(\d+)(?:\.(\d+))?$").unwrap();
        let Some(caps) = re.captures(trimmed) else {
            return Ok(8);
        };
        let minor: u32 = caps.get(1).unwrap().as_str().parse().unwrap_or(0);
        let patch: u32 = caps
            .get(2)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);

        if minor <= 16 {
            Ok(8)
        } else if minor == 17 {
            Ok(16)
        } else if minor < 20 || (minor == 20 && patch <= 4) {
            Ok(17)
        } else {
            Ok(21)
        }
    }

    pub fn resolve(required_major: u32) -> Result<u32> {
        for managed in Self::SUPPORTED_MAJORS {
            if required_major <= managed {
                return Ok(managed);
            }
        }
        Err(RuntimeError::UnsupportedJavaMajor(required_major))
    }

    pub fn accepted_majors(required_major: u32, allow_higher: bool) -> Result<Vec<u32>> {
        let managed = Self::resolve(required_major)?;
        let mut majors = vec![required_major];
        if managed != required_major {
            majors.push(managed);
        }
        if allow_higher {
            for m in Self::SUPPORTED_MAJORS {
                if m >= required_major && !majors.contains(&m) {
                    majors.push(m);
                }
            }
        }
        Ok(majors)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaInstallation {
    pub path: PathBuf,
    pub major: u32,
    pub version_string: String,
}

pub struct JavaDiscovery;

impl JavaDiscovery {
    pub fn discover(runtimes_root: Option<&Path>) -> Vec<JavaInstallation> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        let exe_name = if cfg!(windows) { "java.exe" } else { "java" };

        // 1. Managed runtimes folder
        if let Some(root) = runtimes_root {
            Self::scan_jdk_parent(root, exe_name, &mut candidates);
        }

        // 2. JAVA_HOME
        if let Ok(java_home) = env::var("JAVA_HOME") {
            let p = PathBuf::from(java_home).join("bin").join(exe_name);
            candidates.push(p);
        }

        // 3. PATH
        if let Ok(path_var) = env::var("PATH") {
            for dir in env::split_paths(&path_var) {
                let p = dir.join(exe_name);
                if p.is_file() {
                    candidates.push(p);
                }
            }
        }

        // 4. Standard OS install locations
        #[cfg(windows)]
        {
            for root in [
                r"C:\Program Files\Eclipse Adoptium",
                r"C:\Program Files\Java",
                r"C:\Program Files\Microsoft",
                r"C:\Program Files\Zulu",
                r"C:\Program Files\BellSoft",
                r"C:\Program Files\Amazon Corretto",
            ] {
                Self::scan_jdk_parent(Path::new(root), exe_name, &mut candidates);
            }
        }
        #[cfg(not(windows))]
        {
            for root in ["/usr/lib/jvm", "/usr/java", "/opt/java"] {
                Self::scan_jdk_parent(Path::new(root), exe_name, &mut candidates);
            }
        }

        let mut seen = HashSet::new();
        let mut found = Vec::new();

        for cand in candidates {
            if !cand.is_file() {
                continue;
            }
            let canonical = strip_verbatim_prefix(std::fs::canonicalize(&cand).unwrap_or(cand));
            if !seen.insert(canonical.clone()) {
                continue;
            }
            if let Some(info) = Self::probe(&canonical) {
                found.push(info);
            }
        }

        found.sort_by_key(|j| (j.major, j.path.clone()));
        found
    }

    fn scan_jdk_parent(parent: &Path, exe_name: &str, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(parent) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let direct = path.join("bin").join(exe_name);
                if direct.is_file() {
                    out.push(direct);
                }
            }
        }
    }

    pub fn probe(java_exe: &Path) -> Option<JavaInstallation> {
        let output = Command::new(java_exe).arg("-version").output().ok()?;
        let combined = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let (major, version_string) = parse_java_version_output(&combined)?;
        Some(JavaInstallation {
            path: java_exe.to_path_buf(),
            major,
            version_string,
        })
    }
}

pub fn parse_java_version_output(output: &str) -> Option<(u32, String)> {
    let re = Regex::new(r#"version\s+"([^"]+)""#).ok()?;
    let version_str = re.captures(output)?.get(1)?.as_str().to_string();
    let major = parse_major_from_version_string(&version_str)?;
    Some((major, version_str))
}

fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(s) = path.to_str() {
            if let Some(stripped) = s.strip_prefix(r"\\?\") {
                return PathBuf::from(stripped);
            }
        }
    }
    path
}

fn parse_major_from_version_string(version: &str) -> Option<u32> {
    let mut parts = version.split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

pub struct JavaResolver;

impl JavaResolver {
    pub fn resolve(
        required_major: u32,
        preferred_path: Option<&Path>,
        runtimes_root: Option<&Path>,
        allow_higher: bool,
    ) -> Result<JavaInstallation> {
        let managed = JavaMajorPolicy::resolve(required_major)?;

        if let Some(preferred) = preferred_path {
            if !preferred.is_file() {
                return Err(RuntimeError::InvalidJavaPath {
                    path: preferred.display().to_string(),
                    reason: "file does not exist".to_string(),
                });
            }
            let info =
                JavaDiscovery::probe(preferred).ok_or_else(|| RuntimeError::InvalidJavaPath {
                    path: preferred.display().to_string(),
                    reason: "could not determine Java version".to_string(),
                })?;
            if info.major < required_major {
                return Err(RuntimeError::InvalidJavaPath {
                    path: preferred.display().to_string(),
                    reason: format!(
                        "Java {} is too old (requires Java {}+)",
                        info.major, required_major
                    ),
                });
            }
            return Ok(info);
        }

        let discovered = JavaDiscovery::discover(runtimes_root);
        let accepted = JavaMajorPolicy::accepted_majors(required_major, allow_higher)?;

        // 1. Prefer exact match in accepted order (required, managed, then higher).
        for target_major in &accepted {
            if let Some(found) = discovered.iter().find(|j| j.major == *target_major) {
                return Ok(found.clone());
            }
        }

        // 2. If allow_higher, also accept any installed Java >= required_major (e.g. Java 22/23/24/25).
        if allow_higher {
            if let Some(found) = discovered.iter().find(|j| j.major >= required_major) {
                return Ok(found.clone());
            }
        }

        Err(RuntimeError::NoCompatibleJava {
            required: required_major,
            managed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_and_modern_java_version_strings() {
        assert_eq!(
            parse_java_version_output(r#"java version "1.8.0_412""#),
            Some((8, "1.8.0_412".to_string()))
        );
        assert_eq!(
            parse_java_version_output(r#"openjdk version "17.0.12" 2024-07-16"#),
            Some((17, "17.0.12".to_string()))
        );
        assert_eq!(
            parse_java_version_output(r#"openjdk version "21.0.5" 2024-10-15 LTS"#),
            Some((21, "21.0.5".to_string()))
        );
        assert_eq!(
            parse_java_version_output(r#"openjdk version "25.0.4" 2026-07-21 LTS"#),
            Some((25, "25.0.4".to_string()))
        );
    }

    #[test]
    fn java_major_policy_matches_minecraft_versions() {
        assert_eq!(
            JavaMajorPolicy::required_for_minecraft("1.16.5", None).unwrap(),
            8
        );
        assert_eq!(
            JavaMajorPolicy::required_for_minecraft("1.17.1", None).unwrap(),
            16
        );
        assert_eq!(
            JavaMajorPolicy::required_for_minecraft("1.20.4", None).unwrap(),
            17
        );
        assert_eq!(
            JavaMajorPolicy::required_for_minecraft("1.21.4", None).unwrap(),
            21
        );
        assert_eq!(
            JavaMajorPolicy::required_for_minecraft("1.21.4", Some(21)).unwrap(),
            21
        );
    }
}
