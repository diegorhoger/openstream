# Public Community and private hosted Cloud boundary

## Accepted decision A1

The public Apache-2.0 repository contains:

- local Rust Engine and Tauri Studio;
- public OSCP schemas, codecs, generated clients, golden fixtures, simulators, and conformance tests;
- portable import/export formats and compatibility tests;
- public native iOS/iPadOS and Android clients and the narrow Rust mobile SDK;
- built-in Community integrations and the later public plugin SDK;
- public Cloud contracts, client libraries, fake services, portability tests, and tracking/meta issues.

It does **not** contain hosted Cloud server, tenant implementation, billing implementation, production operations, private registry/marketplace, secrets, deployment configuration, or customer data logic. Those belong to a separate private repository.

## Hosted product

OpenStream Cloud is an optional commercial service for encrypted sync, backup, history, web editing, relay, collaboration, roles, managed distribution, availability, and support. Public issue completion can validate public contracts and record opaque private-release evidence, but never imports private source.

## Anti-captivity rules

- Cloud outage or cancellation cannot disable local desktop execution.
- Users may export their own configurations and applicable history.
- Cloud cannot widen an Engine capability grant.
- Local connector secrets are not recoverable from Cloud.
- Protocol compatibility and portability are independently testable without private source.
- No advertising or mandatory telemetry funds the Community edition.

Apache-2.0 grants code rights, not rights to the OpenStream name, logo, hosted service, signing keys, store identity, or production infrastructure.
