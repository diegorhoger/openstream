# Design tokens and the executable accessibility contract

Status: implemented in M0 (issue #5). Authority:
`docs/design/DESIGN_SYSTEM.md`. This document records how that authority is
made code-consumable and testable, what each automated check enforces, the
versioning/compatibility posture, and the exact evidence commands. Where
wording appears to conflict, DESIGN_SYSTEM.md governs.

## Source of truth and layering

```
docs/design/DESIGN_SYSTEM.md        (authority, human-readable)
  └─ apps/desktop/ui/src/design/tokens.ts   (typed source of truth, versioned)
       ├─ apps/desktop/ui/src/styles/tokens.css  (CSS custom properties)
       │    └─ src/styles/global.css             (shell styles consume vars)
       ├─ src/testing/css-parity.test.ts         (CSS ⇄ TS lockstep)
       ├─ src/testing/contrast.test.ts           (WCAG minimums)
       ├─ src/testing/tokens.test.ts             (completeness + vocabulary)
       └─ src/testing/shell-a11y.test.ts         (keyboard/SR criteria)
```

`tokens.ts` exports `TOKENS_VERSION`; `tokens.css` mirrors it as custom
properties. Neither may drift: `css-parity.test.ts` compares every value.

## Token inventory (version 1.0.0)

| Vocabulary | TS export | CSS custom properties |
|---|---|---|
| Color (dark/light): `canvas`, `surface-1`, `surface-2`, `border`, `text`, `text-muted`, `signal`, `live`, `warning`, `info` | `THEMES.dark`, `THEMES.light`, `COLOR_TOKEN_NAMES` | `--canvas` … on `:root` (dark default) and `[data-theme='light']` |
| Typography scale 12–40 px, body 16 px, compact floor 12 px, font stacks | `TYPOGRAPHY_SCALE_PX`, `BODY_FONT_SIZE_PX`, `MIN_TEXT_SIZE_PX`, `UI_FONT_STACK`, `MONO_FONT_STACK` | `--font-size-*`, `--font-ui`, `--font-mono` |
| Spacing base 4 px / rhythm 8 px / steps | `SPACING_BASE_PX`, `SPACING_RHYTHM_PX`, `SPACING_STEPS_PX` | `--space-base`, `--space-rhythm`, `--space-*` |
| Radii 6/10/14 px | `RADII_PX` | `--radius-control`, `--radius-panel`, `--radius-deck-key` |
| Focus ring 3 px signal, 2 px separation | `FOCUS_RING` | `--focus-ring-width`, `--focus-ring-separation` |
| Motion budgets (80–160 ms direct manipulation, 200 ms panels) | `MOTION` | `--motion-direct-manipulation-max`, `--motion-panel-transition` |
| Touch targets (44 px pointer minimum, 88×88 deck key) | `TARGETS` | `--size-target-min`, `--size-deck-key-width/height` |
| Control states (11) + delivery states (`relayed`/`accepted`/`executed`) | `CONTROL_STATES`, `DELIVERY_STATES`, `NON_COLOR_INDICATOR_RULE` | n/a (semantic vocabulary) |
| Localization skeleton (en-US, pt-BR) | `src/i18n/catalog.ts` (`LOCALES`, `CATALOG`) | n/a |

## Executable accessibility contract

All checks run via `pnpm --dir apps/desktop/ui test` (Node's built-in test
runner over TypeScript; no new runtime dependency):

1. **Token completeness** — every documented color token exists in both
   themes with verbatim hex values; scales, radii, focus geometry, motion
   budgets, targets, state vocabularies, and catalog key parity are asserted
   literally against DESIGN_SYSTEM.md.
2. **Contrast minimums** — WCAG 2.x math validated against canonical anchors
   (black/white = 21:1, symmetry, monotonicity). Enforced per theme:
   - `text`/`text-muted`: ≥ 4.5:1 AA on every surface;
     primary `text`: ≥ 7:1 AAA everywhere.
   - Status colors: ≥ 4.5:1 on `canvas` and `surface-1` (AA text-capable);
     ≥ 3:1 non-text indicator floor on all surfaces including `surface-2`.
3. **Focus visibility** — `:focus-visible` renders a solid `var(--signal)`
   outline at `var(--focus-ring-width)` with `var(--focus-ring-separation)`
   offset (measured 4.31:1+ against every surface).
4. **Touch targets** — 44 px pointer minimum and 88×88 deck-key tokens exist
   in TS and CSS; controls introduced later must size from them.
5. **Keyboard/screen-reader criteria** — the shell view is rendered through
   `react-dom/server` from the same module the app ships; assertions cover
   document language/title, single h1→h2 heading order, named sections,
   DOM-order reading order, zero manual tabindex, decorative shapes hidden
   via `aria-hidden`, textual status beside any color indicator (never
   color alone), and fully localized rendering for en-US and pt-BR.
6. **Reduced motion** — `prefers-reduced-motion` collapses both motion
   budgets to 0 ms; an explicit `.no-motion` class provides a no-animation
   mode.

The current shell has no interactive controls yet (issue #17 owns editor
UI); the tests pin this baseline so future controls inherit enforced
criteria instead of renegotiating them.

## Measured contrast table (computed by the test suite)

| Pair (both themes unless noted) | Dark | Light | Floor asserted |
|---|---|---|---|
| `text` vs canvas/surface-1/surface-2 | 18.13 / 16.63 / 14.80 | 17.06 / 18.49 / 15.70 | 7:1 (AAA) |
| `text-muted` vs same surfaces | 9.14 / 8.38 / 7.46 | 6.74 / 7.31 / 6.20 | 4.5:1 (AA) |
| status colors vs canvas/surface-1 | 5.94–11.73 | 4.69–6.10 | 4.5:1 (AA) |
| status colors vs surface-2 | 5.28–9.58 | 4.31–5.18 | 3:1 non-text |
| focus ring (`signal`) vs any surface | 9.58–11.73 | 4.31–5.08 | 3:1 non-text |

Known boundary (documented, not waived): light-theme `signal` (4.31:1) and
`info` (4.41:1) pass the non-text indicator floor on `surface-2` but not
AA text. Status rendering on raised surfaces is therefore indicator usage;
if status *text* must ever sit on `surface-2`, the palette needs a reviewed
adjustment (major token version bump), not an exception.

## Versioning and compatibility

- `TOKENS_VERSION` follows semver over the exported shape: additive tokens
  or locales are minor bumps; renames, removals, or value changes that alter
  rendered output are major bumps requiring documentation updates here.
- The catalogs are flat string maps with locale-key parity; adding an RTL
  locale requires no schema change (styles use logical properties only).
- Compatibility notes live in PR bodies; this file must always match
  implementation evidence (issue acceptance criterion).

## Rollback

Revert the branch: tokens are additive UI-package assets consumed only by
the shell stylesheet and tests. No schema, wire format, storage, network,
or capability impact exists; Rust crates are untouched.

## Evidence commands

```sh
pnpm --dir apps/desktop/ui install
pnpm --dir apps/desktop/ui typecheck   # tsc --noEmit, strict
pnpm --dir apps/desktop/ui test        # node --test, 62 accessibility/token checks
pnpm --dir apps/desktop/ui build       # typecheck + vite build -> ui/dist
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Exact-head results are recorded in the implementing pull request; the
independent verifier reproduces them before merge.
