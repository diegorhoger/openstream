# ADR-0006: Profile switching mechanisms and capability vocabulary

Status: proposed  
Date: 2026-08-25  
Decision owners: diegorhoger (repository owner; independent review per REVIEW_GATES.md)

## Context

Issue #19 requires predictable deck switching by explicit matcher and
shortcut, with grants explicit per mechanism and deterministic
priority/conflict rules. Two mechanisms imply two OS-level authorities that
the closed v1 capability vocabulary cannot express today:

1. **Global shortcuts** — the OS must be told (registration) which key
   combinations belong to OpenStream so it delivers them while any other
   application has focus.
2. **Focused-app matching** — switching on "when OBS has focus" needs the
   identity of the application that currently holds keyboard focus.

`docs/security/CAPABILITY_TAXONOMY.md` §7 reserves additive-minor additions
inside an existing reserved domain (`os.*`) but routes every capability
addition through a security ADR. This document is that ADR.

## Decision

Add exactly two unqualified capabilities to the reserved `os.*` domain:

| Capability | Authority conferred | Consent |
|---|---|---|
| `os.hotkey.register` | Register OS-global keyboard shortcuts for profile switching. Registration-based delivery only: the OS notifies us for combos WE registered. No hooks, no keystroke reading, no input streams. | First-use confirmation recorded as `ConsentEvidence` |
| `os.focus.read` | Observe ONLY which application identity (lowercased process image file name) holds keyboard focus. Window titles, content, and anything beyond the image name are out of scope by construction. | First-use confirmation recorded as `ConsentEvidence` |

Both are unqualified kinds (mechanism-level authority); no qualifier
grammar is introduced, so `covers()`/narrowing semantics stay trivially
exact-match and fail-closed rules from issue #8 apply unchanged.

## Alternatives considered

- **Reuse `os.keyboard.emit`** — rejected: that row authorizes SYNTHESIZING
  input, the exact inverse of these authorities. Overloading it would make
  a high-risk grant look like consent for reading.
- **No new vocabulary; treat switch rules as plain configuration** —
  rejected: the issue demands grants explicit PER MECHANISM with typed
  revocation semantics; without ledger-backed authority there is nothing to
  revoke immediately and no audit evidence trail.
- **Qualified scopes per combo / per app** — deferred: mechanism-wide
  grants keep v1 honest and small; narrowing can be added later as another
  additive minor if operators need per-trigger revocation.

## Consequences

- The taxonomy gains two §5 rows whose Status column names this milestone;
  enforcement ships in the same PR (no contract-without-runtime gap).
- Consent records live in the in-memory shell ledger this milestone: after
  a restart both mechanisms start DENIED until re-granted. Conservative,
  documented, and revisited when durable grant storage lands.

## Security and privacy impact

- Deny-by-default preserved: empty ledger = zero switching authority.
- No input capture anywhere: hotkeys use registration-based delivery
  (`RegisterHotKey`-class inside pinned wrappers); focus reads only the
  foreground process image file name. This is the same observation class
  task bars expose, never keystrokes or titles.
- Revocation deletes the grant record, appends audit evidence, tears down
  live registrations/polling in the same call, and denies at the very next
  evaluation (taxonomy §3).
- Effective authority stays intersection-only (manifest ∩ user grant);
  recomputed before every evaluation, never cached.

## Compatibility and migration

Pure additive minor: two new enum members of an existing reserved domain,
no qualifier changes, no removals. Older builds reject the new identifiers
fail closed at parse time, which is the specified behavior for unknown
capabilities. No schema migration is required; switch rules ride profiles
as an optional `serde(default)` field (additive minor per DOMAIN_MODEL.md
§1), so pre-#19 documents decode unchanged.

## Reversal

Deprecate the rows (kept with strikethrough per registry integrity rules),
stop issuing grants, and remove the surfaces. Registrations die with the
process; there is no persisted state to migrate away.

## Evidence

- Enforcement + tests: PR for issue #19 (`crates/openstream-domain/src/switching.rs`, `apps/desktop/src-tauri/src/{hotkeys,focus,switching}.rs`).
- Windows-gated real-backend tests prove registration/conflict/unregister round-trips and typed focus identities against the live OS.
- Fakes prove conflicts, revocation immediacy, lifecycle convergence, and visible degradation deterministically on every platform CI runs.
