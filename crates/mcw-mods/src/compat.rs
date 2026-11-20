use crate::{is_loader_compatible, ModInfo};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModIssueSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone)]
pub struct ModIssue {
    pub severity: ModIssueSeverity,
    pub code: String,
    pub message: String,
    pub mod_ids: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ModCompatibilityReport {
    pub issues: Vec<ModIssue>,
    pub error_count: usize,
    pub warning_count: usize,
    pub enabled_mods: usize,
    pub disabled_mods: usize,
}

const SYSTEM_DEPENDENCY_IDS: &[&str] = &[
    "minecraft",
    "java",
    "forge",
    "neoforge",
    "javafml",
    "fml",
    "fabric",
    "fabricloader",
    "fabric-loader",
    "quilt",
    "quilt_loader",
    "quiltloader",
    "mixinextras",
    "fabric-api-base",
];

pub struct ModCompatibilityAnalyzer;

impl ModCompatibilityAnalyzer {
    pub fn analyze(instance_loader: &str, mods: &[ModInfo]) -> ModCompatibilityReport {
        let enabled: Vec<&ModInfo> = mods.iter().filter(|m| m.enabled).collect();
        let disabled: Vec<&ModInfo> = mods.iter().filter(|m| !m.enabled).collect();

        let mut enabled_by_id: HashMap<String, Vec<&ModInfo>> = HashMap::new();
        let mut enabled_capability_ids: HashSet<String> = HashSet::new();
        let mut disabled_ids: HashSet<String> = HashSet::new();

        for &m in &enabled {
            let pid = m.mod_id.trim().to_ascii_lowercase();
            if !pid.is_empty() && pid != "unknown" {
                enabled_by_id.entry(pid.clone()).or_default().push(m);
                enabled_capability_ids.insert(pid);
            }
            for emb in &m.embedded_mod_ids {
                let eid = emb.trim().to_ascii_lowercase();
                if !eid.is_empty() {
                    enabled_capability_ids.insert(eid);
                }
            }
        }
        for &m in &disabled {
            let pid = m.mod_id.trim().to_ascii_lowercase();
            if !pid.is_empty() && pid != "unknown" {
                disabled_ids.insert(pid);
            }
        }

        let mut issues = Vec::new();
        let has_fabric_api = enabled_capability_ids.contains("fabric-api")
            || enabled_capability_ids.contains("quilted_fabric_api");

        // 1. Loader mismatches
        for &m in &enabled {
            if !is_loader_compatible(m.loader, instance_loader) {
                issues.push(ModIssue {
                    severity: ModIssueSeverity::Error,
                    code: "loader-mismatch".to_string(),
                    message: format!(
                        "{} targets {:?}, which is incompatible with loader '{}'.",
                        m.name, m.loader, instance_loader
                    ),
                    mod_ids: vec![m.mod_id.clone()],
                });
            }
        }

        // 2. Duplicate enabled mod IDs
        for (mod_id, list) in &enabled_by_id {
            if list.len() > 1 {
                let files: Vec<&str> = list.iter().map(|m| m.file_name.as_str()).collect();
                issues.push(ModIssue {
                    severity: ModIssueSeverity::Error,
                    code: "duplicate-mod-id".to_string(),
                    message: format!("Duplicate enabled mod ID '{mod_id}': {}", files.join(", ")),
                    mod_ids: vec![mod_id.clone()],
                });
            }
        }

        // 3. Required dependencies
        for &m in &enabled {
            for dep in &m.depends {
                let dep_lower = dep.trim().to_ascii_lowercase();
                if dep_lower.is_empty()
                    || SYSTEM_DEPENDENCY_IDS.contains(&dep_lower.as_str())
                    || enabled_capability_ids.contains(&dep_lower)
                {
                    continue;
                }
                if has_fabric_api && dep_lower.starts_with("fabric-") {
                    continue;
                }
                if disabled_ids.contains(&dep_lower) {
                    issues.push(ModIssue {
                        severity: ModIssueSeverity::Warning,
                        code: "disabled-dependency".to_string(),
                        message: format!(
                            "{} requires '{}', which is installed but currently disabled.",
                            m.name, dep
                        ),
                        mod_ids: vec![m.mod_id.clone(), dep.clone()],
                    });
                } else {
                    issues.push(ModIssue {
                        severity: ModIssueSeverity::Warning,
                        code: "missing-dependency".to_string(),
                        message: format!("{} requires '{}', which was not found.", m.name, dep),
                        mod_ids: vec![m.mod_id.clone(), dep.clone()],
                    });
                }
            }
        }

        issues.sort_by_key(|i| match i.severity {
            ModIssueSeverity::Error => 0u8,
            ModIssueSeverity::Warning => 1u8,
            ModIssueSeverity::Info => 2u8,
        });

        let error_count = issues
            .iter()
            .filter(|i| i.severity == ModIssueSeverity::Error)
            .count();
        let warning_count = issues
            .iter()
            .filter(|i| i.severity == ModIssueSeverity::Warning)
            .count();

        ModCompatibilityReport {
            issues,
            error_count,
            warning_count,
            enabled_mods: enabled.len(),
            disabled_mods: disabled.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModLoaderTag;
    use std::path::PathBuf;

    fn sample_mod(id: &str, loader: ModLoaderTag, enabled: bool) -> ModInfo {
        ModInfo {
            file_name: format!("{id}.jar"),
            path: PathBuf::from(format!("{id}.jar")),
            enabled,
            size_bytes: 100,
            mod_id: id.to_string(),
            name: id.to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            authors: Vec::new(),
            loader,
            required_java_major: 17,
            embedded_mod_ids: Vec::new(),
            depends: Vec::new(),
        }
    }

    #[test]
    fn detects_loader_mismatch_duplicates_and_dependencies() {
        let mut sodium = sample_mod("sodium", ModLoaderTag::Fabric, true);
        sodium.depends = vec![
            "fabricloader".to_string(),
            "minecraft".to_string(),
            "cloth-config".to_string(),
            "indium".to_string(),
        ];
        sodium.embedded_mod_ids = vec!["cloth-config".to_string()];

        let indium_disabled = sample_mod("indium", ModLoaderTag::Fabric, false);
        let jei_forge = sample_mod("jei", ModLoaderTag::Forge, true);

        let report =
            ModCompatibilityAnalyzer::analyze("quilt", &[sodium, indium_disabled, jei_forge]);
        // sodium on quilt is valid; cloth-config is embedded; indium is disabled; jei is Forge mismatch
        assert_eq!(report.error_count, 1);
        assert_eq!(report.warning_count, 1);
        assert!(report.issues.iter().any(|i| i.code == "loader-mismatch"));
        assert!(report
            .issues
            .iter()
            .any(|i| i.code == "disabled-dependency"));
    }
}
