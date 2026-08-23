# ADR-0002: Host-authoritative execution

Status: proposed  
Date: 2026-08-23

## Decision

Only the paired desktop Engine executes privileged local actions. Studio, browser, Cloud, mobile, and plugins submit bounded typed requests. The Engine revalidates identity, expiry, revision, capability grants, policy, rate, and dedupe state before effects.

Cloud may report `relayed`; only Engine evidence may report execution success.

## Consequences

Local use remains fast and offline. Cloud compromise cannot directly become OS authority. A desktop Engine is required for desktop effects.
