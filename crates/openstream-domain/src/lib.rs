//! `openstream-domain` — pure domain types, IDs, and validation.
//!
//! Boundary rules (ADR-0001, TECHNICAL_SPEC §3): this crate imports no UI,
//! database, network, Tauri, or Cloud implementation and stays dependency-free
//! so authority remains testable in isolation.
//!
//! Status: M0 boundary skeleton. Entity types and validation arrive with the
//! M1 domain-model milestones; this skeleton pins the crate boundary so
//! dependents cannot invent parallel authority.

/// Major version of the versioned domain model this crate implements,
/// anchored to `DOMAIN_MODEL.md` v1 (ADR-0005). Breaking domain changes
/// require a major bump plus ADR, migration proof, and human gate.
pub const DOMAIN_MODEL_MAJOR: u32 = 1;

/// Minor version of the versioned domain model this crate implements,
/// anchored to `DOMAIN_MODEL.md` v1 (ADR-0005).
pub const DOMAIN_MODEL_MINOR: u32 = 0;

#[cfg(test)]
mod tests {
    use super::{DOMAIN_MODEL_MAJOR, DOMAIN_MODEL_MINOR};

    #[test]
    fn domain_model_version_matches_documented_v1() {
        assert_eq!(DOMAIN_MODEL_MAJOR, 1);
        assert_eq!(DOMAIN_MODEL_MINOR, 0);
    }
}
