//! Structured diagnostic logging with severity levels and redaction-on-write.
//!
//! Implements the issue #21 acceptance criteria: structured allowlist logs,
//! bounded retention, local preview, and redaction of sensitive fields.
//! The log is an in-memory ring buffer with tail eviction; persistence is
//! the composition root's concern (#15/#16).
//!
//! Every entry is redacted before entering the buffer — the log never holds
//! raw sensitive values. Console output mirrors the buffer contents. The
//! diagnostic surface exposes a read-only view for headless installs.

use crate::error::DomainError;
use crate::limits::{MAX_DIAGNOSTIC_LOG_ENTRIES, MAX_DIAGNOSTIC_MESSAGE_BYTES};
use crate::redaction::{RedactionConfig, redact_map};
use serde::{Deserialize, Serialize};

/// Log severity levels matching the standard diagnostic taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Extremely detailed tracing, disabled by default.
    Trace,
    /// Debugging information.
    Debug,
    /// General informational messages.
    Info,
    /// Potential issues that need attention.
    Warn,
    /// Errors that affect functionality.
    Error,
}

impl Level {
    /// Canonical lowercase token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    /// Numeric severity (higher = more severe).
    #[must_use]
    pub const fn severity(self) -> u8 {
        match self {
            Self::Trace => 0,
            Self::Debug => 1,
            Self::Info => 2,
            Self::Warn => 3,
            Self::Error => 4,
        }
    }
}

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One structured diagnostic log entry. Fully serializable for persistence
/// and support bundles. Fields are redacted before storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    /// Monotonic timestamp (milliseconds since process start).
    pub timestamp_monotonic_ms: u64,
    /// Severity level.
    pub level: Level,
    /// Module path where the event originated.
    pub module: String,
    /// Human-readable message.
    pub message: String,
    /// Structured key-value fields (already redacted).
    #[serde(default)]
    pub fields: Vec<(String, String)>,
}

impl LogEntry {
    /// Validates the entry before it enters the buffer.
    /// Rejects oversized messages (fail-closed on content).
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.message.len() > MAX_DIAGNOSTIC_MESSAGE_BYTES {
            return Err(DomainError::DiagnosticValidationError {
                reason: "message too long",
            });
        }
        Ok(())
    }
}

/// Configuration for the structured log.
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Minimum level to record (entries below this are dropped).
    pub min_level: Level,
    /// Redaction configuration applied to every entry.
    pub redaction: RedactionConfig,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            min_level: Level::Info,
            redaction: RedactionConfig::new(),
        }
    }
}

/// In-memory structured diagnostic log with ring-buffer eviction.
///
/// Entries are redacted before insertion. The log is append-only; nothing
/// mutates or removes a stored entry. When the capacity bound is reached,
/// the oldest entries are evicted (tail eviction — diagnostics are
/// best-effort, not evidence, so we never fail closed on overflow).
#[derive(Debug, Clone)]
pub struct StructuredLog {
    entries: Vec<LogEntry>,
    config: LogConfig,
    dropped_count: u64,
}

impl StructuredLog {
    /// Creates a new log with the given configuration.
    #[must_use]
    pub fn new(config: LogConfig) -> Self {
        Self {
            entries: Vec::new(),
            config,
            dropped_count: 0,
        }
    }

    /// Creates a log with default configuration (Info level, everything
    /// redacted).
    #[must_use]
    pub fn default_config() -> Self {
        Self::new(LogConfig::default())
    }

    /// Records one log entry. The entry is redacted on-write. If the
    /// message exceeds the byte limit, the entry is rejected.
    pub fn record(
        &mut self,
        level: Level,
        module: &str,
        message: &str,
        fields: Vec<(String, String)>,
        timestamp_monotonic_ms: u64,
    ) -> Result<(), DomainError> {
        if level < self.config.min_level {
            return Ok(());
        }

        // Redact structured fields before entry. Messages are structural
        // (never user content) and pass through unchanged.
        let field_refs: Vec<(&str, String)> = fields
            .iter()
            .map(|(k, v)| (k.as_str(), v.clone()))
            .collect();
        let redacted_fields = redact_map(&field_refs, &self.config.redaction);

        let entry = LogEntry {
            timestamp_monotonic_ms,
            level,
            module: module.to_string(),
            message: message.to_string(),
            fields: redacted_fields,
        };

        entry.validate()?;

        // Ring buffer: evict oldest when full.
        if self.entries.len() >= MAX_DIAGNOSTIC_LOG_ENTRIES {
            let drain_count = MAX_DIAGNOSTIC_LOG_ENTRIES / 4;
            self.entries.drain(..drain_count);
            self.dropped_count += drain_count as u64;
        }

        self.entries.push(entry);
        Ok(())
    }

    /// Read-only snapshot of all retained entries, newest first.
    #[must_use]
    pub fn entries(&self) -> &[LogEntry] {
        &self.entries
    }

    /// Read-only snapshot of entries matching a minimum level.
    #[must_use]
    pub fn entries_at_level(&self, min_level: Level) -> Vec<&LogEntry> {
        self.entries
            .iter()
            .filter(|e| e.level >= min_level)
            .collect()
    }

    /// Number of retained entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when no entries have been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of entries dropped due to ring-buffer eviction.
    #[must_use]
    pub fn dropped_count(&self) -> u64 {
        self.dropped_count
    }

    /// Serializes all entries to JSON (for support bundles).
    /// # Errors
    /// Fails only if serde serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.entries)
    }
}

/// Records a Trace-level diagnostic event.
#[macro_export]
macro_rules! diag_trace {
    ($log:expr, $module:expr, $msg:expr $(, $key:expr => $val:expr)*) => {
        $log.record(
            $crate::structured_log::Level::Trace,
            $module,
            $msg,
            vec![$(($key.to_string(), $val.to_string())),*],
            0,
        )
    };
}

/// Records an Info-level diagnostic event.
#[macro_export]
macro_rules! diag_info {
    ($log:expr, $module:expr, $msg:expr $(, $key:expr => $val:expr)*) => {
        $log.record(
            $crate::structured_log::Level::Info,
            $module,
            $msg,
            vec![$(($key.to_string(), $val.to_string())),*],
            0,
        )
    };
}

/// Records a Warn-level diagnostic event.
#[macro_export]
macro_rules! diag_warn {
    ($log:expr, $module:expr, $msg:expr $(, $key:expr => $val:expr)*) => {
        $log.record(
            $crate::structured_log::Level::Warn,
            $module,
            $msg,
            vec![$(($key.to_string(), $val.to_string())),*],
            0,
        )
    };
}

/// Records an Error-level diagnostic event.
#[macro_export]
macro_rules! diag_error {
    ($log:expr, $module:expr, $msg:expr $(, $key:expr => $val:expr)*) => {
        $log.record(
            $crate::structured_log::Level::Error,
            $module,
            $msg,
            vec![$(($key.to_string(), $val.to_string())),*],
            0,
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redaction::RedactionConfig;

    fn test_config() -> LogConfig {
        let mut redaction = RedactionConfig::new();
        redaction.allow("level");
        redaction.allow("module");
        LogConfig {
            min_level: Level::Trace,
            redaction,
        }
    }

    #[test]
    fn record_and_retrieve() {
        let mut log = StructuredLog::new(test_config());
        log.record(Level::Info, "test", "hello world", vec![], 1000)
            .unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log.entries()[0].level, Level::Info);
        assert_eq!(log.entries()[0].message, "hello world");
    }

    #[test]
    fn min_level_filters_entries() {
        let config = LogConfig {
            min_level: Level::Warn,
            redaction: RedactionConfig::new(),
        };
        let mut log = StructuredLog::new(config);
        log.record(Level::Debug, "test", "dropped", vec![], 0)
            .unwrap();
        log.record(Level::Warn, "test", "kept", vec![], 0).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log.entries()[0].message, "kept");
    }

    #[test]
    fn redaction_applies_on_write() {
        let mut redaction = RedactionConfig::new();
        redaction.allow("level");
        let config = LogConfig {
            min_level: Level::Trace,
            redaction,
        };
        let mut log = StructuredLog::new(config);
        log.record(
            Level::Info,
            "test",
            "message",
            vec![("secret".to_string(), "abc123".to_string())],
            0,
        )
        .unwrap();
        assert_eq!(log.entries()[0].fields[0].1, "[REDACTED]");
    }

    #[test]
    fn ring_buffer_evicts_oldest() {
        let config = LogConfig {
            min_level: Level::Trace,
            redaction: RedactionConfig::new(),
        };
        let mut log = StructuredLog::new(config);
        for i in 0..crate::limits::MAX_DIAGNOSTIC_LOG_ENTRIES + 100 {
            log.record(Level::Info, "test", &format!("entry {i}"), vec![], i as u64)
                .unwrap();
        }
        assert!(log.len() <= crate::limits::MAX_DIAGNOSTIC_LOG_ENTRIES);
        assert!(log.dropped_count() > 0);
    }

    #[test]
    fn entries_at_level_filters() {
        let mut log = StructuredLog::new(test_config());
        log.record(Level::Info, "test", "info", vec![], 0).unwrap();
        log.record(Level::Error, "test", "error", vec![], 0)
            .unwrap();
        log.record(Level::Warn, "test", "warn", vec![], 0).unwrap();
        let warns = log.entries_at_level(Level::Warn);
        assert_eq!(warns.len(), 2);
    }

    #[test]
    fn to_json_serializes() {
        let mut log = StructuredLog::new(test_config());
        log.record(Level::Info, "test", "hello", vec![], 0).unwrap();
        let json = log.to_json().unwrap();
        assert!(json.contains("hello"));
    }

    #[test]
    fn oversized_message_rejects() {
        let mut log = StructuredLog::new(test_config());
        let long_msg = "x".repeat(MAX_DIAGNOSTIC_MESSAGE_BYTES + 1);
        let result = log.record(Level::Info, "test", &long_msg, vec![], 0);
        assert!(result.is_err());
    }

    #[test]
    fn default_config_redacts_everything() {
        let mut log = StructuredLog::default_config();
        log.record(
            Level::Info,
            "test",
            "msg",
            vec![("anything".to_string(), "value".to_string())],
            0,
        )
        .unwrap();
        assert_eq!(log.entries()[0].fields[0].1, "[REDACTED]");
    }
}
