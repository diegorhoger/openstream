# OpenStream product requirements

Status: proposed v0.1  
Owner: `@diegorhoger`  
Last updated: 2026-08-23

## 1. Problem

Professional control surfaces are useful but often closed, hardware-bound, account-bound, platform-limited, or intimidating to configure. Creators need a dependable control plane that is useful before manual configuration, works without the internet, explains failures during a live session, and can grow from a free local tool into a synchronized multi-device workspace.

OpenStream is a control surface, not an OBS replacement, video encoder, audio mixer, or arbitrary remote shell.

## 2. Users

- Solo creators who need scenes, sources, audio, media, and shortcuts at hand.
- Professional operators who need deterministic multi-actions, live state, diagnostics, and multiple surfaces.
- Teams who need shared decks, review, history, roles, and fleet management.
- Developers and hardware builders who need an open protocol and connector SDK.
- Users who need keyboard, screen-reader, high-contrast, reduced-motion, or large-target operation.

## 3. The magic loop

1. **Discover** OBS and other supported local capabilities.
2. **Compose** a useful starter deck automatically.
3. **Preview** or dry-run actions before going live.
4. **Perform** with low-latency, two-way state feedback.
5. **Explain** every outcome in a readable execution timeline.
6. **Improve** through versioned edits, templates, and later assisted suggestions.

First-run target: install to a working OBS scene action in under three minutes for at least 80% of first-time test users.

## 4. Stage 1 — Community desktop and LAN

### Must ship

- Windows-first signed desktop application with macOS/Linux-compatible boundaries.
- Local Rust Engine, Tauri Studio, execution journal, and local surface service.
- No account, mandatory telemetry, or internet dependency.
- Desktop deck plus installable LAN browser/PWA surface with secure QR pairing.
- Decks, profiles, pages, folders, variable grids, drag/drop, undo/redo, and smart profile switching.
- Idle, pressed, armed, running, success, failure, disabled, and disconnected states.
- Press, release, hold, repeat, toggle, sequence, delay, condition, timeout, cancellation, and fail-fast policies.
- Deep OBS integration: discovery, scenes, sources, audio mute, streaming, recording, and replay buffer with live feedback.
- Core actions: hotkey, URL/file/app launch, media transport, soundboard, HTTP request, and explicitly permissioned restricted process execution.
- Automatic OBS starter deck.
- Preview/dry-run and a per-invocation execution timeline.
- Atomic local persistence; versioned, validated import/export with secret redaction.
- OS keychain/credential vault integration; no plaintext secrets.
- Capability declarations and user grants for every action and connector.
- Keyboard navigation, screen-reader labels, high contrast, reduced motion, and large touch targets.
- Localization foundation for English and Brazilian Portuguese.

### Should follow during Stage 1

- macOS and Linux parity.
- MIDI, HID, keyboard, and legally clean hardware adapters.
- Dynamic images, timers, counters, variables, and soundboard controls.
- Twitch, YouTube, Discord, Spotify, Home Assistant, and Streamlabs integrations.
- Sandboxed third-party connector runtime, CLI, and community templates.

### Explicit non-goals

- Cloud accounts, billing, team collaboration, or internet relay.
- Native App Store/Play Store apps.
- Video capture/encoding or a full audio-routing engine.
- Unrestricted local or remote shell execution.
- Bundling proprietary plugins or scraping proprietary marketplaces.
- Requiring generative AI for setup.

## 5. Stage 2 — OpenStream Cloud

### Paid coordination features

- Account and device management.
- End-to-end encrypted sync and backups.
- Version history, comparison, restoration, and explicit conflict resolution.
- Web editor using the same versioned model as Studio.
- Encrypted relay that invokes only preconfigured, locally authorized actions.
- Personal and team workspaces with owner, admin, editor, operator, and viewer roles.
- Audit history, comments, approvals, private templates, and managed rollout.
- Billing, regional pricing, consent, export, deletion, retention, and customer portal.
- Marketplace distribution, signing, compatibility validation, and revocation.

### Cloud invariants

- The Engine remains the only authority for OS actions and local connector secrets.
- Cloud never accepts arbitrary shell text for execution.
- Subscription expiry or Cloud outage cannot impair local execution.
- Users can export their decks and history at any time.
- Telemetry and analytics remain explicit opt-in.

## 6. Stage 3 — Native mobile

- Native SwiftUI iOS/iPadOS and Jetpack Compose Android shells over a shared Rust protocol SDK.
- Free, accountless LAN mode with unlimited controls.
- QR pairing, discovery, certificate pinning, device identity, revocation, and manual fallback.
- Cached decks, instant startup, reconnect, deduplicated invocation, and visible latency/connection health.
- Portrait/landscape grids, one-button mode, tablet multi-panel layouts, haptics, long press, repeat, sliders, and knobs.
- VoiceOver/TalkBack, dynamic type, high contrast, safe areas, and reduced motion.
- Optional Cloud relay for subscribers.

Mobile initially does not run desktop plugins, encode a stream, author arbitrary remote commands, or require Cloud for LAN use.

## 7. Open-core boundary and pricing hypothesis

| Plan | Price hypothesis | Boundary |
|---|---:|---|
| Community | Free forever | Complete local Engine/Studio/PWA, unlimited decks/workflows/LAN devices, built-ins, SDK, import/export, templates |
| Creator Cloud | US$4.99/mo or US$39.99/yr | E2EE sync, backups, 90-day history, remote relay, hosted assistance, five Engines |
| Team | US$10/user/mo annual or US$12 monthly | Shared workspaces, roles, approvals, audit, private templates, one-year history |
| Enterprise | Custom | SSO/SCIM, fleet policy, private registry, regional hosting, SLA, support |

Pricing is a hypothesis until beta evidence. Regional purchasing-power pricing, education/nonprofit discounts, and a 30-day Cloud trial are required considerations. There is no lifetime hosted-service promise.

## 8. Success gates

### Local alpha

- 80% complete install-to-working-OBS-action in under three minutes without assistance.
- LAN/local acknowledgement: p50 <= 50 ms and p95 <= 120 ms on the reference matrix.
- 99.99% correct delivery across 100,000 automated presses with zero duplicate executions.
- Reconnect p95 <= 2 seconds; four-hour soak without lost/duplicate actions or unbounded memory growth.
- Crash-free sessions >= 99.5%.
- Entire offline suite passes with outbound network blocked.
- Every destructive action requires confirmation or explicit arming.
- Import/export round-trip is semantically exact.

### Cloud private beta

- Sync p95 <= 2 seconds and remote action p95 <= 500 ms in supported regions.
- Monthly availability >= 99.9%.
- No plaintext local secrets in Cloud data, logs, analytics, or support bundles.
- Independent security review closes all critical/high findings.
- 100 weekly active Cloud users, 10 active teams, and 40% four-week retention before broader release.

### Native mobile beta

- Median QR pairing <= 30 seconds.
- LAN acknowledgement p95 <= 120 ms; remote p95 <= 500 ms.
- Wi-Fi reconnect p95 <= 3 seconds with zero duplicate actions.
- Crash-free sessions >= 99.8% and four-hour foreground soak on the reference matrix.
- VoiceOver/TalkBack, dynamic text, contrast, reduced motion, and touch-target audits pass.

## 9. Hard product constraints

- Rust is authoritative for protocol, permissions, persistence rules, execution, pairing, and sync semantics.
- Another language is used only for a platform UI or integration boundary where it materially improves delivery.
- No Python ships in the product.
- No autonomous merge, production deployment, store submission, billing activation, signing-key use, or widening of privileged capabilities.
- Every pushed commit invalidates the previous review gate.
