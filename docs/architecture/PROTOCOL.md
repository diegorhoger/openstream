# OpenStream Control Protocol (OSCP) v1

OSCP is the public, versioned contract among Engine, Studio, surfaces, Cloud relay, mobile clients, and future hardware adapters. It is binary Protobuf over TLS WebSocket. Tauri invoke commands are never the device protocol.

## Envelope

Every envelope contains protocol major/minor, session ID, monotonically increasing sequence, globally unique message ID, correlation ID, send time, expiry time, and one body.

Bodies include Hello, CapabilitySnapshot, DeckSnapshot, DeckPatch, ControlEvent, ExecuteRequest, ExecutionUpdate, AssetRequest, AssetChunk, Ack, Error, and Heartbeat.

## Compatibility

- Major mismatch: fail closed.
- Minor changes: additive only; unknown fields ignored.
- `Hello` negotiates features, compression, maximum frame size, and asset limits.
- Generated clients and golden fixtures are committed; CI rejects stale generation.

## Delivery semantics

- Commands are at-least-once transport and exactly-once-observed within the Engine dedupe window.
- Persist `(source_device_id, message_id)` before side effects.
- Commands have short deadlines; expired commands are rejected and never replayed.
- Ordering exists only within one session stream.
- State patches may coalesce under backpressure; commands may not.
- Default frame maximum is 1 MiB; assets are chunked and SHA-256 verified.

Execution states are `accepted`, `running`, `succeeded`, `failed`, `cancelled`, and `expired`. `relayed` is transport evidence, not an execution state.

## Local discovery and enrollment

The Engine advertises `_openstream._tcp.local` only after LAN companions are enabled. mDNS includes instance UUID, OSCP major, port, and a human-safe label—never an account, deck, action, or token.

Pairing uses a two-minute single-use 128-bit QR secret, endpoint hint, protocol version, and TLS certificate fingerprint. The client pins the certificate; both peers complete a standardized authenticated key-exchange transcript, display the same short authentication string, and require desktop confirmation. The Engine then records the peer Ed25519 identity. Peers can be scoped, paused, renamed, and revoked.

Manual connection remains available, but a short numeric code is never the sole secret.

## Remote relay

The Engine connects outward to Cloud. A caller encrypts a short-lived command to an authorized target. Cloud verifies membership, entitlement, revocation, target, rate, TTL, and route metadata, then relays opaque ciphertext. The Engine decrypts, revalidates local grants and graph revision, deduplicates, executes, and returns encrypted authoritative updates.

Cloud never turns “socket write succeeded” into “action succeeded.”
