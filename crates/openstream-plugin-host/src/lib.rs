//! `openstream-plugin-host` — Wasmtime host for capability-scoped plugins.
//!
//! Hosts plugins compiled to the Wasmtime Component Model with narrow,
//! capability-scoped imports, memory/fuel/time limits, and sandbox denial
//! tests (TECHNICAL_SPEC §2, SECURITY.md). Plugins receive opaque connection
//! handles only; the Engine integration broker performs approved operations
//! so raw secret bytes never enter plugin memory.
//!
//! Status: M0 boundary skeleton. The WIT world (`wit/openstream-action/`) and
//! host arrive with the plugin milestones.
