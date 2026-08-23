# OpenStream Control Protocol (OSCP) v1

OSCP is the public, versioned contract among Engine, Studio, conformance simulators, Cloud relay, native mobile clients, and future hardware adapters. Canonical messages are Protobuf. Transport security depends on the stage and trust boundary; Tauri invoke commands are never the public device protocol.

The typed message schemas, negotiation mechanics, error registry, recovery flows, and fixture obligations extending this summary with issue-#3 precision live in [`OSCP_MESSAGES.md`](OSCP_MESSAGES.md); the versioned domain model lives in [`DOMAIN_MODEL.md`](DOMAIN_MODEL.md) (ADR-0005). Where wording appears to conflict, this file plus ADR-0005 govern.

## Supported transports

- **Desktop-local:** in-process or narrowly scoped Tauri IPC; no public listener is required.
- **Native LAN:** binary OSCP frames inside a Noise transport session. Enrollment uses `Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s`; established peers use `Noise_IK_25519_ChaChaPoly_BLAKE2s`.
- **Cloud:** authenticated TLS 1.3 WebSocket to the hosted service plus OSCP end-to-end encrypted relay envelopes.
- **Browser:** Cloud transport only. Direct browser-to-LAN OSCP is unsupported.

M2 implements codecs, state machines, generated clients, simulators, fixtures, and conformance. A real LAN listener is not enabled for users until native mobile work explicitly ships and passes its permission/security gate.

## Envelope

Every envelope contains protocol major/minor, session ID, monotonically increasing sequence, globally unique message ID, correlation ID, send time, expiry time, and one body.

Bodies include Hello, CapabilitySnapshot, DeckSnapshot, DeckPatch, ControlEvent, ExecuteRequest, ExecutionUpdate, AssetRequest, AssetChunk, Ack, Error, and Heartbeat.

## Compatibility

- Major mismatch: fail closed.
- Minor changes: additive only; unknown fields are ignored only where the schema marks them forward-compatible.
- `Hello` negotiates additive features, compression, maximum frame size, and asset limits after the secure session exists.
- Noise pattern and cipher suite are fixed by OSCP major and are not negotiated.
- Generated clients and golden fixtures are committed; CI rejects stale generation.

## Honest delivery and recovery semantics

- Transport delivery is at least once; Engine admission is deduplicated by `(source_device_id, message_id)` within a bounded durable window.
- The Engine persists a prepared invocation record before requesting an external side effect and persists the terminal result afterward.
- A crash between those records produces `outcome_unknown`; it must never be reported as success or automatically retried for a non-idempotent adapter.
- Replay after `outcome_unknown` is permitted only when the adapter supplies a stable idempotency key or a reconciliation operation proves the effect did not occur.
- Commands have short deadlines; expired commands are rejected and never queued for later execution.
- Ordering exists only within one session stream. State patches may coalesce under backpressure; commands may not.
- Default frame maximum is 1 MiB; assets are chunked and SHA-256 verified.

Execution states are `accepted`, `running`, `succeeded`, `failed`, `cancelled`, `expired`, and `outcome_unknown`. `relayed` is transport evidence, not an execution state. OpenStream does not claim exactly-once external side effects.

## Native LAN enrollment

The desktop creates a two-minute, single-use QR payload containing:

- OSCP major and pairing protocol version;
- Engine instance UUID and endpoint hint;
- Engine static X25519 public key;
- 32-byte cryptographically random pre-shared key;
- pairing identifier and expiry.

The native client creates its X25519 static key in OS-protected storage and runs `Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s`. Both peers use the exact prologue `OpenStream-Pairing-v1 || oscp_major || pairing_id || engine_instance_uuid`. The QR PSK is consumed once and erased on success, expiry, cancellation, or five failed attempts.

Both peers derive the displayed six-word/30-bit short authentication string from `BLAKE2s("OpenStream-SAS-v1" || handshake_hash)`. The desktop displays the candidate device, requested scopes, and SAS and requires explicit confirmation before persisting the peer key. The client must show pending until authoritative confirmation arrives.

Subsequent sessions use `Noise_IK_25519_ChaChaPoly_BLAKE2s` with stored static keys and prologue `OpenStream-OSCP-v1 || oscp_major || engine_instance_uuid`. There is no downgrade fallback to another pattern, cipher suite, plaintext session, numeric-only secret, or legacy major. Revocation deletes trust and terminates live sessions.

The cryptographic state machine and vectors require independent security review before a real LAN listener ships.

## Discovery

When the user explicitly enables native LAN control, the Engine may advertise `_openstream._tcp.local` with instance UUID, OSCP major, port, and a human-safe label. It never advertises an account, deck, action, token, public key, PSK, or device membership. Manual QR endpoint fallback remains available.

## Remote relay

The Engine connects outward to Cloud. A caller encrypts a short-lived command to an authorized target. Cloud verifies server-readable membership, entitlement, revocation, target, rate, TTL, and route metadata, then relays opaque ciphertext. The Engine decrypts, revalidates local grants and graph revision, deduplicates admission, executes, and returns encrypted authoritative updates.

Cloud never turns socket delivery into action success.
