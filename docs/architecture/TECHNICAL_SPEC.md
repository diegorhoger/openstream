# Rust-first technical specification

Status: proposed v0.2

## 1. Authority boundary

The desktop **Engine** owns privileged execution, grants, local secrets, state, durable admission dedupe, and the execution journal. Studio, simulators, Cloud, browsers, and native mobile are untrusted requesters. The Engine revalidates immediately before every side effect.

OpenStream transports control and state, not video or audio media.

## 2. Technology decisions

| Layer | Choice |
|---|---|
| Authoritative core | Stable Rust, pinned toolchain, Tokio |
| Desktop | Tauri 2; React + strict TypeScript + Vite UI |
| Local storage | SQLite WAL with explicit migrations |
| Public protocol | Protobuf OSCP; Noise for native LAN; TLS 1.3 WSS for Cloud |
| Public Cloud clients | Generated Rust/TypeScript/Swift/Kotlin clients and fake services |
| Hosted Cloud | Separate private repository; Rust/Axum/PostgreSQL baseline is a private implementation choice |
| iOS/iPadOS | SwiftUI with narrow Rust UniFFI bindings |
| Android | Jetpack Compose with narrow Rust UniFFI bindings |
| Plugins | Wasmtime Component Model, capability-scoped host imports |
| Assets | Content-addressed SHA-256 blobs |
| Observability | Privacy-filtered facade with public redaction contracts |

No Python ships in product paths. Other languages exist only at UI/platform boundaries. Do not introduce speculative Kubernetes, Kafka, service mesh, or microservices here.

## 3. Public repository target

```text
apps/
  desktop/ui/              React/Vite
  desktop/src-tauri/       desktop composition root
  ios/                     public SwiftUI client
  android/                 public Jetpack Compose client
crates/
  openstream-domain/       pure types, IDs, validation
  openstream-protocol/     OSCP schema/codecs/compatibility
  openstream-engine/       deterministic action graph
  openstream-persistence/  repository traits + SQLite
  openstream-sync/         public operation log and merge rules
  openstream-crypto/       identity and encrypted envelopes
  openstream-discovery/    mDNS adapter and fixtures
  openstream-pairing/      Noise pairing/session state machine
  openstream-plugin-host/  Wasmtime host
  openstream-action-sdk/   SDK helpers
  openstream-integrations/ adapter contracts
  openstream-mobile-ffi/   narrow UniFFI surface
  openstream-testkit/      fake clock/adapters/services/fixtures
integrations/obs/ os-automation/ http/ midi/ osc/
proto/openstream/v1/
wit/openstream-action/
packages/design-tokens/ ui-web/ protocol-generated/ cloud-contracts/
simulators/engine/ surface/ cloud-fake/
migrations/sqlite/
```

Hosted server, PostgreSQL migrations, billing, tenancy, operations, production configuration, and customer-data code are forbidden in this public tree. `domain` imports no UI, database, network, Tauri, or Cloud implementation. `unsafe` is forbidden by default and isolated when a platform/FFI boundary proves it necessary.

## 4. Domain and secrets

Use UUIDv7 IDs, explicit schema versions, UTC metadata, monotonic runtime deadlines, soft deletion, and SHA-256 asset identity. Core entities include Installation, TrustedPeer, Workspace, Deck, Profile, Page, Control, ActionBinding, GraphNode, GraphEdge, Variable, SecretRef, Asset, PluginInstall, IntegrationConnection, Operation, Execution, AuditEvent, SyncCursor, and DedupeEntry.

Secret values stay in OS credential storage. Plugins receive opaque connection handles; the Engine integration broker performs the approved operation without exposing raw secret bytes.

## 5. Action engine and effect truth

Actions are immutable validated DAGs. V1 node types: action, sequence, parallel, delay, conditional, retry, variable transform, and explicit compensating action.

- Validate on save and immediately before execution.
- Maximum 128 nodes, nesting depth 16, default deadline 30 seconds, macro maximum 10 minutes.
- Default per-plugin concurrency four; global 32; cancellation propagates.
- Each run reads an immutable graph revision.
- Retry requires adapter-declared idempotency or explicit reconciliation.
- Persist `prepared` before requesting a side effect and terminal evidence afterward.
- A crash gap becomes `outcome_unknown`; never infer success or automatically retry a non-idempotent effect.
- Failure policy is stop, continue, or compensate only where the adapter declares safe compensation.

## 6. Local persistence and public sync semantics

Local writes commit to SQLite first and append a versioned operation to an outbox. Public sync clients encrypt workspace content before it reaches the hosted service.

Merge rules: field-level LWW by hybrid logical clock plus actor-ID tie-break; tombstones dominate older updates; fractional ordering; grid collisions preserve both edits and mark `needs_resolution`; invalid merged graphs remain stored but disabled. Operational button presses are never sync operations and never execute after expiry.

## 7. Cloud boundary

This repository defines schemas, generated clients, E2EE envelopes, fake services, test vectors, export formats, and conformance expectations. It does not implement accounts, tenancy, billing, hosted web application, production relay, storage, backups, or operations.

The public contract requires outbound Engine connections, opaque short-lived relay payloads, server-readable routing/entitlement metadata, authoritative Engine execution states, user export, and a local-forever downgrade invariant. Hosted readiness may be tracked only through opaque exact-version evidence.

## 8. Mobile boundary

Rust UniFFI owns OSCP codecs, Noise session crypto, model validation, patch application, sync merge, asset verification, and execution state. Native code owns discovery, local-network permission, lifecycle, secure storage, haptics, accessibility, billing UI, and store policy.

Mobile does not execute downloaded third-party code initially. Cached host-dependent controls show unavailable rather than fake success.

## 9. Quality targets

- Desktop dispatch overhead p95 < 10 ms.
- Native LAN host acceptance p95 < 50 ms; visible feedback p95 < 100 ms.
- Cloud relay acceptance p95 < 250 ms excluding action runtime.
- Healthy native LAN reconnect < 2 seconds.
- Desktop startup < 2 seconds on the reference baseline.
- Zero duplicate effects in idempotent adapter conformance; unknown non-idempotent outcomes are surfaced honestly.

## 10. Test system

- Rust unit/property tests with fake clock, integrations, randomness, and side-effect journal.
- Golden protocol fixtures retained across releases and generated-client dirty checks.
- Migration tests, fuzz targets, sandbox denial/resource-limit tests.
- Pairing expiry, replay, MITM, downgrade, revocation, and lost-confirmation tests.
- Simulator fault injection for dropped/duplicated/reordered frames and crash windows.
- Playwright/Tauri, XCTest, Compose UI, and real-device local-network/accessibility matrices.
- Cloud portability tests run against public fakes and, at a human gate, an opaque hosted compatibility endpoint.

## 11. Release system

PR checks include exact-head assertion, DCO, Rust format/Clippy/tests, strict TypeScript, codegen cleanliness, dependency/license/advisory policy, secret scanning, migrations, and accessibility smoke. Releases require signed tags, platform signing/notarization, checksums, SBOM, provenance, protected environments, and human authorization.
