//! Shared fixtures for the SQLite storage integration tests (issue #15).
//!
//! Entry builders mirror the runtime's real shapes: Engine-minted UUIDv7
//! execution ids, envelope message ids, validated device identities, and
//! registry-aligned failure tokens. Nothing here fabricates secret-shaped
//! content except where the no-secrets proof deliberately plants a
//! sentinel it controls.

#![allow(missing_docs)]
#![allow(dead_code)]

use openstream_engine::{
    AdmissionEntry, DedupeKey, ExecutionId, JournalLifecycle, MessageId, NodeKey, PreparedEntry,
    SourceDeviceId,
};
#[allow(unused_imports)]
pub use openstream_persistence::sqlite::JournalBounds;
use std::path::PathBuf;

/// A per-test scratch directory; removed when the guard drops.
pub struct ScratchDir {
    path: PathBuf,
    _guard: tempfile::TempDir,
}

impl ScratchDir {
    pub fn new(label: &str) -> Self {
        let temp = tempfile::tempdir().expect("temp dir available");
        let path = temp.path().join(format!("{label}.sqlite3"));
        Self { path, _guard: temp }
    }

    pub fn db_path(&self) -> PathBuf {
        self.path.clone()
    }
}

pub fn device(name: &str) -> SourceDeviceId {
    SourceDeviceId::try_new(name).expect("fixture device id is valid")
}

pub const DEVICE_A: &str = "peer:015f6a1c-7b21-7cc0-9f31-0e3d5a9d4c11";
pub const DEVICE_B: &str = "installation.local";

pub fn dedupe(source: &str) -> DedupeKey {
    DedupeKey::new(device(source), MessageId::generate())
}

pub fn admission(key: DedupeKey, accepted_at_wall_ms: i64) -> AdmissionEntry {
    AdmissionEntry {
        key,
        execution_id: ExecutionId::generate(),
        accepted_at_wall_ms,
        expires_at_wall_ms: accepted_at_wall_ms + 30_000,
        lifecycle: JournalLifecycle::Accepted,
    }
}

pub fn prepared(execution_id: ExecutionId, node: &str, attempt: u32) -> PreparedEntry {
    PreparedEntry {
        execution_id,
        node_key: NodeKey::try_new(node).expect("fixture node key is valid"),
        attempt,
        action_type: "midi.tap".to_string(),
        idempotency_key: format!("{execution_id}:{node}:{attempt}"),
        prepared_at_monotonic_ms: 1_000,
    }
}

pub const NODE_EFFECT: &str = "obs-scene-set";
