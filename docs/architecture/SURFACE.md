# Live deck surface and action-state feedback (issue #18)

Status: shipped in M1

## What this is

The reliable on-screen control surface over the authored decks: an Edit/Live
mode switch on the Studio window turns the editing chrome off and fills the
main area with deck keys that can be pressed, held, repeated, latched, and
— when a binding ever declares itself destructive — armed and confirmed
before anything fires. Every key renders AUTHORITATIVE action-state feedback:
`relayed`, `accepted`, `running`, and the five terminal states are distinct,
and nothing ever shows success unless the Rust side reported it. No LAN
listener, no native mobile, and — per the PR #75 binding constraint — no OBS
source-visibility/input-mute consent surface anywhere.

## Architecture

Rust owns truth; the WebView owns interaction only.

```
apps/desktop/ui/src/surface/         React + strict TypeScript (Vite)
  types.ts        typed mirror of the surface wire contract
  decode.ts       fail-closed client-side validation of load results and
                  invocation outcomes (defense in depth)
  bridge.ts       SurfaceBridge port: Tauri IPC realization + scripted fake
  machine.ts      pure execution-state machine (phases, arming, hold/repeat)
  keyboard.ts     pure key bindings (Space press/hold, Enter tap, Escape cancel)
  views/surface-view.ts  plain element factories (page tabs, grid, keys,
                  arming strip, live region, alerts) — the same markup the
                  accessibility contract executes
apps/desktop/src-tauri/src/surface.rs  authoritative local boundary:
  `surface_load` (read-only projection + honest engine-availability flag)
  `surface_invoke` (fail-closed interaction evaluation, below)
```

## Execution phases and the honesty rule

Each deck key carries one phase from the closed set
`idle / pressed / armed / relayed / accepted / running / succeeded / failed /
cancelled / expired / outcome_unknown`. The machine enforces:

- `relayed` is transport evidence only (OSCP_MESSAGES.md §8): the command
  left toward the Engine. It is reachable ONLY through the explicit
  `relayed` event fired after the IPC hand-off.
- `accepted` and `running` are reachable ONLY through their own events,
  which today arrive from authoritative evidence paths.
- Terminals are reachable ONLY through the authoritative `terminal` event or
  the reducer's `invoked` settlement of a real response. A refused answer
  lands as `failed` carrying its closed-vocabulary token — never a silent
  reset, never success.
- Toggle latching (`latched`) is control-local presentation, kept separate
  from every execution phase; it never claims an action result.
- Late terminals land even if engine availability dropped mid-flight, so a
  decided outcome can never be swallowed by a disconnect.

## The binding gate (milestone boundary)

No binding vocabulary exists anywhere yet: issue #17 deliberately kept
action configuration out of the op set and nothing since added it, so there
is no graph a key could lawfully dispatch. `surface_invoke` therefore
evaluates every gesture fail-closed — canonical UUIDv7 identity, existence,
enabled flag, state-sink exclusion, and the DOMAIN_MODEL.md §4
event/policy matrix — and then refuses BEFORE any admission, journal write,
or effect with the typed token `binding_absent`. The surface renders that
refusal as FAILURE state. This is the honest end-to-end behavior for this
milestone; when bindings arrive, the same function gains graph resolution
ahead of Engine admission and every other rule here stays as tested.

## Interactions

| Gesture | Pointer path | Keyboard path |
|---|---|---|
| Press / release | pointer down/up on the key | Space down/up |
| Tap | quick click | Enter (complete cycle) |
| Hold window | hold ≥ 500 ms → fires `hold_begin`; release fires `hold_end` | identical, holding Space |
| Repeat | after 500 ms, refires every 150 ms while held | identical |
| Arm destructive | press shows ARMED + confirm strip | Escape cancels; Confirm button fires once confirmed |
| Cancel arming | Cancel button | Escape |

Thresholds are pinned constants (`HOLD_THRESHOLD_MS` = 500,
`REPEAT_INTERVAL_MS` = 150) shared by both paths; the parity suite proves
keyboard sequences produce byte-identical machine-event lists to pointer
sequences, ticks included.

## Destructive arming

Destructive-class bindings require explicit arming at press time
(CAPABILITY_TAXONOMY.md consent column): the key enters ARMED, announces via
`role="alert"`, resolves ONLY through a named Confirm/Cancel group, and
never times out (a11y contract: no timeout-dependent interaction). Because
no binding vocabulary exists yet, production passes an EMPTY destructive
set — no shipped control can arm today — while the machine, view, and tests
prove the full flow so the gate cannot regress before destructive bindings
ever ship.

## Accessibility contract mapping

`DESIGN_SYSTEM.md` requirements are executable in
`src/testing/surface-a11y.test.ts`: it renders the exact shipped composition
(renderStudio in Live mode with renderSurface) across EVERY phase × both
locales and asserts heading order, zero manual tab overrides, textual state
badges beside aria-hidden glyphs (status never color alone), accessible
names ending with the state word, DISTINCT badge vocabulary per phase,
role="status" engine availability, one polite live region fed by every real
phase change, role="alert" failures and arming, named confirm/cancel, grid
geometry stated in words, inert value displays with explanatory text, and
regex-verified rendering of every localized template including
placeholders. Contrast for all reused status tokens on panel surfaces was
already proven by the #5 suite; no new colors were introduced. Focus ring,
44 px targets, 200% zoom, reduced-motion zeroing, and the `.no-motion`
escape hatch remain enforced by the #5 CSS parity tests; the restrained
running progress edge collapses under reduced motion.

## Failure paths

| Failure | Behavior |
|---|---|
| Shell bridge absent (plain browser dev) | Editor load fails honestly as in #17; the surface stays "unavailable" and every interactive event is ignored |
| Load result malformed | Client decoder refuses before render; availability stays false |
| Invocation refused (any token) | Key lands FAILED with the token; sentence rendered via `role="alert"`; next press starts fresh |
| Transport error (bridge rejects) | Same failed path with the typed error token — never success |
| Hold/repeat answered with refusal | Repeating stops at the decided terminal; nothing keeps firing against a dead end |

## Security, privacy, permissions

Risk high (issue-declared). Effective authority stays inside explicit
grants: two new application-defined commands (`surface_load`,
`surface_invoke`) join the four #17 commands; capabilities still grant ZERO
plugin/core permissions; no network surface exists; the invocation path
cannot dispatch any effect this milestone and writes nothing durable.
Refusal tokens carry structural reasons only — labels never enter evidence.

## Compatibility and rollback

Additive UI module + additive commands over the existing schema v2 store;
no migration, no new artifacts, older builds unaffected. Rollback: revert
the PR commits; `surface.rs` and the `src/surface/` tree disappear cleanly
and the capability description returns to its prior wording.
