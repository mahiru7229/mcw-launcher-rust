use crate::instance::InstanceSettings;
use serde::{Deserialize, Serialize};

/// Port of MCW Launcher Python's `ManagedContentPolicy` (`src/core/config/managed_content_policy.py`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FailurePolicy {
    Inherit,
    Block,
    Warn,
    Allow,
}

impl FailurePolicy {
    /// Parse a global launcher setting policy (defaults to `Block`).
    /// Accepts `"warn"` or legacy Python `"ask"` as `Warn`.
    pub fn parse_global(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "warn" | "ask" => Self::Warn,
            "allow" => Self::Allow,
            "block" => Self::Block,
            _ => Self::Block,
        }
    }

    /// Parse an instance-level override policy (defaults to `Inherit`).
    pub fn parse_instance(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "block" => Self::Block,
            "warn" | "ask" => Self::Warn,
            "allow" => Self::Allow,
            "inherit" | "" => Self::Inherit,
            _ => Self::Inherit,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Inherit => "inherit",
            Self::Block => "block",
            Self::Warn => "warn",
            Self::Allow => "allow",
        }
    }

    pub fn is_blocking(&self) -> bool {
        *self == Self::Block
    }

    pub fn warns(&self) -> bool {
        *self == Self::Warn
    }
}

pub struct ManagedContentPolicy;

impl ManagedContentPolicy {
    /// Resolve an instance-level override against the global policy.
    pub fn resolve(instance_override: &str, global_policy: &str) -> FailurePolicy {
        let inst = FailurePolicy::parse_instance(instance_override);
        if inst != FailurePolicy::Inherit {
            inst
        } else {
            FailurePolicy::parse_global(global_policy)
        }
    }

    /// Resolve effective policy for `"modrinth"`, `"curseforge"`, or `"forge_preflight"`.
    pub fn resolve_for_provider(
        settings: &InstanceSettings,
        global_modrinth: &str,
        global_curseforge: &str,
        global_forge_preflight: &str,
        provider: &str,
    ) -> FailurePolicy {
        match provider.trim().to_ascii_lowercase().as_str() {
            "modrinth" => Self::resolve(&settings.modrinth_failure_policy, global_modrinth),
            "curseforge" => Self::resolve(&settings.curseforge_failure_policy, global_curseforge),
            "forge_preflight" | "forge" | "neoforge" => {
                // Global forge preflight defaults to Warn unless explicitly Block/Allow.
                let normalized = global_forge_preflight.trim().to_ascii_lowercase();
                if normalized.is_empty() {
                    FailurePolicy::Warn
                } else {
                    FailurePolicy::parse_global(&normalized)
                }
            }
            _ => FailurePolicy::Block,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_inherit_and_instance_overrides() {
        assert_eq!(
            ManagedContentPolicy::resolve("inherit", "warn"),
            FailurePolicy::Warn
        );
        assert_eq!(
            ManagedContentPolicy::resolve("inherit", "ask"),
            FailurePolicy::Warn
        );
        assert_eq!(
            ManagedContentPolicy::resolve("allow", "block"),
            FailurePolicy::Allow
        );
        assert_eq!(
            ManagedContentPolicy::resolve("block", "allow"),
            FailurePolicy::Block
        );
    }

    #[test]
    fn dispatches_by_provider_cleanly() {
        let mut settings = InstanceSettings::default();
        assert_eq!(
            ManagedContentPolicy::resolve_for_provider(
                &settings,
                "warn",
                "block",
                "warn",
                "curseforge"
            ),
            FailurePolicy::Block
        );
        settings.curseforge_failure_policy = "warn".to_string();
        assert_eq!(
            ManagedContentPolicy::resolve_for_provider(
                &settings,
                "block",
                "block",
                "warn",
                "curseforge"
            ),
            FailurePolicy::Warn
        );
        assert_eq!(
            ManagedContentPolicy::resolve_for_provider(
                &settings,
                "block",
                "block",
                "",
                "forge_preflight"
            ),
            FailurePolicy::Warn
        );
    }
}
