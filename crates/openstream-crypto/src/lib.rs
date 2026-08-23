//! `openstream-crypto` — identity and encrypted envelopes.
//!
//! Owns Engine/peer identity keys and the E2EE envelope layer used before any
//! content reaches a hosted service. Hard rules (SECURITY.md): no homegrown
//! cryptographic primitive, no silent suite fallback, no raw secret in logs,
//! storage, or telemetry. Suite changes require a security ADR and human gate.
//!
//! Status: M0 boundary skeleton. Noise suites (`Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s`
//! enrollment, `Noise_IK_25519_ChaChaPoly_BLAKE2s` established peers) arrive
//! with the pairing/crypto milestones.
