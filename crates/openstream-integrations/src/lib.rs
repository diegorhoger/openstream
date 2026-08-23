//! `openstream-integrations` — adapter contracts for external systems.
//!
//! Defines the adapter trait surface (OBS, OS automation, HTTP, MIDI, OSC)
//! including adapter-declared idempotency and safe-compensation declarations
//! required before retry/compensation is permitted (TECHNICAL_SPEC §5).
//! Concrete adapters live under `integrations/` in later milestones; this
//! crate owns the contracts only.
//!
//! Status: M0 boundary skeleton.
