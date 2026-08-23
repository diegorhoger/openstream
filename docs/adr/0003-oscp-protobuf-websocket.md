# ADR-0003: OSCP Protobuf with boundary-specific secure transports

Status: accepted for foundation review  
Date: 2026-08-23

## Decision

OSCP uses canonical Protobuf messages with explicit version negotiation, IDs, sequence, correlation, expiry, bounded admission dedupe, snapshot/patch recovery, and authoritative execution states.

- Desktop-local composition uses in-process/Tauri IPC.
- Native LAN uses Noise transport: enrollment `Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s`, established peers `Noise_IK_25519_ChaChaPoly_BLAKE2s`.
- Cloud uses TLS 1.3 WebSocket plus end-to-end encrypted OSCP relay envelopes.
- Browser clients use Cloud only; direct browser-to-LAN control is unsupported.

## Rationale

The original self-signed WSS/PWA plan could not provide ordinary browsers with a portable certificate-pinning and secure-origin model. Native clients can own OS-protected identity, local-network permission, and a fixed reviewed Noise state machine. Protobuf remains shared while transport matches the real trust boundary.

## Consequences

Generated code, golden fixtures, pairing vectors, simulator fault injection, and downgrade tests are mandatory. M2 is a contract/conformance milestone, not a phone PWA. A real LAN listener cannot ship before independent security review and the Stage 3 native permission gate.

Exactly-once external effects are not claimed; crash gaps surface `outcome_unknown`.
