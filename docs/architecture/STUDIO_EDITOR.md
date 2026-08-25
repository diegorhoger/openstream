# Studio deck editor (issue #17)

Status: shipped in M1

## What this is

The accessible desktop authoring experience: a visual deck editor over the
versioned domain model (`DOMAIN_MODEL.md` v1) with full keyboard parity for
every drag operation, an inspector driven by domain validation, undo/redo,
atomic autosave through the local persistence pipeline (#15), zoom, and
en-US/pt-BR localization from one resource catalog. No Cloud collaboration,
no generative builder, and — per the PR #75 binding constraint — no OBS
source-visibility/input-mute consent surface anywhere: action *configuration*
does not exist in this milestone's vocabulary at all.

## Architecture

Rust owns structural truth; the WebView owns interaction only.

```
apps/desktop/ui/src/studio/          React + strict TypeScript (Vite)
  types.ts        typed mirror of the serde wire contract (documents + ops)
  decode.ts       fail-closed client-side validation mirror (defense in depth)
  bridge.ts       StudioBridge port: Tauri IPC realization + test fake
  editor.ts       pure view reducer (selection, zoom, lift, announcements)
  keyboard.ts     pure key bindings (drag alternatives, shortcuts)
  views/          plain element factories (canvas, panels, inspector, shell)
apps/desktop/src-tauri/src/studio.rs authoritative editing service:
  StudioOp (closed serde-tagged vocabulary) -> apply_op() mutates through
  openstream-domain's typed API, revalidates whole documents fail-closed,
  bumps revisions, then EditorSession persists atomically.
crates/openstream-persistence/src/sqlite/workspace.rs  WorkspaceStore:
  all-or-nothing whole-snapshot transactions over WAL + synchronous=FULL.
migrations/sqlite/0002_workspace_documents.sql         schema v1 -> v2.
```

Every edit is one closed-vocabulary `StudioOp` (`create_deck`,
`move_control`, `reorder_page`, `profile_move_deck`, ...). The service
applies it by value through the domain API and validates the ENTIRE affected
document on every change; a refused op leaves state byte-identical and
returns a typed token that the UI localizes into a `role="alert"` sentence.
Unknown op variants reject at deserialization (deny-by-default).

UUIDv7 identity is minted only in Rust (`ids.rs` generation authority); the
WebView never invents identifiers.

## Undo/redo and autosave

- Snapshot stacks bounded at 100 per direction (`studio::UNDO_LIMIT`);
  accepted ops push history, new work clears redo. Availability flags ride
  every outcome so buttons render truthfully disabled.
- Autosave rewrites the whole workspace as ONE transaction after every
  accepted mutation, undo, or redo. A returning call means durable; a
  refusal surfaces honestly as `saved = false` plus a token
  (`autosave_unavailable` / `autosave_refused`) while editing continues
  in memory — degraded, never silent, never lost silently either.
- On session open the store runs the #15 open pipeline (integrity check,
  forward-only migration with verified backup, typed refusals). Documents
  that fail decode refuse the WHOLE load (fail closed) and the session
  degrades to autosave-off rather than dropping content.

## Schema v2

`workspace_documents(kind CHECK kind IN ('deck','profile'), id,
document_json, updated_at_wall_ms)` with `(kind, id)` primary key. The
migration chain is crate-global: existing journal databases receive the
table unused through the standard verified-backup upgrade path (covered by
the widened upgrade-from-every-released-version harness).

## Keyboard map (drag alternatives are first-class)

| Action | Pointer path | Keyboard path |
|---|---|---|
| Move control | drag key onto grid | Enter/Space lifts, arrows move, Enter drops |
| Resize control | inspector fields | Shift+arrows grow, Alt+Shift+arrows shrink, Enter drops |
| Cancel move | — | Escape |
| Delete control | Delete key / inspector button | same binding, same op |
| Reorder pages | — | Move up/down buttons per page row |
| Deck to folder | folder combobox | same combobox + inspector folder field |
| Profile ordering | up/down/remove buttons | same buttons |
| Undo/redo | toolbar buttons | Ctrl+Z / Ctrl+Y or Ctrl+Shift+Z |
| Zoom | toolbar buttons | Ctrl+= / Ctrl+- ; reset via toolbar |

The parity suite (`src/studio/keyboard-parity.test.ts`) drives each keyboard
sequence through the pure bindings and asserts byte-identical ops against
the pointer/drag path. Lift geometry clamps inside the grid, so keyboard
moves can never propose invalid placements; the domain still re-validates
authoritatively.

## Accessibility contract mapping

`docs/design/DESIGN_SYSTEM.md` requirements are executable:
`apps/desktop/ui/src/testing/shell-a11y.test.ts` renders the exact shipped
markup across a state matrix (ready, loading, failed, save-degraded,
save-refused, selection variants, lifted, collisions, error tokens,
announcements, empty workspace) in both locales and asserts:

- one h1 before any h2; sections labelled by existing ids; DOM order equals
  reading order; zero manual tab-order overrides;
- decorative shapes aria-hidden while textual status sits beside them;
  engine state, autosave status (`role="status"`), and announcements
  (`aria-live="polite"`) are always text;
- accessible names for every button; explicit label/field associations in
  the inspector; grid geometry stated in words;
- control states — disabled, selected, lifted, overlapping — carried by
  TEXT (accessible name suffixes, badges, `aria-describedby`), never color
  alone; refusals announced via `role="alert"`;
- every catalog string reachable somewhere in the matrix (placeholder
  templates matched as patterns), en-US and pt-BR identical key coverage.

Focus visibility (3 px signal ring with canvas separation), reduced-motion
zeroing, and the `.no-motion` escape hatch remain enforced by the #5 CSS
parity tests. Zoom scales cell size from 44 px (50%) to 176 px (200%),
keeping the 88 px desktop key target honest at minimum zoom.

## Failure paths

| Failure | Behavior |
|---|---|
| Workspace store cannot open | Session runs in memory; footer shows autosave-unavailable sentence with token; every outcome reports `saved = false` |
| Stored document fails domain validation | Whole load refuses (no silent drops); degraded in-memory session starts empty |
| Op refused by domain validation | State untouched; typed token localized into the inspector alert line; nothing autosaves because nothing changed |
| Autosave transaction refused | Logged typed reason; outcome carries `autosave_refused`; memory keeps the edit for retry on next successful persist |
| Shell bridge absent (plain browser dev) | Honest load-failed screen naming the missing bridge; no fake data, no fake persistence |

## Security, privacy, permissions

Risk medium (issue-declared). The WebView gains exactly four application-
defined commands (`studio_load/apply/undo/redo`) over validated local
documents; capabilities still grant ZERO plugin/core permissions; no
network surface exists; documents hold user-authored labels/titles only —
no secrets, no execution evidence, no personal data beyond authored text.
Effective authority stays inside explicit grants: the editor cannot touch
grants, secrets, engines, or integrations.

## Compatibility and rollback

- Schema v2 is additive; v1 databases upgrade through the tested verified-
  backup path; older builds refuse newer stores fail closed
  (`SchemaTooNew`).
- Rollback: revert the PR commits. The workspace store file is unknown to
  older builds and simply ignored; deleting `<data dir>/workspace.sqlite3`
  removes authored content (journal evidence and lifecycle artifacts are
  unaffected).
