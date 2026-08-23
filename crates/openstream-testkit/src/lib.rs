//! `openstream-testkit` — deterministic test infrastructure shared by crates.
//!
//! Provides fake clocks, fake adapters/services, seeded randomness, side-effect
//! journals, and golden-fixture helpers so tests stay deterministic and honest
//! (TECHNICAL_SPEC §10). Test-only by convention: production crates must not
//! depend on this crate outside `dev-dependencies`.
//!
//! Status: M0 boundary skeleton. Fixtures arrive with the protocol and engine
//! milestones.
