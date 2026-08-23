# OpenStream product requirements

Status: proposed v0.2  
Owner: `@diegorhoger`  
Last updated: 2026-08-23

## 1. Problem

Professional control surfaces are useful but often closed, hardware-bound, account-bound, platform-limited, or intimidating to configure. Creators need a dependable control plane that works offline, explains failures during a live session, and can grow into a synchronized multi-device workspace without making local execution subscription-dependent.

OpenStream is a control surface, not an OBS replacement, video encoder, audio mixer, or arbitrary remote shell.

## 2. Users

- Solo creators who need scenes, sources, audio, media, and shortcuts at hand.
- Professional operators who need deterministic multi-actions, live state, diagnostics, and multiple surfaces.
- Teams who need shared decks, review, history, roles, and fleet management.
- Developers and hardware builders who need open protocols, clients, and conformance fixtures.
- Users who need keyboard, screen-reader, high-contrast, reduced-motion, or large-target operation.

## 3. The magic loop

1. **Discover** OBS and supported local capabilities.
2. **Compose** a useful starter deck automatically.
3. **Preview** or dry-run actions before going live.
4. **Perform** with low-latency, two-way state feedback.
5. **Explain** every outcome in a readable execution timeline.
6. **Improve** through versioned edits and portable templates.

First-run target: at least 80% of first-time test users reach a working OBS scene action within three minutes without assistance.

## 4. Stage 1 — Community desktop

### Must ship

- Windows-first desktop application with macOS/Linux-compatible boundaries.
- Local Rust Engine, Tauri Studio, on-screen deck, execution journal, and atomic local persistence.
- No account, mandatory telemetry, public listener, or internet dependency.
- Decks, profiles, pages, folders, variable grids, drag/drop, undo/redo, and profile switching.
- Typed idle, pressed, armed, running, success, failure, disabled, and disconnected states.
- Press, release, hold, repeat, toggle, sequence, delay, condition, timeout, cancellation, and fail-fast policies.
- OBS discovery, starter deck generation, scenes, sources, audio mute, streaming, recording, replay buffer, and live feedback.
- Initial built-ins: hotkey, URL/file/application launch, media transport, soundboard, and capability-scoped HTTP.
- Preview/dry-run and a per-invocation execution timeline.
- Versioned import/export with secret redaction and local backup.
- OS credential-vault references; no plaintext secrets.
- Capability declarations and grants for every action and connector.
- Keyboard navigation, screen-reader labels, high contrast, reduced motion, and large targets.
- Localization foundation for English and Brazilian Portuguese.

### Stage 1 exclusions

- No browser/PWA LAN controller, Cloud account, billing, collaboration, or internet relay.
- No native App Store/Play Store client.
- No unrestricted process or shell execution.
- No third-party plugin runtime or broad provider promise before M5.
- No proprietary plugin assets or mandatory generative AI.

macOS/Linux parity and additional built-ins follow only after the OBS-first alpha qualifies.

## 5. Stage 2 — OpenStream Cloud web

The optional paid web application provides account/device management, E2EE sync and backup, version history, conflict resolution, web editing, encrypted remote relay, teams and roles, audit, managed distribution, billing, export, deletion, retention, and support.

### Source boundary

- Hosted Cloud server, tenancy, billing, production operations, and private marketplace code live in a separate private repository.
- This public repository retains public wire/storage contracts, generated clients, local Engine behavior, portable formats, cryptographic descriptions and vectors, fake services, and portability/conformance suites.
- Public issues #32–#42 track those public deliverables and opaque cross-repository readiness evidence; they do not authorize hosted server implementation here.

### Cloud invariants

- The Engine remains the only authority for OS actions and local connector secrets.
- Browsers communicate with Engines only through the authenticated Cloud relay; direct browser-to-LAN control is unsupported.
- Cloud never accepts arbitrary shell text or converts relay delivery into execution success.
- Subscription expiry or Cloud outage cannot impair local execution.
- Users can export their configurations and applicable history.
- Telemetry and analytics remain explicit opt-in.

## 6. Stage 3 — Native mobile

- Public SwiftUI iOS/iPadOS and Jetpack Compose Android clients over a narrow public Rust SDK.
- Free, accountless secure LAN mode with unlimited controls.
- QR pairing, local discovery, OS-protected device identity, revocation, and manual fallback.
- Cached decks, instant startup, reconnect, deduplicated admission, and visible connection/latency health.
- Portrait/landscape grids, one-button mode, tablet panels, haptics, long press, repeat, sliders, and knobs.
- VoiceOver/TalkBack, dynamic type, high contrast, safe areas, and reduced motion.
- Optional Cloud relay for subscribers.

Mobile initially does not run downloaded plugins, encode streams, author arbitrary remote commands, or require Cloud for LAN use.

## 7. Commercial hypothesis

| Plan | Price hypothesis | Boundary |
|---|---:|---|
| Community | Free forever | Complete local Engine/Studio, unlimited local decks/workflows, built-ins, SDKs, import/export, and templates |
| Creator Cloud | US$4.99/mo or US$39.99/yr | E2EE sync, backups, 90-day history, web editor, remote relay, hosted assistance, five Engines |
| Team | US$10/user/mo annual or US$12 monthly | Shared workspaces, roles, approvals, audit, private templates, one-year history |
| Enterprise | Custom | SSO/SCIM, fleet policy, private registry, regional hosting, SLA, support |

Pricing remains a hypothesis until beta evidence. Regional purchasing-power pricing, education/nonprofit discounts, and a trial require later human decisions.

## 8. Success gates

### Desktop alpha

- 80% complete install-to-working-OBS-action in under three minutes without assistance.
- Desktop dispatch overhead p95 <= 10 ms on the reference matrix.
- 99.99% correct admission across 100,000 automated presses; idempotent adapters produce zero duplicate effects.
- Four-hour soak without lost accepted work, unbounded memory growth, or false success.
- Crash-free sessions >= 99.5%; outbound-network-blocked suite passes.
- Every destructive action requires confirmation or explicit arming.
- Import/export round-trip is semantically exact.

### Public Cloud-contract release

- Generated clients, schemas, fake services, golden vectors, and portability tests pass.
- No hosted server, billing, tenancy, or production credentials exist in this repository.
- Local-outage, cancellation, export, grant-narrowing, and relay-state invariants pass.

### Hosted Cloud private beta

- Sync p95 <= 2 seconds and remote action p95 <= 500 ms in supported regions.
- Monthly availability >= 99.9%.
- No plaintext local secrets in Cloud data, logs, analytics, or support bundles.
- Independent security review closes all critical/high findings.
- Hosted evidence is linked as opaque release metadata; private source is not copied here.

### Native mobile beta

- Median QR pairing <= 30 seconds.
- LAN acknowledgement p95 <= 120 ms; remote p95 <= 500 ms.
- Wi-Fi reconnect p95 <= 3 seconds; idempotent conformance produces zero duplicate effects.
- Crash-free sessions >= 99.8% and four-hour foreground soak passes.
- VoiceOver/TalkBack, dynamic text, contrast, reduced motion, and target audits pass.

## 9. Hard constraints

- Rust is authoritative for protocol, permissions, persistence rules, execution, pairing, and sync semantics.
- Other languages are limited to documented UI/platform boundaries.
- No Python ships in product paths.
- No autonomous merge, production deployment, store submission, billing activation, signing-key use, or widening of privileged capability.
- Every pushed commit invalidates the previous review gate.
