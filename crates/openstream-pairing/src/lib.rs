//! `openstream-pairing` — Noise pairing and session state machine.
//!
//! Implements enrollment via a two-minute, single-use QR payload (fixed
//! `Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s` suite, 32-byte random PSK, exact
//! prologue, SAS comparison, explicit desktop confirmation) and established
//! peer sessions with pause/revoke/revoke-all (PROTOCOL.md, SECURITY.md).
//! No suite fallback exists; pairing failures fail closed.
//!
//! Status: M0 boundary skeleton. Pairing flows arrive with the LAN security
//! milestones behind their permission/security gate.
