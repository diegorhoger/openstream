# OpenStream

**The open control surface for live production.**

OpenStream is a local-first, vendor-neutral Stream Deck alternative. It turns a desktop window, browser, phone, tablet, keyboard, MIDI device, or compatible physical controller into one coherent control surface for OBS and creative workflows.

The first product promise is simple:

> Install OpenStream, detect OBS, receive a useful starter deck, preview it, and press a button — without creating an account.

## Product layers

- **OpenStream Engine** — the local Rust authority that discovers integrations, stores secrets, evaluates permissions, executes actions, and records an explainable timeline.
- **OpenStream Studio** — the desktop editor and on-screen deck built with Tauri.
- **OpenStream Surface** — the versioned local protocol used by browser, mobile, hardware, and accessibility clients.
- **OpenStream Cloud** — the optional paid control plane for encrypted sync, history, collaboration, remote relay, and managed distribution.
- **OpenStream Mobile** — native iOS/iPadOS and Android control surfaces. LAN mode remains free and accountless.

Cloud never executes privileged desktop actions itself. The paired local Engine remains the authority and local execution continues when Cloud is unavailable or a subscription ends.

## Status

OpenStream is at **M0 Foundation**. The product contract, architecture, security boundaries, design system, and autonomous engineering graph are under review. Development proceeds from the oldest unblocked issue and stops at exact-head human review gates.

## Principles

1. Local forever: unlimited local decks, actions, workflows, profiles, and LAN surfaces.
2. Safe live operation: preview, arming, timeouts, idempotency, and readable execution evidence.
3. Open and portable: public schemas, SDKs, import/export, and no profile lock-in.
4. Rust owns authority: protocols, state, permissions, execution, pairing, and sync semantics.
5. Cloud adds coordination, never captivity.
6. Accessible and global from the first release.

Read the [PRD](docs/product/PRD.md), [roadmap](docs/product/ROADMAP.md), [technical specification](docs/architecture/TECHNICAL_SPEC.md), [security model](docs/architecture/SECURITY.md), and [agent graph](docs/engineering/AGENT_GRAPH.md).

## License

The community repository is licensed under Apache-2.0. The OpenStream name and marks are not granted by that license. The hosted Cloud implementation is a separate commercial product; public protocols, clients, and portability formats remain open.
