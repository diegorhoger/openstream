# OSCP v1 typed messages, errors, and recovery

Status: specified contract (M0); protobuf blocks below are normative spec text for M2 codegen — no generated artifacts exist in this repository yet  
Authority: extends `docs/architecture/PROTOCOL.md` with the precision required by issue #3 under `docs/adr/0005-versioned-domain-model-action-protocol.md`. This file adds no transport, cipher, pairing, or discovery content; those remain governed by `PROTOCOL.md`. Where wording appears to conflict, `PROTOCOL.md` plus ADR-0005 govern.

## 1. Identifier conventions

- `session_id`, `message_id`, `correlation_id`, `execution_id`, and device/engine identifiers are UUIDv7 in canonical lowercase hyphenated form (`DOMAIN_MODEL.md` §2).
- `correlation_id` echoes the `message_id` of the request being answered; unsolicited messages correlate to themselves.
- `execution_id` is assigned by the Engine at admission and never minted by callers.
- `sequence` is assigned by the sender, monotonically non-decreasing within one session stream. Regression fails closed (`SEQUENCE_REGRESSION`). Duplicates are expected under at-least-once delivery and resolved by dedupe (§7), not by sequence logic.
- Ordering guarantees exist only within one session stream (`PROTOCOL.md`).

## 2. Envelope and bodies

Exactly the twelve body types of `PROTOCOL.md` v1 are defined here; adding a body is additive-minor when marked forward-compatible.

```proto
syntax = "proto3";
package openstream.oscp.v1;

import "google/protobuf/timestamp.proto";

// Canonical envelope. Field inventory matches PROTOCOL.md "Envelope" verbatim.
message Envelope {
  uint32 protocol_major = 1;                 // must equal 1 for this major
  uint32 protocol_minor = 2;                 // additive within major
  string session_id = 3;                     // UUIDv7; new value on every transport session
  uint64 sequence = 4;                       // per-sender monotonic non-decreasing
  string message_id = 5;                     // UUIDv7; globally unique; never reused
  string correlation_id = 6;                 // UUIDv7 of initiating message, else self
  google.protobuf.Timestamp sent_at = 7;     // UTC
  google.protobuf.Timestamp expires_at = 8;  // UTC; mandatory for command-class bodies
  oneof body {
    Hello hello = 16;
    CapabilitySnapshot capability_snapshot = 17;
    DeckSnapshot deck_snapshot = 18;
    DeckPatch deck_patch = 19;
    ControlEvent control_event = 20;
    ExecuteRequest execute_request = 21;
    ExecutionUpdate execution_update = 22;
    AssetRequest asset_request = 23;
    AssetChunk asset_chunk = 24;
    Ack ack = 25;
    OscpError error = 26;                      // reserved word avoided at message level
    Heartbeat heartbeat = 27;
  }
}
```

```proto
// Negotiation and recovery hint. Sent by both peers after the secure session exists.
message Hello {
  repeated string features = 1;        // additive registry; unknown values ignored by receiver
  repeated string compressions = 2;    // offered algorithms; result = intersection, else none
  uint64 max_frame_size_bytes = 3;     // result = min(offered, received); hard cap 1 MiB default
  uint64 asset_chunk_max_bytes = 4;    // result = min
  repeated uint32 domain_schema_majors = 5;   // DOMAIN_MODEL.md majors the peer can read
  RecoveryHint recovery = 6;           // optional; drives §10
}
message RecoveryHint {
  string workspace_id = 1;             // UUIDv7
  uint64 last_known_deck_revision = 2; // 0 = no cached state; full snapshot required
}

message CapabilitySnapshot {
  google.protobuf.Timestamp issued_at = 1;
  repeated ConnectionStatus connections = 2; // typed availability entries only; never credentials
}
message ConnectionStatus {
  string connection_id = 1;            // UUIDv7 of IntegrationConnection
  string connector_type = 2;           // registry string, e.g. "obs.ws"
  bool available = 3;
  string detail = 4;                   // redaction-safe diagnostic string
}

message DeckSnapshot {
  string workspace_id = 1;             // UUIDv7
  string deck_id = 2;                  // UUIDv7
  uint64 revision = 3;                 // deck revision this snapshot pins
  uint32 domain_schema_version_major = 4;
  string snapshot_id = 5;              // UUIDv7
  google.protobuf.Timestamp issued_at = 6;
  bytes payload = 7;                   // serialized domain document set (pages/controls/bindings/variables)
}

message DeckPatch {
  string deck_id = 1;
  uint64 base_revision = 2;            // must equal receiver's current revision
  uint64 target_revision = 3;          // base_revision + n; applied atomically or rejected
  repeated PatchOp ops = 4;
}
message PatchOp { /* add/update/remove page | control | binding | variable; typed per op */ }

message ControlEvent {
  string control_id = 1;               // UUIDv7
  string binding_id = 2;               // UUIDv7; empty if control-level event
  string trigger = 3;                  // press|release|hold_begin|hold_end|long_press|repeat
  google.protobuf.Timestamp occurred_at = 4;
}

message ExecuteRequest {
  string binding_id = 1;               // preconfigured binding; remote surfaces may never author effects
  bytes arguments = 2;                 // typed, bounded payload validated against binding schema
}

message ExecutionUpdate {
  string execution_id = 1;             // UUIDv7
  string state = 2;                    // one of the seven authoritative states (§8)
  string failure_reason_code = 3;      // error registry key when state = failed
  string detail = 4;                   // redaction-safe
  google.protobuf.Timestamp emitted_at = 5;
  bool coalescable = 6;                // true only for intermediate running/progress frames
}

message AssetRequest  { string asset_sha256 = 1; uint64 offset = 2; }
message AssetChunk    { string asset_sha256 = 1; uint64 offset = 2; bytes data = 3; bool final = 4; }

message Ack {
  string acked_message_id = 1;         // UUIDv7 echo
  bool duplicate_suppressed = 2;       // dedupe hit (§7)
  string original_execution_id = 3;    // set when duplicate_suppressed
}

message OscpError {
  string code = 1;                     // registry key from §9
  string detail = 2;                   // redaction-safe; no labels, paths, URLs, tokens
  string offending_message_id = 3;     // UUIDv7 echo
  bool retryable = 4;
  google.protobuf.Timestamp retry_after = 5; // optional hint
}

message Heartbeat { uint64 sender_tick = 1; } // monotonic liveness counter
```

## 3. Version negotiation

Negotiation happens only after the secure transport session exists (`PROTOCOL.md`, Compatibility) and proceeds:

1. Both peers send `Hello`.
2. Protocol majors must match both directions; mismatch emits `PROTOCOL_MAJOR_MISMATCH` and closes the session. No fallback pattern, cipher, plaintext path, or legacy major exists.
3. Effective minor is `min(advertised minors)` within the matched major.
4. Features resolve by intersection over the additive string registry; unknown feature strings are ignored by receivers (forward-compatible), so a peer advertising nothing loses nothing it needs.
5. Compression, frame size, and asset-chunk limits take the minimum of offers; frame size never exceeds the fixed default maximum of 1 MiB unless a later minor explicitly raises it for both peers.
6. The negotiated tuple binds to `session_id` and applies for that session's lifetime; renegotiation requires a new session.
7. Peers unable to read any of the other's `domain_schema_majors` refuse deck traffic with `DOMAIN_SCHEMA_MISMATCH` while remaining otherwise connected.

## 4. Delivery classes

| Class | Bodies | Coalescing | Ordering |
|---|---|---|---|
| Command | `ControlEvent`, `ExecuteRequest`, `AssetRequest` (cancellation is graph policy per `TECHNICAL_SPEC.md` §5, not a separate body) | Never | Admission order within the session stream |
| State | `ExecutionUpdate` frames marked `coalescable`, `CapabilitySnapshot`, `Heartbeat` | May coalesce under backpressure | Latest wins |
| Structural | `DeckSnapshot`, `DeckPatch` | Never | Revision-chained (§10) |
| Transport | `Hello`, `Ack`, `OscpError`, `AssetChunk` | Never | Per stream |

Terminal execution updates are never dropped or coalesced away: every admitted command eventually produces exactly one terminal journal state or remains honestly `outcome_unknown`.

## 5. Expiry and deadlines

- `expires_at` is mandatory on command-class bodies; advisory elsewhere. Admission rejects expired work with `EXPIRED` and journals `expired`; expired commands are never queued for later execution (`PROTOCOL.md`).
- After admission, deadline measurement switches to monotonic runtime clocks: invocation deadline defaults to 30 seconds, macro cap 10 minutes (`TECHNICAL_SPEC.md` §5). Wall-clock skew cannot extend a running effect's deadline.
- Pairing QR payloads keep their own two-minute single-use lifetime (`PROTOCOL.md`); Cloud relay TTLs are server-side metadata bounds on top of, never instead of, envelope expiry.

## 6. Admission pipeline

Each inbound command passes ordered checks; every failure emits a typed `OscpError` (§9) and, where an execution exists, journals the mapped terminal state:

1. Session authenticity and peer scope (transport layer; Noise/TLS established earlier).
2. Major/minor check → `PROTOCOL_MAJOR_MISMATCH` / `PROTOCOL_MINOR_UNSUPPORTED`.
3. Frame-size and parse checks → `FRAME_TOO_LARGE` / `MALFORMED_ENVELOPE`.
4. Expiry → `EXPIRED`, journal `expired`.
5. Sequence regression → `SEQUENCE_REGRESSION`.
6. Dedupe (§7) → suppressed duplicate answered via `Ack(duplicate_suppressed=true)` without re-execution.
7. Authorization: peer scopes, taxonomy §2 six-layer intersection, pinned graph revision, rate limits → denials emit `CAPABILITY_DENIED` / `RATE_LIMITED` / `REVISION_CONFLICT` and journal `failed` with the typed reason.
8. Persist prepared record → execute → persist terminal record (§8).

## 7. Admission dedupe and idempotency

- Key: `(source_device_id, message_id)`, durable in the `DedupeEntry` table (`DOMAIN_MODEL.md` §3). `source_device_id` is the trusted peer identity for LAN/mobile peers, the local installation identity for desktop-local IPC, and the Engine-mapped device membership for Cloud relay.
- Retention: default 24 hours, configurable within hard bounds [1 hour, 7 days], pruned oldest-first. Entries whose outcome state is `outcome_unknown` are exempt from pruning until reconciled.
- Duplicate admission returns `Ack(acked_message_id, duplicate_suppressed=true, original_execution_id=…)` citing the original execution; the engine never re-runs the effect.
- Idempotency keys: adapters declare an idempotency class at registration. When replay is permitted (adapter-supplied stable idempotency key, or reconciliation proving the prior effect did not occur — `PROTOCOL.md`), the Engine derives the adapter-facing idempotency key deterministically from `(source_device_id, original message_id)` so adapter-side collapse is possible.
- Non-idempotent adapters receive no automatic retry after `outcome_unknown`; OpenStream claims no exactly-once external side effects (`PROTOCOL.md`, honesty rule).

## 8. Authoritative execution states

The complete state set is exactly: `accepted`, `running`, `succeeded`, `failed`, `cancelled`, `expired`, `outcome_unknown` (`PROTOCOL.md`). `relayed` is transport evidence, never an execution state.

| From | To | Condition |
|---|---|---|
| — | accepted | Admission pipeline §6 passed; `execution_id` assigned |
| accepted | running | First effect attempt begins |
| accepted/running | succeeded / failed / cancelled | Terminal evidence persisted |
| accepted | expired | Expiry detected before first attempt |
| running | expired | Monotonic deadline elapsed mid-run where the adapter supports safe abort |
| accepted/running | outcome_unknown | Crash window between the durable prepared record and the terminal record |

Rules:

- The Engine persists the prepared invocation record before requesting an external side effect and the terminal result afterward; the gap is surfaced as `outcome_unknown` and must never be reported as success nor automatically retried for a non-idempotent adapter (`PROTOCOL.md`, honest delivery).
- `outcome_unknown` may be superseded only by explicit reconciliation evidence recorded as a corrective journal entry linking the same `execution_id`. Automatic inference of success or failure is forbidden.
- UIs render execution truth exclusively from these journal states (`SECURITY.md`: no success before authoritative Engine result).

## 9. Error registry v1

All errors travel as `OscpError`; `detail` strings obey redaction rules (no labels, configs, file paths, URLs, tokens, scene names). Denials always journal as `failed` with the typed reason; nothing converts silently into success.

| Code | Trigger | Journal effect | Retryable |
|---|---|---|---|
| PROTOCOL_MAJOR_MISMATCH | Envelope major ≠ negotiated major | none (session closes) | no |
| PROTOCOL_MINOR_UNSUPPORTED | Required field/body outside effective minor | none | no |
| DOMAIN_SCHEMA_MISMATCH | Peer cannot read any offered domain schema major | none | no |
| FRAME_TOO_LARGE | Body exceeds negotiated frame limit | none | no |
| MALFORMED_ENVELOPE | Decode/validation S1 failure | none | no |
| SEQUENCE_REGRESSION | Sender sequence decreased | none | no |
| EXPIRED | `expires_at` passed at admission | `expired` | no |
| DUPLICATE_SUPPRESSED | Dedupe key already admitted | cites original execution | informational |
| RATE_LIMITED | Peer/route rate bound exceeded | `failed` (typed reason) | yes, after `retry_after` |
| PEER_REVOKED | Trust revoked mid-session | none (session terminates) | no |
| CAPABILITY_DENIED | Taxonomy §2 intersection fails | `failed` + typed reason | no |
| GRAPH_INVALID | S2–S4 validation failure at runtime re-check | `failed` + typed reason | no |
| REVISION_CONFLICT | Pinned revision no longer current | `failed`; resync via §10 | yes, after resync |
| DEADLINE_EXCEEDED | Monotonic deadline elapsed | `failed` | adapter-dependent |
| ADAPTER_UNAVAILABLE | Target connection unavailable | `failed` | yes |
| ASSET_HASH_MISMATCH | Chunk assembly hash mismatch | `failed` | yes |
| OUTCOME_UNKNOWN_REPORTED | Query/crash-gap surfacing of an unresolved execution | stays `outcome_unknown` | per §7 replay rules |
| INTERNAL_ERROR | Unexpected engine fault, redacted | `failed` (conservative) | no |

## 10. Snapshot/patch recovery

Reconnect always opens a fresh `session_id`; sequence state never carries across sessions. Recovery rides `Hello.recovery` plus existing bodies:

1. Client sends `Hello` including `recovery.last_known_deck_revision`.
2. Server replies `CapabilitySnapshot`.
3. If the client hint is absent or below the server's patch retention floor, the server sends `DeckSnapshot(revision=R)` (full state).
4. Otherwise the server sends a chained series of `DeckPatch(base=rᵢ, target=rᵢ₊₁)` ending at the current revision.
5. The client applies each patch atomically iff `base_revision` equals its current revision; any mismatch discards partial application and requests a full snapshot by re-sending `Hello` with `last_known_deck_revision = 0`.
6. Structural patches are never coalesced; state-class updates may coalesce under backpressure but terminal execution updates never drop (§4).
7. Every run reads its immutable graph revision; a `REVISION_CONFLICT` forces the same resync path before further triggers admit.

## 11. Golden fixtures and conformance plan

Fixture classes (all committed, release-retained, CI-rejected when stale — `PROTOCOL.md`, Compatibility; TM-SUP-04):

| Class | Contents |
|---|---|
| F1 Canonical encodings | One byte-exact vector per body type at the current protocol minor, plus JSON diagnostic renderings |
| F2 Negotiation matrices | Major mismatch, minor intersection, unknown-feature tolerance, limit minimization |
| F3 Replay/duplication | Duplicated, reordered, delayed, and replayed command sequences with fake-clock timestamps |
| F4 Expiry boundaries | Commands at/before/after `expires_at`; monotonic-deadline overrun vectors |
| F5 Crash windows | Prepared-without-terminal records producing `outcome_unknown`; reconciliation-corrective sequences |
| F6 Recovery chains | Snapshot→patch chains, conflicting bases, coalesced-state backpressure runs |
| F7 Error vectors | One vector per registry code in §9 with expected journal effects |
| F8 Cross-language parity | Rust/TypeScript/Swift/Kotlin decode equality over F1–F7 |

Rules: planned locations follow `TECHNICAL_SPEC.md` §3 (`proto/openstream/v1/`, `simulators/`, `openstream-testkit`); generation uses fixed seeds and fake clocks; fixtures contain synthetic identifiers only — no secrets, credentials, or personal data; every protocol/domain change PR must add or update fixture vectors and reference them in the PR description.

## 12. Status honesty statement

At base commit `554e0f97fcfd29c703b7e5fe5eb040088ec2f784` no codec, transport, or simulator exists; everything above is *specified* contract implemented and verified by M2 issues #24–#31. This section must be updated, not deleted, as implementations land.

## References

- `docs/architecture/PROTOCOL.md` — transports, envelope summary, honest delivery, pairing, discovery, relay
- `docs/architecture/DOMAIN_MODEL.md` — entities, controls, graphs, validation, portability
- `docs/security/THREAT_MODEL.md` — TM-RPL-01..04 replay/admission coverage
- `docs/adr/0003-oscp-protobuf-websocket.md`, `docs/adr/0005-versioned-domain-model-action-protocol.md`
