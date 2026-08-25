# Workspace and build

Status: scaffolded in M0 (issue #4). This document records the pinned
workspace layout, the exact validation sequence, and the compatibility and
rollback posture of the scaffold.

## Pinned toolchain

| Tool | Pin | Enforcement |
|---|---|---|
| Rust | stable `1.98.0` | `rust-toolchain.toml` (rustup installs it automatically) |
| Node | `>=24` | `apps/desktop/ui/package.json` `engines` |
| pnpm | `10.x` (built with 10.33.1) | lockfile committed at `apps/desktop/ui/pnpm-lock.yaml` |

Dependency policy: workspace dependencies are pinned to exact versions
(`tauri =2.11.5`, `tauri-build =2.6.3`); npm dependencies use exact versions
with the resolved lockfile committed (`@types/node 24.13.3` is a types-only
devDependency for the UI package's Node-native test runner). Advisory/license
automation (cargo-deny equivalent) arrives in a later milestone; until then
pins stay deliberately minimal.

## Layout versus TECHNICAL_SPEC §3

Crate boundaries exist now as skeletons; implementations arrive with their own
milestones. Naming matches TECHNICAL_SPEC §3 exactly.

| Path | Status |
|---|---|
| `crates/openstream-domain/` | skeleton; carries `DOMAIN_MODEL_MAJOR/MINOR = 1.0` anchor (ADR-0005) |
| `crates/openstream-protocol/` | skeleton; carries OSCP major/minor `1.0` anchor (PROTOCOL.md) |
| `crates/openstream-engine/` | skeleton |
| `crates/openstream-persistence/` | skeleton |
| `crates/openstream-sync/` | skeleton |
| `crates/openstream-crypto/` | skeleton |
| `crates/openstream-discovery/` | skeleton |
| `crates/openstream-pairing/` | skeleton |
| `crates/openstream-plugin-host/` | skeleton |
| `crates/openstream-action-sdk/` | skeleton |
| `crates/openstream-integrations/` | skeleton |
| `crates/openstream-mobile-ffi/` | skeleton |
| `crates/openstream-testkit/` | skeleton |
| `apps/desktop/src-tauri/` | Tauri 2 composition root, deny-by-default capabilities |
| `apps/desktop/ui/` | React + strict TypeScript + Vite shell (TECHNICAL_SPEC §2) |
| `integrations/os-automation/` | keyboard shortcut adapter (issue #10), application/file/URL launch adapters (issue #11), and media transport / master-scope volume adapters (issue #12): Windows real backends, honest Unsupported elsewhere; issue #14 adds the multi-action graph-semantics integration suite (`tests/graph_semantics_integration.rs`) driving sequence/delay/conditional/policy/deadline/cancellation semantics through these registered actions |
| `integrations/os-obs/` | OBS WebSocket v5 integration adapters (issue #13): discovery with typed version compatibility, vault-only connection secrets, eight registered actions across the `obs.*` capability rows, destructive arm/confirm gating, bounded-backoff reconnect, event-driven live state, and a deterministic fake OBS server for CI contract tests; issue #14 adds the multi-action graph-semantics integration suite (`tests/graph_semantics_integration.rs`) over the registered scene/replay/stream actions |

Remaining TECHNICAL_SPEC §3 directories (`apps/ios`, `apps/android`,
`integrations/`, `proto/`, `wit/`, `packages/`, `simulators/`,
`migrations/sqlite/`) are intentionally absent until their owning milestones;
creating them empty would add no boundary value.

## Boundary rules enforced by the scaffold

- `unsafe` code is forbidden workspace-wide via `[workspace.lints.rust]`
  (`unsafe_code = "forbid"`), inherited by every crate including the desktop
  composition root.
- Public items require documentation (`missing_docs = "warn"`, denied under
  `-D warnings`).
- `openstream-domain` stays dependency-free; it imports no UI, database,
  network, Tauri, or Cloud implementation.
- The WebView capability `apps/desktop/src-tauri/capabilities/main.json`
  grants zero IPC permissions and no Tauri commands are registered, so the
  shell has an empty invokable surface. A strict CSP
  (`default-src 'self'`, self-only scripts/styles/images) applies to the
  window. Any future permission addition requires explicit security review;
  widening never happens silently.
- No Python exists anywhere in shipped paths (governance gate also enforces
  this).

## Build and validation

Run from the repository root in this order (the UI build must precede any
Cargo command because the Tauri context embeds `apps/desktop/ui/dist`):

```sh
pnpm --dir apps/desktop/ui install
pnpm --dir apps/desktop/ui typecheck   # tsc --noEmit, strict
pnpm --dir apps/desktop/ui test        # node --test over TS (design-token + a11y contract)
pnpm --dir apps/desktop/ui build       # typecheck + vite build -> ui/dist
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

A clean clone passes all seven commands; `cargo test` currently reports the two
contract-anchor unit tests (domain model version, OSCP version) plus zero-test
skeleton crates, which is expected for M0; the UI test script runs the design
token and accessibility contract (see docs/design/DESIGN_TOKENS.md).

## Compatibility and rollback

The scaffold adds no schema, wire format, storage, or network surface. The
only contract data introduced is the two version-anchor constant pairs, which
match already-merged documentation (ADR-0005, PROTOCOL.md). Rollback is a full
branch revert; nothing consumes the scaffold yet. Future milestones must not
weaken the lints, the capability allowlist, or the CSP without a security ADR
and human gate.
