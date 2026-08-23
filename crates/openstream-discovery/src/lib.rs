//! `openstream-discovery` — mDNS adapter and fixtures.
//!
//! Advertises and discovers Engine endpoints on the local network. Mandatory
//! control (SECURITY.md): no secrets in mDNS records, and the listener stays
//! disabled until the user grants native-LAN consent.
//!
//! Status: M0 boundary skeleton. The adapter, fixtures, and consent gating
//! arrive with the LAN milestones.
