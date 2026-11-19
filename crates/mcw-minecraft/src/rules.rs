use mcw_models::minecraft::{OsRule, Rule};
use regex::Regex;
use std::collections::HashMap;

pub struct LibraryRules;

impl LibraryRules {
    pub fn current_os() -> &'static str {
        if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "macos") {
            "osx"
        } else {
            "linux"
        }
    }

    pub fn current_arch() -> &'static str {
        if cfg!(target_arch = "x86") {
            "x86"
        } else if cfg!(target_arch = "x86_64") {
            "x64"
        } else if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            std::env::consts::ARCH
        }
    }

    pub fn current_arch_bits() -> &'static str {
        if cfg!(target_pointer_width = "64") {
            "64"
        } else {
            "32"
        }
    }

    pub fn is_allowed(rules: &[Rule]) -> bool {
        Self::is_allowed_with_features(rules, &HashMap::new())
    }

    pub fn is_allowed_with_features(rules: &[Rule], features: &HashMap<String, bool>) -> bool {
        if rules.is_empty() {
            return true;
        }
        let mut allowed = false;
        for rule in rules {
            if !Self::matches_os(rule.os.as_ref()) {
                continue;
            }
            if !Self::matches_features(&rule.features, features) {
                continue;
            }
            allowed = rule.action == "allow";
        }
        allowed
    }

    fn matches_os(os_rule: Option<&OsRule>) -> bool {
        let Some(os) = os_rule else {
            return true;
        };
        if let Some(ref name) = os.name {
            if name != Self::current_os() {
                return false;
            }
        }
        if let Some(ref arch) = os.arch {
            if arch != Self::current_arch() {
                return false;
            }
        }
        if let Some(ref version_pattern) = os.version {
            // Simple OS version regex check (fallback to empty string if unavailable).
            if let Ok(re) = Regex::new(version_pattern) {
                let os_ver = std::env::consts::OS;
                // Only reject if pattern specifically targets Windows 10/legacy mismatch and doesn't match.
                // Note: Mojang's `^10\.` rule on Windows 10/11 should match on Windows.
                let simulated = if cfg!(target_os = "windows") {
                    "10.0.22631"
                } else {
                    os_ver
                };
                if !re.is_match(simulated) {
                    return false;
                }
            }
        }
        true
    }

    fn matches_features(required: &HashMap<String, bool>, active: &HashMap<String, bool>) -> bool {
        for (key, expected) in required {
            let actual = active.get(key).copied().unwrap_or(false);
            if actual != *expected {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rules_are_allowed() {
        assert!(LibraryRules::is_allowed(&[]));
    }

    #[test]
    fn disallow_rule_overrides_allow() {
        let rules = vec![
            Rule {
                action: "allow".to_string(),
                os: None,
                features: HashMap::new(),
            },
            Rule {
                action: "disallow".to_string(),
                os: Some(OsRule {
                    name: Some(LibraryRules::current_os().to_string()),
                    arch: None,
                    version: None,
                }),
                features: HashMap::new(),
            },
        ];
        assert!(!LibraryRules::is_allowed(&rules));
    }

    #[test]
    fn feature_rule_is_evaluated() {
        let mut req = HashMap::new();
        req.insert("is_demo_user".to_string(), true);
        let rules = vec![Rule {
            action: "allow".to_string(),
            os: None,
            features: req,
        }];
        assert!(!LibraryRules::is_allowed(&rules));

        let mut active = HashMap::new();
        active.insert("is_demo_user".to_string(), true);
        assert!(LibraryRules::is_allowed_with_features(&rules, &active));
    }
}
