# OpenStream domain model v1

Status: specified contract (M0)  
Authority: implements the typed domain layer required by issue #3 under `docs/adr/0005-versioned-domain-model-action-protocol.md`. Constrains and extends `docs/architecture/TECHNICAL_SPEC.md` §4–§6; the permission vocabulary it references is defined in `docs/security/CAPABILITY_TAXONOMY.md`. Where sources appear to disagree, the stricter rule plus an ADR govern.

## 1. Versioning scheme

Every durable or portable document (deck, profile, page, export file, snapshot payload) carries an explicit `schema_version` of the form `major.minor`.

| Change class | Version effect | Rule |
|---|---|---|
| New optional field, new enum member explicitly marked forward-compatible, new optional message body | minor + | Allowed within a major; readers ignore unknown forward-compatible members |
| Unknown field/member **not** marked forward-compatible | none | Fail closed at validation; never guess semantics |
| Field removal, type change, semantic reinterpretation, constraint tightening that rejects previously valid documents | major | Requires a new ADR, migration tests, dual-read window, and documented rollback |
| Identifier reuse for any entity | forbidden | Deprecations keep their records; IDs are never recycled |

The domain schema major evolves independently of the OSCP wire protocol major but follows identical additive-minor discipline (`PROTOCOL.md`, Compatibility).

## 2. Identifiers

- All durable entity identifiers are UUIDv7, canonical lowercase hyphenated form (`TECHNICAL_SPEC.md` §4). Time ordering is a diagnostic aid only; correctness never depends on it.
- Assets are identified by content address `sha256:<hex>`; the asset row's UUIDv7 id is bookkeeping only.
- `SecretRef` values are opaque vault references scoped to OS credential storage. They never contain secret bytes and are never serialized into portable files, sync payloads, logs, or fixtures.
- Generation authority: the Rust Engine/core mints durable entity IDs. Other surfaces copy them verbatim and never invent durable entity identifiers. Per-message identifiers (`session_id`, `message_id`) are minted by the sending peer per session.
- Stability: an identifier is immutable for the lifetime of its entity and forever after deletion; soft deletion keeps the row and its ID; tombstones dominate older updates in sync (`TECHNICAL_SPEC.md` §6). Sync merge ties break by actor ID after hybrid logical clock comparison.

## 3. Core entities

Exactly these twenty entities exist (`TECHNICAL_SPEC.md` §4); adding one requires a domain-minor change with migration.

| Entity | Key typed fields | Notes |
|---|---|---|
| Installation | `installation_id` UUIDv7, engine instance UUID, created_at, platform profile | Provisioned once per desktop install; instance UUID anchors pairing prologues |
| TrustedPeer | `peer_id` UUIDv7, device identity reference, display label, granted OSCP scopes, paired_at, revoked_at | Created by pairing; revocation deletes trust and terminates live sessions; record retained for audit |
| Workspace | `workspace_id` UUIDv7, title, HLC cursor, deleted_at | Local-first container; sync optional and E2EE |
| Deck | `deck_id` UUIDv7, workspace ref, title, `revision` uint64 monotonic, folder path, pages, deleted_at | Folders are a path attribute on Deck, not an entity; structural edits bump `revision` |
| Profile | `profile_id` UUIDv7, workspace ref, name, ordered deck list | A switchable named arrangement ("profile switching", PRD Stage 1) |
| Page | `page_id` UUIDv7, deck ref, ordinal, grid dimensions, controls | Grid geometry is page-relative |
| Control | `control_id` UUIDv7, page ref, kind, geometry, label, interaction policy, bindings, enabled flag | Labels are user data; redacted from logs/evidence per taxonomy rules |
| ActionBinding | `binding_id` UUIDv7, control ref, trigger, graph revision ref, capability narrowing set, arming requirement, enabled flag | Binds one trigger to one immutable graph revision; narrowing never exceeds the manifest request |
| GraphNode | `node_id` UUIDv7, graph ref, kind, typed params, deadline override ≤ macro cap | Kinds listed in §5 |
| GraphEdge | `edge_id` UUIDv7, graph ref, from/to node, edge kind | Edge kinds: sequence order, branch condition, compensation link |
| Variable | `variable_id` UUIDv7, scope (workspace/deck/page), name, type ∈ {string, number, boolean, datetime}, value, HLC stamp | Rendered via variable-display controls; secrets can never enter variable storage because secret bytes never leave the broker |
| SecretRef | opaque reference token | Vault-backed; see §2 |
| Asset | sha256 content address, size, chunk plan | Bytes verified by SHA-256 before use; transport chunks carry offsets and final hash |
| PluginInstall | `install_id` UUIDv7, plugin reverse-DNS id, semver, component hash, requested capability set, enabled | Stores *requests*, never grants; grants live in user-consent records owned by the Engine |
| IntegrationConnection | `connection_id` UUIDv7, connector type, credential reference(s), handle issuance log | Plugins receive opaque handles only; raw secrets resolve solely inside the integration broker |
| Operation | `operation_id` UUIDv7, versioned op payload, HLC, actor id, applied state | Sync outbox entry; operational presses are never operations |
| Execution | `execution_id` UUIDv7, origin refs, pinned graph revision, state ∈ seven authoritative states, prepared/result record refs, timestamps, typed failure reason | The execution journal; see `OSCP_MESSAGES.md` §8 |
| AuditEvent | `event_id` UUIDv7, subject, redacted payload, timestamp | Grant create/narrow/revoke plus terminal execution evidence |
| SyncCursor | device id + workspace id, last applied HLC/op ordinal | Merge progress marker |
| DedupeEntry | (`source_device_id`, `message_id`) composite key, first_seen, execution ref, outcome state | Durable admission dedupe; retention bounds fixed in `OSCP_MESSAGES.md` §7 |

## 4. Controls

Control kinds v1 (additive enum; additions are domain-minor when marked forward-compatible):

| Kind | Purpose | Interaction events |
|---|---|---|
| `button` | Momentary action surface | press, release, hold_begin, hold_end, long_press, repeat |
| `toggle` | Latched action surface | press (toggles latched state), release |
| `page_jump` | Navigate to a target page | press |
| `variable_display` | Render one variable value | none (state sink) |

- Interaction policies derive from PRD Stage 1 semantics: press, release, hold, repeat, toggle, with timeout, cancellation, and fail-fast handled inside the graph (§5), not by the control.
- Visual control states are typed and exhaustive: `idle`, `pressed`, `armed`, `running`, `success`, `failure`, `disabled`, `disconnected` (PRD Stage 1 must-ship). States are derived from Engine journal evidence only — no success before an authoritative Engine result (`SECURITY.md` hard rule).
- Destructive-class bindings require explicit arming or confirmation at press time before the trigger admits (`CAPABILITY_TAXONOMY.md` consent column).
- Accessibility contract: every visual state has a non-color signal (text/icon/shape) and a screen-reader-legible name/state; color alone never carries state.

## 5. Action graphs

- Graphs are immutable validated DAGs; each run reads exactly one immutable graph revision (`TECHNICAL_SPEC.md` §5). Editing produces a new revision; published revisions never mutate.
- Node kinds v1: `action`, `sequence`, `parallel`, `delay`, `conditional`, `retry`, `variable_transform`, `compensate` (explicit compensating action).
- Limits: maximum 128 nodes; nesting depth 16; default deadline 30 seconds per invocation; macro maximum 10 minutes; concurrency four per plugin, 32 global; cancellation propagates.
- Failure policy per graph: `stop`, `continue`, or `compensate`. Compensation is valid only where the adapter declares safe compensation; `retry` nodes require adapter-declared idempotency or explicit reconciliation.
- Edges are typed (§3); cycles fail validation unconditionally.

## 6. Validation pipeline

Fail closed at every stage; a failed document applies nothing partially.

| Stage | When | Checks | Failure surface |
|---|---|---|---|
| S1 Syntax/schema | save + admission + pre-execution | Document decodes against its declared `schema_version`; unknown non-forward-compatible members reject | typed validation error |
| S2 Structural | save-time and pre-execution | DAG acyclicity; node/depth/deadline limits; dangling edge endpoints absent | typed validation error |
| S3 Referential | save-time and pre-execution | Capability identifiers exist in the registry and match qualifier grammar; handle tokens resolvable; asset hashes present; binding targets exist | fail closed (`CAPABILITY_TAXONOMY.md` §1) |
| S4 Semantic | save-time and pre-execution | Narrowing monotonicity (binding qualifiers ⊆ manifest request ⊆ grant); destructive class carries arming requirement; compensation/retry preconditions hold | typed validation error |
| S5 Runtime revalidation | immediately before every side effect | Full taxonomy §2 intersection: platform capability ∩ manifest ∩ grant ∩ instance narrowing ∩ workspace policy ∩ runtime context (session validity, expiry, rate, graph revision, dedupe state) | denial journaled as `failed` with typed reason |

Save-time runs S1–S4; nothing failing S1–S4 persists as enabled. Execution-time re-runs S1–S4 cheaply from persisted validated form, then S5 authoritatively.

## 7. Persistence and migration

- SQLite WAL with explicit ordered forward-only migrations under `migrations/sqlite/`; destructive migration is a hard stop (`AGENTS.md`).
- Timestamps UTC; deadlines measured on monotonic runtime clocks after admission (`TECHNICAL_SPEC.md` §4).
- Soft deletion via `deleted_at`; tombstones dominate older updates; grid collisions preserve both edits and mark `needs_resolution`; invalid merged graphs remain stored but disabled.
- Operational button presses never become sync operations and never execute after expiry.

## 8. Portability (import/export)

- Export documents carry their own `schema_version`, a minimum-required-engine range, content hashes, and capability *requests* — never grants (`CAPABILITY_TAXONOMY.md` §7).
- Imports referencing capabilities without local grants arrive disabled and surface a typed denial until consented.
- Import whose hash/signature checks fail restores nothing.
- Round-trip import/export must be semantically exact (PRD success gate); fixtures cover this (see `OSCP_MESSAGES.md` §11).

## 9. Compatibility summary

| Change | Class | Migration requirement |
|---|---|---|
| Add optional field / forward-compatible enum member / new control kind | minor | Fixture vectors added; changelog entry |
| Add entity to the twenty-entity list | minor+ | Storage migration; sync op version bump |
| Remove/retype field, reinterpret meaning, tighten validity | major | ADR, migration tests, dual-read window, rollback proof, human gate |

## 10. Status honesty statement

At base commit `554e0f97fcfd29c703b7e5fe5eb040088ec2f784` this repository contains no runtime code; every rule above is *specified* contract that M1/M2 issues implement and verify. This section must be updated, not deleted, as controls become enforced.

## References

- `docs/architecture/OSCP_MESSAGES.md` — typed envelopes, errors, states, recovery, fixtures
- `docs/architecture/TECHNICAL_SPEC.md` — authority boundary, engine limits, sync semantics
- `docs/security/CAPABILITY_TAXONOMY.md` — permission vocabulary and lifecycle
- `docs/product/PRD.md` — product behavior gates referenced above
- `docs/adr/0005-versioned-domain-model-action-protocol.md` — decision record
