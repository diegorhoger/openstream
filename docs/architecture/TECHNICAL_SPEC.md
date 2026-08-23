# Rust-first technical specification

Status: proposed v0.1

## 1. Authority boundary

The desktop **Engine** owns privileged execution, grants, local secrets, state, deduplication, and the execution journal. Studio, browser, Cloud, and mobile are untrusted requesters. They may render, edit, and request; the Engine validates again immediately before every side effect.

OpenStream transports control and state, not video or audio media.

## 2. Technology decisions

| Layer | Choice |
|---|---|
| Authoritative core | Stable Rust, pinned toolchain, Tokio |
| Desktop | Tauri 2; React + strict TypeScript + Vite UI |
| Local storage | SQLite WAL with explicit migrations |
| Cloud API | Rust Axum modular monolith, SQLx, PostgreSQL |
| Realtime | Versioned binary Protobuf over TLS WebSocket |
| Web | Next.js/React with generated protocol clients |
| iOS/iPadOS | SwiftUI with narrow Rust UniFFI bindings |
| Android | Jetpack Compose with narrow Rust UniFFI bindings |
| Plugins | Wasmtime Component Model, capability-scoped host imports |
| Assets | Content-addressed SHA-256 blobs |
| Observability | Privacy-filtered OpenTelemetry facade |

No Python ships in the product. Other languages exist only at UI/platform boundaries. Start with a modular monolith; do not add Kubernetes, Kafka, a service mesh, or speculative microservices.

## 3. Target monorepo

```text
apps/
  desktop/ui/              React/Vite
  desktop/src-tauri/       desktop composition root
  web/                     Cloud web editor/dashboard
  ios/                     SwiftUI
  android/                 Jetpack Compose
services/
  control-plane/           Axum accounts/workspaces/sync/registry
  realtime-gateway/        authenticated WSS relay
  worker/                  billing/email/retention jobs
crates/
  openstream-domain/       pure types, IDs, validation
  openstream-protocol/     OSCP schema/codecs/compatibility
  openstream-engine/       deterministic action graph
  openstream-persistence/  repository traits + SQLite
  openstream-sync/         operation log and merge rules
  openstream-crypto/       identity and encrypted envelopes
  openstream-discovery/    mDNS
  openstream-pairing/      QR enrollment and peer trust
  openstream-plugin-host/  Wasmtime host
  openstream-action-sdk/   SDK helpers
  openstream-integrations/ adapter contracts
  openstream-mobile-ffi/   narrow UniFFI surface
  openstream-testkit/      fake clock/adapters/fixtures
integrations/obs/ os-automation/ http/ midi/ osc/
proto/openstream/v1/
wit/openstream-action/
packages/design-tokens/ ui-web/ protocol-ts-generated/
migrations/sqlite/ postgres/
```

`domain` imports no UI, database, network, Tauri, or Cloud code. Apps/services are composition roots. `unsafe` is forbidden by default and isolated when a platform/FFI boundary proves it necessary.

## 4. Domain model

Use UUIDv7 IDs, explicit schema versions, UTC metadata, monotonic runtime deadlines, soft deletion, and SHA-256 asset identity.

Core entities: Installation, TrustedPeer, Workspace, Deck, Profile, Page, Control, ActionBinding, GraphNode, GraphEdge, Variable, SecretRef, Asset, PluginInstall, IntegrationConnection, Operation, Execution, AuditEvent, SyncCursor, and DedupeEntry.

Secrets are opaque references to OS credential storage. Secret values never enter SQLite, deck bundles, execution logs, sync operations, telemetry, or protocol diagnostics.

## 5. Action engine

Actions are immutable validated DAGs. V1 node types: action, sequence, parallel, delay, conditional, retry, variable transform, and explicit compensating action.

- Validate on save and again before execution.
- Maximum 128 nodes and nesting depth 16.
- Default deadline 30 seconds; macro maximum 10 minutes.
- Default per-plugin concurrency 4; global 32.
- Cancellation propagates.
- Each run reads an immutable graph revision.
- Retry requires adapter-declared idempotency or explicit user opt-in.
- Compensation exists only when an adapter declares it.
- Failure policy: stop, continue, or compensate.
- Persist lifecycle and dedupe key before side effects.

Triggers include down/up, tap, double-tap, long-press, encoder movement/press, strip value, timer, webhook, and integration event.

## 6. Persistence and sync

Local writes commit to SQLite first and append a versioned operation to an outbox. Cloud sync stores end-to-end encrypted workspace content and server-readable routing/billing metadata.

Merge rules:

- Scalars: field-level last-writer-wins by hybrid logical clock plus actor-ID tie-break.
- Deletion: tombstone dominates older updates.
- Ordering: fractional order keys.
- Grid collision: deterministic visible winner, preserve both edits, mark `needs_resolution`.
- Invalid merged graphs remain stored but disabled.

Operational button presses are never sync operations and never execute after their deadline.

## 7. Cloud topology

Begin with control-plane, realtime-gateway, and worker deployables backed by PostgreSQL, an S3-compatible store, and a transactional outbox. Redis/NATS are introduced only with measured need.

The Engine opens an outbound authenticated WebSocket. Relay acknowledgements mean relayed, never executed. Only the Engine may produce authoritative accepted/running/succeeded/failed execution states.

Workspace content is client encrypted. A per-workspace key is sealed to authorized device public keys. Device revocation rotates future keys. Publishing a template creates a separate explicit public copy.

## 8. Mobile boundary

Rust UniFFI owns OSCP codecs, certificate pin/session crypto, model validation, patch application, sync merge, asset verification, and execution state. Native code owns discovery, local-network permission, lifecycle, secure storage, haptics, accessibility, billing UI, and store policy.

Mobile does not execute downloaded third-party code initially. Cached host-dependent controls show unavailable rather than fake success.

## 9. Quality targets

- Desktop dispatch overhead p95 < 10 ms.
- LAN host acceptance p95 < 50 ms; visible feedback p95 < 100 ms.
- Cloud relay acceptance p95 < 250 ms excluding action runtime.
- Healthy LAN reconnect < 2 seconds.
- Desktop startup < 2 seconds on the reference baseline.
- Zero duplicate side effects in the idempotent conformance suite.

## 10. Test system

- Rust unit/property tests with fake clock, integrations, and randomness.
- Golden protocol fixtures retained across releases.
- Migration tests from every supported schema.
- Fuzz targets for protocol, bundle, manifest, graph, and sync parsers.
- Sandbox denial/resource-limit tests.
- Pairing replay, expiry, MITM, and revocation tests.
- Playwright/Tauri, XCTest, Compose UI, and real-device local-network/accessibility matrices.
- End-to-end dropped/duplicated/reordered frames, conflicting offline sync, fake/real OBS qualification, upgrade/rollback, and corrupted assets.

## 11. Release system

PR checks include Rust format/Clippy/tests, strict TypeScript, codegen cleanliness, dependency/license/advisory policy, secret scanning, migrations, and accessibility smoke. Release artifacts require signed tags, platform signing/notarization, checksums, SBOM, provenance, updater signatures, protected environments, and human authorization.
