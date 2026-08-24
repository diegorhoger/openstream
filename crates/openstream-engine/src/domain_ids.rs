//! Re-exports of the durable entity identifiers the engine consumes from
//! `openstream-domain` (typed UUIDv7 ids; DOMAIN_MODEL.md §2). The engine
//! never redefines entity identity; protocol-envelope identities live in
//! [`crate::identifiers`].

pub use openstream_domain::ids::ExecutionId;
