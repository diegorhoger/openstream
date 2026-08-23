# ADR-0004: Public Community products and private hosted Cloud

Status: accepted decision A1  
Date: 2026-08-23

## Decision

The local Engine, Studio, public protocols, portable formats, generated clients, simulators, conformance tests, plugin SDK, and native mobile clients are Apache-2.0 in this repository.

Hosted Cloud server, tenancy, billing, operations, private marketplace, deployment configuration, and customer-data implementation live in a separate private repository. This public repository keeps Cloud contracts, clients, fake services, cryptographic vectors, portability tests, and tracking/meta issues.

## Consequences

Community desktop use is complete rather than trialware. Revenue comes from hosted coordination, reliability, collaboration, history, relay, managed distribution, and support. Public roadmap issues cannot import or request private hosted source. Cross-repository readiness is recorded only as opaque versioned evidence.

## Reversal

Any source-boundary or licensing change requires legal review, a new ADR, compatibility/export analysis, and a human hard stop.
