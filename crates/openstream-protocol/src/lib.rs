//! `openstream-protocol` — OSCP schemas, codecs, and compatibility rules.
//!
//! This crate owns the public wire contract described in `PROTOCOL.md`,
//! `OSCP_MESSAGES.md`, and ADR-0003/ADR-0005. It must not embed transport,
//! pairing, discovery, or platform policy.
//!
//! Status: M0 boundary skeleton. Protobuf codegen, codecs, golden fixtures,
//! and negotiation mechanics arrive in M2.

/// OSCP protocol major version (`PROTOCOL.md`). A major mismatch fails
/// closed; this constant is the single anchor every codec negotiates against.
pub const OSCP_PROTOCOL_MAJOR: u16 = 1;

/// OSCP protocol minor version (`PROTOCOL.md`). Minor evolution is additive
/// only within the same major.
pub const OSCP_PROTOCOL_MINOR: u16 = 0;

#[cfg(test)]
mod tests {
    use super::{OSCP_PROTOCOL_MAJOR, OSCP_PROTOCOL_MINOR};

    #[test]
    fn protocol_version_matches_documented_v1() {
        assert_eq!(OSCP_PROTOCOL_MAJOR, 1);
        assert_eq!(OSCP_PROTOCOL_MINOR, 0);
    }
}
