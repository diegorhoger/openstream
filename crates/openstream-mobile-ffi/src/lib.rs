//! `openstream-mobile-ffi` — narrow UniFFI surface for native mobile clients.
//!
//! Exposes only the shared Rust logic mobile needs per TECHNICAL_SPEC §8:
//! OSCP codecs, Noise session crypto, model validation, patch application,
//! sync merge, asset verification, and execution state. The surface stays
//! narrow and versioned; discovery, local-network permission, lifecycle,
//! secure storage, and store policy remain native (Swift/Kotlin) concerns.
//!
//! Status: M0 boundary skeleton. UniFFI bindings arrive with the Stage 3
//! mobile milestones.
