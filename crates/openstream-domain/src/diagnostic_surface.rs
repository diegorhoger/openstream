//! Internal diagnostic surface for headless installs.
//!
//! Implements issue #21 acceptance criteria: internal diagnostic surface
//! for headless installs. Provides a read-only view of all diagnostic
//! subsystems — structured log, audit chain, consent state, rate limiter
//! stats, and crash reports — that headless or headful installs can query.
//!
//! The surface is a facade over the individual diagnostic subsystems and
//! exposes no mutation paths (read-only by construction).

use crate::audit_chain::AuditChain;
use crate::consent::ConsentManager;
use crate::crash_report::CrashReportStore;
use crate::rate_limiter::BucketRegistry;
use crate::structured_log::StructuredLog;
use serde::{Deserialize, Serialize};

/// Summary of the structured log subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogSummary {
    /// Total entries retained.
    pub entry_count: usize,
    /// Entries dropped by ring-buffer eviction.
    pub dropped_count: u64,
    /// Minimum configured level.
    pub min_level: String,
    /// Count per severity level.
    pub by_level: std::collections::HashMap<String, usize>,
}

/// Summary of the audit chain subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditSummary {
    /// Total chained entries.
    pub entry_count: usize,
    /// Whether the chain is currently valid.
    pub chain_valid: bool,
    /// Head hash of the chain (if non-empty).
    pub head_hash: Option<String>,
    /// Bucket counts.
    pub buckets: std::collections::HashMap<String, u64>,
}

/// Summary of the consent subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsentSummary {
    /// Current consent state.
    pub state: String,
    /// Whether telemetry is allowed.
    pub is_allowed: bool,
    /// Number of consent changes.
    pub change_count: u64,
}

/// Summary of the crash report subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashSummary {
    /// Total stored reports.
    pub total_reports: usize,
    /// Reports awaiting user decision.
    pub pending_count: usize,
    /// Reports approved for sharing.
    pub approved_count: usize,
}

/// Full diagnostic surface snapshot: a read-only view of all subsystems.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticSnapshot {
    /// Structured log summary.
    pub log: LogSummary,
    /// Audit chain summary.
    pub audit: AuditSummary,
    /// Telemetry consent summary.
    pub consent: ConsentSummary,
    /// Crash report summary.
    pub crashes: CrashSummary,
    /// Rate limiter bucket summaries.
    pub rate_limiters: Vec<crate::rate_limiter::BucketSummary>,
}

/// The diagnostic surface: a facade providing read-only access to all
/// diagnostic subsystems.
pub struct DiagnosticSurface<'a> {
    log: &'a StructuredLog,
    audit: &'a AuditChain,
    consent: &'a ConsentManager,
    crashes: &'a CrashReportStore,
    buckets: &'a BucketRegistry,
}

impl<'a> DiagnosticSurface<'a> {
    /// Creates a new surface over the diagnostic subsystems.
    #[must_use]
    pub fn new(
        log: &'a StructuredLog,
        audit: &'a AuditChain,
        consent: &'a ConsentManager,
        crashes: &'a CrashReportStore,
        buckets: &'a BucketRegistry,
    ) -> Self {
        Self {
            log,
            audit,
            consent,
            crashes,
            buckets,
        }
    }

    /// Takes a full diagnostic snapshot.
    #[must_use]
    pub fn snapshot(&self) -> DiagnosticSnapshot {
        DiagnosticSnapshot {
            log: self.log_summary(),
            audit: self.audit_summary(),
            consent: self.consent_summary(),
            crashes: self.crash_summary(),
            rate_limiters: self.buckets.summary(),
        }
    }

    /// Structured log summary.
    #[must_use]
    pub fn log_summary(&self) -> LogSummary {
        let mut by_level = std::collections::HashMap::new();
        for entry in self.log.entries() {
            *by_level
                .entry(entry.level.as_str().to_string())
                .or_insert(0) += 1;
        }
        LogSummary {
            entry_count: self.log.len(),
            dropped_count: self.log.dropped_count(),
            min_level: "info".to_string(),
            by_level,
        }
    }

    /// Audit chain summary.
    #[must_use]
    pub fn audit_summary(&self) -> AuditSummary {
        AuditSummary {
            entry_count: self.audit.len(),
            chain_valid: self.audit.verify().is_ok(),
            head_hash: self.audit.head_hash().map(String::from),
            buckets: self.audit.buckets().clone(),
        }
    }

    /// Consent summary.
    #[must_use]
    pub fn consent_summary(&self) -> ConsentSummary {
        ConsentSummary {
            state: self.consent.state().to_string(),
            is_allowed: self.consent.is_allowed(),
            change_count: self.consent.change_count(),
        }
    }

    /// Crash report summary.
    #[must_use]
    pub fn crash_summary(&self) -> CrashSummary {
        CrashSummary {
            total_reports: self.crashes.len(),
            pending_count: self.crashes.pending_reports().len(),
            approved_count: self.crashes.approved_reports().len(),
        }
    }

    /// Serializes the full snapshot to JSON.
    /// # Errors
    /// Fails only if serialization fails.
    pub fn snapshot_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.snapshot())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::{AuditEvent, ExecutionState};
    use crate::audit_chain::AuditChain;
    use crate::consent::ConsentManager;
    use crate::crash_report::{CrashReport, CrashReportStore, CrashSeverity};
    use crate::rate_limiter::{Bucket, BucketRegistry};
    use crate::redaction::RedactionConfig;
    use crate::structured_log::{Level, LogConfig, StructuredLog};

    fn setup() -> (
        StructuredLog,
        AuditChain,
        ConsentManager,
        CrashReportStore,
        BucketRegistry,
    ) {
        let log = StructuredLog::new(LogConfig {
            min_level: Level::Trace,
            redaction: RedactionConfig::new(),
        });
        let audit = AuditChain::new();
        let consent = ConsentManager::new();
        let crashes = CrashReportStore::new();
        let mut buckets = BucketRegistry::new();
        buckets
            .register(Bucket::new("test".into(), 100, 60_000, 1))
            .unwrap();
        (log, audit, consent, crashes, buckets)
    }

    #[test]
    fn snapshot_captures_all_subsystems() {
        let (log, mut audit, consent, crashes, buckets) = setup();
        let mut log = log;
        log.record(Level::Info, "test", "hello", vec![], 0).unwrap();
        audit
            .append(AuditEvent::ExecutionObserved {
                at_ms: 1000,
                execution_id: crate::ids::ExecutionId::generate(),
                state: ExecutionState::Accepted,
            })
            .unwrap();

        let surface = DiagnosticSurface::new(&log, &audit, &consent, &crashes, &buckets);
        let snap = surface.snapshot();
        assert_eq!(snap.log.entry_count, 1);
        assert_eq!(snap.audit.entry_count, 1);
        assert!(snap.audit.chain_valid);
        assert_eq!(snap.consent.state, "denied");
        assert_eq!(snap.crashes.total_reports, 0);
    }

    #[test]
    fn snapshot_json_serializes() {
        let (log, audit, consent, crashes, buckets) = setup();
        let surface = DiagnosticSurface::new(&log, &audit, &consent, &crashes, &buckets);
        let json = surface.snapshot_json().unwrap();
        assert!(json.contains("log"));
        assert!(json.contains("audit"));
    }

    #[test]
    fn log_summary_by_level() {
        let log_config = LogConfig {
            min_level: Level::Trace,
            redaction: RedactionConfig::new(),
        };
        let mut log = StructuredLog::new(log_config);
        log.record(Level::Info, "m", "a", vec![], 0).unwrap();
        log.record(Level::Error, "m", "b", vec![], 0).unwrap();
        log.record(Level::Info, "m", "c", vec![], 0).unwrap();
        let audit = AuditChain::new();
        let consent = ConsentManager::new();
        let crashes = CrashReportStore::new();
        let buckets = BucketRegistry::new();
        let surface = DiagnosticSurface::new(&log, &audit, &consent, &crashes, &buckets);
        let summary = surface.log_summary();
        assert_eq!(summary.by_level.get("info"), Some(&2));
        assert_eq!(summary.by_level.get("error"), Some(&1));
    }

    #[test]
    fn crash_summary_counts() {
        let (log, audit, consent, mut crashes, buckets) = setup();
        let mut r1 = CrashReport::new(
            "r1".into(),
            "t".into(),
            CrashSeverity::Fatal,
            "err".into(),
            "m".into(),
            "0.1.0".into(),
            "win".into(),
        );
        r1.approve();
        crashes.store(r1).unwrap();
        crashes
            .store(CrashReport::new(
                "r2".into(),
                "t".into(),
                CrashSeverity::Recoverable,
                "err".into(),
                "m".into(),
                "0.1.0".into(),
                "win".into(),
            ))
            .unwrap();
        let surface = DiagnosticSurface::new(&log, &audit, &consent, &crashes, &buckets);
        let summary = surface.crash_summary();
        assert_eq!(summary.total_reports, 2);
        assert_eq!(summary.approved_count, 1);
        assert_eq!(summary.pending_count, 1);
    }
}
