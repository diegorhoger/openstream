# ADR-0003: OSCP Protobuf over TLS WebSocket

Status: proposed  
Date: 2026-08-23

## Decision

OSCP uses canonical Protobuf schemas and binary frames over TLS WebSocket with explicit version negotiation, message IDs, sequence, correlation, expiry, dedupe, snapshot/patch recovery, and execution states.

## Alternatives

JSON is simpler to inspect but larger and easier to drift across clients. QUIC is attractive but adds platform and operational complexity before measured need. Tauri invoke is private UI plumbing and not a public protocol.

## Consequences

Generated code and golden fixtures are mandatory. Transport remains broadly available across desktop, web, mobile, and gateways.
