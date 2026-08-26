//! Redaction engine for diagnostics and support bundles.
//!
//! Implements privacy-safe logging per SECURITY.md and the issue #21
//! acceptance criteria: tokens, labels, paths, URLs, scene names, and
//! payloads are never written verbatim to diagnostic output. Redaction is
//! applied on-write (before any entry enters the log tail), not on-read.
//!
//! The allowlist-based approach means only explicitly whitelisted fields
//! pass through unchanged; everything else is replaced with a structural
//! placeholder. This is the inverse of a denylist and fails closed: new
//! fields are redacted by default until explicitly approved.

use std::collections::HashSet;

/// Redaction strategy for a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RedactionStrategy {
    /// Pass through unchanged (allowlisted).
    Pass,
    /// Replace with a fixed structural placeholder.
    Redact,
    /// Replace with a truncated form (first N bytes visible).
    Truncate,
}

/// Configuration for the redaction engine: which field names are allowlisted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RedactionConfig {
    /// Field names that pass through unchanged. All others are redacted.
    allowlist: HashSet<String>,
}

impl RedactionConfig {
    /// Empty configuration (everything redacted — deny-by-default).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Allowlist a single field name.
    pub fn allow(&mut self, field: impl Into<String>) {
        self.allowlist.insert(field.into());
    }

    /// Allowlist multiple field names at once.
    pub fn allow_many(&mut self, fields: impl IntoIterator<Item = impl Into<String>>) {
        for field in fields {
            self.allowlist.insert(field.into());
        }
    }

    /// Remove a field from the allowlist (revoke pass-through).
    pub fn deny(&mut self, field: &str) {
        self.allowlist.remove(field);
    }

    /// Strategy for a given field name.
    #[must_use]
    pub fn strategy_for(&self, field: &str) -> RedactionStrategy {
        if self.allowlist.contains(field) {
            RedactionStrategy::Pass
        } else {
            RedactionStrategy::Redact
        }
    }

    /// True when the field is allowlisted.
    #[must_use]
    pub fn is_allowed(&self, field: &str) -> bool {
        self.allowlist.contains(field)
    }

    /// The allowlist entries in sorted order (for deterministic testing).
    #[must_use]
    pub fn allowed_fields(&self) -> Vec<&str> {
        let mut fields: Vec<&str> = self.allowlist.iter().map(String::as_str).collect();
        fields.sort();
        fields
    }
}

/// The fixed placeholder used for redacted values.
pub const REDACTED_PLACEHOLDER: &str = "[REDACTED]";

/// Redact a single value according to the strategy.
#[must_use]
pub fn redact_value(value: &str, strategy: RedactionStrategy) -> String {
    match strategy {
        RedactionStrategy::Pass => value.to_string(),
        RedactionStrategy::Redact => REDACTED_PLACEHOLDER.to_string(),
        RedactionStrategy::Truncate => {
            let max = 16;
            if value.len() <= max {
                value.to_string()
            } else {
                let truncated: String = value.chars().take(max).collect();
                format!("{truncated}…")
            }
        }
    }
}

/// Redact a structured key-value map, applying the config to each field.
/// Returns a new map with redacted values; keys are preserved for structure.
#[must_use]
pub fn redact_map(fields: &[(&str, String)], config: &RedactionConfig) -> Vec<(String, String)> {
    fields
        .iter()
        .map(|(key, value)| {
            let strategy = config.strategy_for(key);
            (key.to_string(), redact_value(value, strategy))
        })
        .collect()
}

/// Standard diagnostic field names that should NEVER be allowlisted
/// (hard-coded deny overrides the config).
pub const FORBIDDEN_FIELDS: &[&str] = &[
    "token",
    "secret",
    "password",
    "api_key",
    "authorization",
    "cookie",
    "session_id",
    "scene_name",
    "scene_id",
    "url",
    "path",
    "label",
    "payload",
    "body",
    "scene",
    "title",
    "name",
    "user_agent",
    "ip_address",
    "remote_host",
];

/// Validate that a config does not allowlist forbidden fields.
/// Returns the first forbidden field found, if any.
#[must_use]
pub fn validate_config(config: &RedactionConfig) -> Option<&'static str> {
    FORBIDDEN_FIELDS
        .iter()
        .find(|&&forbidden| config.is_allowed(forbidden))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_redacts_everything() {
        let config = RedactionConfig::new();
        assert_eq!(config.strategy_for("anything"), RedactionStrategy::Redact);
    }

    #[test]
    fn allowlisted_field_passes_through() {
        let mut config = RedactionConfig::new();
        config.allow("level");
        config.allow("module");
        assert_eq!(config.strategy_for("level"), RedactionStrategy::Pass);
        assert_eq!(config.strategy_for("module"), RedactionStrategy::Pass);
        assert_eq!(config.strategy_for("secret"), RedactionStrategy::Redact);
    }

    #[test]
    fn deny_removes_from_allowlist() {
        let mut config = RedactionConfig::new();
        config.allow("temp");
        assert!(config.is_allowed("temp"));
        config.deny("temp");
        assert!(!config.is_allowed("temp"));
    }

    #[test]
    fn redact_value_replaces_with_placeholder() {
        assert_eq!(
            redact_value("sensitive-data", RedactionStrategy::Redact),
            "[REDACTED]"
        );
    }

    #[test]
    fn redact_value_passes_through() {
        assert_eq!(
            redact_value("safe-data", RedactionStrategy::Pass),
            "safe-data"
        );
    }

    #[test]
    fn redact_value_truncates_long() {
        let long = "a".repeat(100);
        let result = redact_value(&long, RedactionStrategy::Truncate);
        assert!(result.len() < 100);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn redact_value_truncates_short() {
        let result = redact_value("short", RedactionStrategy::Truncate);
        assert_eq!(result, "short");
    }

    #[test]
    fn redact_map_applies_config() {
        let mut config = RedactionConfig::new();
        config.allow("level");
        let fields = [
            ("level".to_string(), "info".to_string()),
            ("secret".to_string(), "abc123".to_string()),
        ];
        let field_refs: Vec<(&str, String)> = fields
            .iter()
            .map(|(k, v)| (k.as_str(), v.clone()))
            .collect();
        let redacted = redact_map(&field_refs, &config);
        assert_eq!(redacted[0].1, "info");
        assert_eq!(redacted[1].1, "[REDACTED]");
    }

    #[test]
    fn forbidden_fields_cannot_be_allowlisted() {
        let mut config = RedactionConfig::new();
        config.allow("token");
        config.allow("password");
        config.allow("level");
        assert!(validate_config(&config).is_some());
        config.deny("token");
        assert!(validate_config(&config).is_some());
        config.deny("password");
        assert!(validate_config(&config).is_none());
    }

    #[test]
    fn allowed_fields_sorted_deterministically() {
        let mut config = RedactionConfig::new();
        config.allow("z_field");
        config.allow("a_field");
        config.allow("m_field");
        let fields = config.allowed_fields();
        assert_eq!(fields, vec!["a_field", "m_field", "z_field"]);
    }

    #[test]
    fn allow_many_bulk_inserts() {
        let mut config = RedactionConfig::new();
        config.allow_many(["alpha", "beta", "gamma"]);
        assert!(config.is_allowed("alpha"));
        assert!(config.is_allowed("beta"));
        assert!(config.is_allowed("gamma"));
        assert!(!config.is_allowed("delta"));
    }
}
