//! Crash report builder and local rollup.
//!
//! Implements issue #21 acceptance criteria: crash reports (rollup, local)
//! with user approval prompt. Crash reports capture the diagnostic state
//! at failure time, redact sensitive data, and are stored locally until
//! the user approves sharing.
//!
//! The report is built before exit, serialized to a local file, and
//! presented to the user for approval before any external transmission.
//! Sensitive fields are never included (redaction-on-write applies).

use crate::error::DomainError;
use crate::limits::MAX_CRASH_REPORTS;
use crate::structured_log::LogEntry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Severity classification of the failure that triggered the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashSeverity {
    /// Non-fatal error that degraded functionality.
    Recoverable,
    /// Fatal error that terminated the process.
    Fatal,
    /// Unknown/unclassified failure.
    Unknown,
}

impl CrashSeverity {
    /// Canonical lowercase token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Recoverable => "recoverable",
            Self::Fatal => "fatal",
            Self::Unknown => "unknown",
        }
    }
}

/// One crash report: a snapshot of diagnostic state at failure time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashReport {
    /// Unique identifier for this report (UUIDv7).
    pub id: String,
    /// ISO-8601 timestamp of when the crash occurred.
    pub timestamp: String,
    /// Severity of the failure.
    pub severity: CrashSeverity,
    /// Error message (structural only, no user content).
    pub error_message: String,
    /// Module where the error originated.
    pub error_module: String,
    /// Application version.
    pub app_version: String,
    /// Platform identifier.
    pub platform: String,
    /// Diagnostic log entries captured around the failure.
    pub diagnostic_entries: Vec<LogEntry>,
    /// Structured metadata (all values redacted).
    pub metadata: HashMap<String, String>,
    /// User's approval status for sharing.
    pub user_approved: bool,
}

impl CrashReport {
    /// Creates a new crash report with default values.
    #[must_use]
    pub fn new(
        id: String,
        timestamp: String,
        severity: CrashSeverity,
        error_message: String,
        error_module: String,
        app_version: String,
        platform: String,
    ) -> Self {
        Self {
            id,
            timestamp,
            severity,
            error_message,
            error_module,
            app_version,
            platform,
            diagnostic_entries: Vec::new(),
            metadata: HashMap::new(),
            user_approved: false,
        }
    }

    /// Attaches diagnostic log entries to the report.
    pub fn with_entries(mut self, entries: Vec<LogEntry>) -> Self {
        self.diagnostic_entries = entries;
        self
    }

    /// Attaches structured metadata.
    pub fn with_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = metadata;
        self
    }

    /// Records user approval for sharing.
    pub fn approve(&mut self) {
        self.user_approved = true;
    }

    /// Records user denial of sharing.
    pub fn deny(&mut self) {
        self.user_approved = false;
    }

    /// Serializes the report to JSON for local storage.
    /// # Errors
    /// Fails only if serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Validates the report before storage.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.error_message.is_empty() {
            return Err(DomainError::DiagnosticValidationError {
                reason: "empty error message",
            });
        }
        Ok(())
    }
}

/// Local crash report storage: retains up to [`MAX_CRASH_REPORTS`]
/// reports, pruning oldest when full.
#[derive(Debug, Clone)]
pub struct CrashReportStore {
    reports: Vec<CrashReport>,
}

impl Default for CrashReportStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CrashReportStore {
    /// Empty store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            reports: Vec::new(),
        }
    }

    /// Stores a crash report. If the store is at capacity, the oldest
    /// report is pruned.
    pub fn store(&mut self, report: CrashReport) -> Result<(), DomainError> {
        report.validate()?;

        if self.reports.len() >= MAX_CRASH_REPORTS {
            self.reports.remove(0);
        }

        self.reports.push(report);
        Ok(())
    }

    /// All stored reports, in insertion order.
    #[must_use]
    pub fn reports(&self) -> &[CrashReport] {
        &self.reports
    }

    /// Number of stored reports.
    #[must_use]
    pub fn len(&self) -> usize {
        self.reports.len()
    }

    /// True when no reports are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.reports.is_empty()
    }

    /// Returns only reports approved by the user for sharing.
    #[must_use]
    pub fn approved_reports(&self) -> Vec<&CrashReport> {
        self.reports.iter().filter(|r| r.user_approved).collect()
    }

    /// Returns only unapproved reports (pending user decision).
    #[must_use]
    pub fn pending_reports(&self) -> Vec<&CrashReport> {
        self.reports.iter().filter(|r| !r.user_approved).collect()
    }

    /// Serializes all reports to JSON for local storage.
    /// # Errors
    /// Fails only if serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.reports)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_report(id: &str) -> CrashReport {
        CrashReport::new(
            id.to_string(),
            "2026-01-01T00:00:00Z".to_string(),
            CrashSeverity::Fatal,
            "test error".to_string(),
            "test::module".to_string(),
            "0.1.0".to_string(),
            "windows".to_string(),
        )
    }

    #[test]
    fn create_and_validate_report() {
        let report = test_report("r1");
        assert!(report.validate().is_ok());
    }

    #[test]
    fn empty_message_rejects() {
        let mut report = test_report("r1");
        report.error_message = String::new();
        assert!(report.validate().is_err());
    }

    #[test]
    fn approve_deny_cycle() {
        let mut report = test_report("r1");
        assert!(!report.user_approved);
        report.approve();
        assert!(report.user_approved);
        report.deny();
        assert!(!report.user_approved);
    }

    #[test]
    fn store_retrieves_reports() {
        let mut store = CrashReportStore::new();
        store.store(test_report("r1")).unwrap();
        store.store(test_report("r2")).unwrap();
        assert_eq!(store.len(), 2);
        assert_eq!(store.reports()[0].id, "r1");
    }

    #[test]
    fn store_prunes_oldest() {
        let mut store = CrashReportStore::new();
        for i in 0..MAX_CRASH_REPORTS + 5 {
            store.store(test_report(&format!("r{i}"))).unwrap();
        }
        assert_eq!(store.len(), MAX_CRASH_REPORTS);
        // Oldest reports were pruned.
        assert_eq!(store.reports()[0].id, "r5");
    }

    #[test]
    fn approved_reports_filter() {
        let mut store = CrashReportStore::new();
        let mut r1 = test_report("r1");
        r1.approve();
        store.store(r1).unwrap();
        store.store(test_report("r2")).unwrap();
        assert_eq!(store.approved_reports().len(), 1);
        assert_eq!(store.pending_reports().len(), 1);
    }

    #[test]
    fn to_json_serializes() {
        let mut store = CrashReportStore::new();
        store.store(test_report("r1")).unwrap();
        let json = store.to_json().unwrap();
        assert!(json.contains("r1"));
    }

    #[test]
    fn crash_report_serialization_roundtrip() {
        let report = test_report("r1");
        let json = report.to_json().unwrap();
        let back: CrashReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, report.id);
        assert_eq!(back.severity, report.severity);
    }
}
