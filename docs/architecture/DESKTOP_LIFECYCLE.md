# Desktop shell lifecycle

Status: shipped in M1 (issue #16). Composes the merged Engine (#9, #14), adapters (#10–#13), and SQLite persistence (#15) into a dependable background desktop application.

## What the shell composes

| Piece | Realization |
|---|---|
| Engine time port | `SystemClock` (`apps/desktop/src-tauri/src/clock.rs`) — the real monotonic/wall source the engine crate explicitly deferred to this composition root |
| Durable evidence | `ActionRuntime` over `SqliteJournal` (issue #15 open pipeline: WAL, `synchronous=FULL`, integrity verification, forward-only migrations) |
| Crash recovery | On every restart, prepared-without-terminal records close as `outcome_unknown` via `recover_outcome_unknown`; success is never inferred, nothing auto-retries |
| System tray | Typed state model (`src/menu.rs`) rendered deterministically; the adapter only translates specs onto widgets |
| Single instance | Exclusive file lock acquired before any window/tray/store exists (`src/single_instance.rs`) |
| Graceful shutdown | Fixed exactly-once step order (`src/shutdown.rs`); failures are reported, never fatal, never skip later steps |
| Autostart | OFF by default; changes only through explicit tray-menu user action |

## Per-OS capability matrix (honest)

| Lifecycle capability | Windows | macOS | Linux |
|---|---|---|---|
| Shell + Studio window | Shipped | Shipped | Shipped |
| Execution journal + crash recovery | Shipped | Shipped | Shipped |
| Single-instance guard | Shipped | Shipped | Shipped |
| Graceful shutdown sequencer | Shipped | Shipped | Shipped |
| System tray | Shipped | Shipped | Shipped (needs appindicator at runtime) |
| Opt-in autostart | **Shipped** — per-user registry value | **Unsupported** — no LaunchAgent is written by this build; tray reports unavailable | **Unsupported** — no XDG autostart entry is written by this build; tray reports unavailable |

`Unsupported` means exactly that: the backend refuses with a typed error and user surfaces render "unavailable". There is no silent fallback that pretends to enable anything.

## Persistence transparency (no hidden persistence)

Exactly two artifacts are ever created, both inside the data directory resolved in `apps/desktop/src-tauri/src/paths.rs`:

| Artifact | Path | Purpose | Created when |
|---|---|---|---|
| Execution journal store | `<data dir>/journal.sqlite3` (+ WAL/SHM sidecars while running) | Durable admission/prepared/terminal evidence (issue #15) | First launch (documented product behavior) |
| Instance lock | `<data dir>/openstream.lock` | Single-instance guard | First launch |

Data directory per platform: Windows `%APPDATA%\OpenStream`; macOS `~/Library/Application Support/OpenStream`; Linux `$XDG_DATA_HOME/OpenStream` or `~/.local/share/OpenStream`.

Nothing else is written: no telemetry, no caches, no hidden preference files. The autostart preference lives ONLY as the OS registration itself (see below) so there is no second store to drift.

## Autostart mechanism (Windows, opt-in only)

- Key: `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`
- Value name: `OpenStream`
- Value: quoted absolute path of the running executable (`REG_SZ`)
- No elevation, no service, no scheduled task, no arguments.
- Enable/disable happen only from an explicit tray-menu action; disable is idempotent. The tray checkbox always reflects OS truth read back from the registry.
- macOS/Linux: not implemented this milestone (see matrix).

## Single-instance mechanism

The lock is an OS-level exclusive file lock (`std::fs`) held on `<data dir>/openstream.lock` for the process lifetime. The kernel releases it when the holder exits for any reason — including crashes — so a previous crash can never wedge startup behind a stale marker. A second launch exits silently BEFORE creating windows, tray, or store connections: there is never a second writer or double tray.

## Shutdown order

Quit (tray menu or OS session end) runs these steps exactly once, each best-effort:

1. Tray renders its shutting-down presentation (every item disabled).
2. Engine runtime drops → its SQLite connection closes cleanly (SQLite auto-checkpoints the WAL on close).
3. Explicit WAL checkpoint through the shared store handle.
4. Store handle drops (last reference closes the database).
5. Instance lock releases.

A step's failure is recorded and remaining steps still run; exit always proceeds. This is safe by construction because committed evidence already survives process death (`WAL` + `synchronous=FULL`, issue #15) — the sequencer adds order and honesty, not durability risk.

## Crash-recovery semantics

- A crash between `prepared` and terminal evidence leaves an unresolved preparation; the next restart closes it as `outcome_unknown` and the tray shows "N executions outcome unknown after restart (review required)".
- `outcome_unknown` executions stay pending human review; they are exempt from pruning and are never replayed automatically (replay requires idempotency-declared graphs through the engine's own gate, wired in a later milestone).
- Damaged stores go through the issue #15 remedy ladder: restore a validated backup, else quarantine damaged files (preserved byte-for-byte) and recreate fresh. The tray/degraded path states what happened; nothing is silently destroyed or guessed.

## Scope guard (binding constraint from PR #75 independent gate)

This milestone creates NO consent surface for source-visibility or input-mute OBS grants, and wires none into the tray, settings, or UI. Those capabilities require a security ADR, capability-taxonomy update, and human gate BEFORE any implementation exists. The tray model here cannot express them by construction.

## Testing posture

- Headless CI (Linux): all lifecycle logic tested via fakes and real temp-dir file operations — menu rendering, shutdown ordering, instance-lock acquire/refuse/release/reacquire, autostart fake semantics, full composition/crash-window round-trips over the real SQLite pipeline.
- Windows-gated real smoke tests (`#[cfg(target_os = "windows")]`): the REAL registry backend against scratch subkeys under `HKCU\Software\OpenStream\Tests` (cleaned up; the production `Run` key is never touched by tests), plus the same lock suite over native Windows locking.
