# Open-core and commercial boundary

## Community promise

The public OpenStream repository contains the local Engine, Studio, LAN browser surface, public protocols, portable formats, connector SDK, reference clients, and built-in connectors. These remain useful without an account or subscription.

The repository is Apache-2.0. That license grants code rights, not rights to the OpenStream name, logo, store identity, hosted service, signing keys, or production infrastructure.

## Hosted product

OpenStream Cloud is an optional commercial service. It sells ongoing coordination and operations: encrypted sync, backups, history, remote relay, collaboration, roles, managed distribution, availability, and support.

The hosted control-plane implementation may live in a separate private repository. The public repository retains:

- wire and storage schemas required for interoperability;
- clients and local Engine behavior;
- portable export/import formats;
- cryptographic protocol descriptions and test vectors;
- a self-contained Community experience.

## Anti-captivity rules

- Cloud outage or cancellation cannot disable local execution.
- Users may export their own configurations and history.
- Cloud cannot widen an Engine capability grant.
- Local connector secrets are not recoverable from Cloud.
- No advertising or mandatory telemetry funds the Community edition.

This boundary is an architectural constraint, not merely packaging text.
