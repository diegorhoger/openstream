# OpenStream

**The open control surface for live production.**

OpenStream is a local-first, vendor-neutral Stream Deck alternative. It starts as a complete desktop application, adds an optional hosted web control plane, and later adds native iOS/iPadOS and Android control surfaces.

The first product promise is simple:

> Install OpenStream, detect OBS, receive a useful starter deck, preview it, and press a button — without creating an account or using the internet.

## Product stages

- **Stage 1 — Community desktop:** the local Rust Engine and Tauri Studio provide editing, an on-screen deck, OBS integration, actions, portability, and evidence. No account or network surface is required.
- **Stage 2 — OpenStream Cloud:** the subscription web application provides encrypted sync, history, collaboration, web editing, and remote relay. The hosted implementation is maintained in a separate private repository.
- **Stage 3 — Native mobile:** public SwiftUI and Jetpack Compose clients provide free, accountless secure LAN control and optional Cloud relay.

## Public repository boundary

This Apache-2.0 repository contains the Community Engine and Studio, public OSCP protocol and schemas, native mobile clients, portable formats, generated clients, conformance simulators and tests, and Cloud interoperability contracts. It does not contain hosted Cloud server, billing, tenancy, production operations, or private marketplace implementation.

The paired local Engine remains the sole authority for privileged desktop actions. Cloud coordination cannot widen a local grant, and Cloud outage or subscription expiry cannot impair local execution.

## Status

OpenStream is at **M0 Foundation**. Product, architecture, security, design, and engineering governance are under exact-head review. Work proceeds from the oldest unblocked issue and stops at documented human gates.

## Principles

1. Local forever: the desktop product is useful, offline, accountless, and not trialware.
2. Safe live operation: preview, arming, deadlines, bounded retries, and readable execution evidence.
3. Open and portable: public schemas, clients, test vectors, import/export, and no profile lock-in.
4. Rust owns authority: protocol, state, permissions, execution, pairing, and sync semantics.
5. Cloud adds coordination, never captivity.
6. Native mobile owns direct LAN control; browsers reach Engines only through Cloud.
7. Accessible and global from the first release.

Read the [PRD](docs/product/PRD.md), [roadmap](docs/product/ROADMAP.md), [technical specification](docs/architecture/TECHNICAL_SPEC.md), [domain model](docs/architecture/DOMAIN_MODEL.md), [OSCP message contract](docs/architecture/OSCP_MESSAGES.md), [security model](docs/architecture/SECURITY.md), [threat model and capability taxonomy](docs/security/README.md), and [agent graph](docs/engineering/AGENT_GRAPH.md).

## License

Public source in this repository is licensed under Apache-2.0. The license does not grant rights to the OpenStream name, marks, hosted service, store identity, signing keys, or production infrastructure.
