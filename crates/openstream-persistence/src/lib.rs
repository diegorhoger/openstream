//! `openstream-persistence` — repository traits plus SQLite storage.
//!
//! Owns repository traits, SQLite WAL storage, explicit versioned migrations,
//! soft deletion, and the local-first commit/outbox ordering described in
//! TECHNICAL_SPEC §6. Raw secret values never persist here; they stay in OS
//! credential storage.
//!
//! Status: M0 boundary skeleton. Schema v1 and migration tests arrive with the
//! persistence milestones.
