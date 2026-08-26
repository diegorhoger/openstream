# Desktop shell lifecycle

Status: shipped in M1 (issue #16). Composes the merged Engine (#9, #14), adapters (#10–#13), and SQLite persistence (#15) into a dependable background desktop application.

## What the shell composes

| Piece | Realization |
|---|---|
| Engine time port | `SystemClock` (`apps/desktop/src-tauri/src/clock.rs`) — the real monotonic/wall source the engine crate explicitly deferred to this composition root |
| Durable evidence | `ActionRuntime` over `SqliteJournal` (issue #15 open pipeline: WAL, `synchronous=FULL`, integrity verification, forward-only migrations) |
| Crash recovery | On every restart, prepared-without-terminal records close as `outcome_unknown` via `recover_outcome_unknown`; success is never inferred, nothing auto-retries |
| System tray | Typed state model (`src/menu.rs`) rendered deterministically; the adapter only translates specs onto widgets |
| Single instance | Exclusive file lock acquired before any window/tray/store exists (`src/single_instance.rs`); an unresolvable guard REFUSES startup (fail closed) |
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
| Global hotkey registration (issue #19) | **Shipped** — `RegisterHotKey`-class registration via pinned wrapper | **Unsupported** — no registration backend in this build; typed visible state | **Unsupported** — no registration backend in this build; typed visible state |
| Focused-app identity (issue #19) | **Shipped** — foreground process image name only | **Unsupported** — typed visible state, no observation of any kind | **Unsupported** — typed visible state, no observation of any kind |

`Unsupported` means exactly that: the backend refuses with a typed error and user surfaces render "unavailable". There is no silent fallback that pretends to enable anything.

## Persistence transparency (no hidden persistence)

Exactly three artifacts are ever created, all inside the data directory resolved in `apps/desktop/src-tauri/src/paths.rs`:

| Artifact | Path | Purpose | Created when |
|---|---|---|---|
| Execution journal store | `<data dir>/journal.sqlite3` (+ WAL/SHM sidecars while running) | Durable admission/prepared/terminal evidence (issue #15) | First launch (documented product behavior) |
| Workspace store | `<data dir>/workspace.sqlite3` (+ WAL/SHM sidecars while running) | Authored deck/profile documents for the Studio editor (issue #17); created on the first Studio session open, including autosave-degraded sessions that only read | First Studio use |
| Instance lock | `<data dir>/openstream.lock` | Single-instance guard | First launch |

Data directory per platform: Windows `%APPDATA%\OpenStream`; macOS `~/Library/Application Support/OpenStream`; Linux `$XDG_DATA_HOME/OpenStream` or `~/.local/share/OpenStream`. If NO data directory can be resolved, the process refuses to start with a logged typed reason and a non-zero exit code — it never runs without its documented persistence home.

Nothing else is written: no telemetry, no caches, no hidden preference files. The autostart preference lives ONLY as the OS registration itself (see below) so there is no second store to drift. The workspace store holds authored documents only; if its store cannot open or its contents fail domain validation, the Studio editor keeps running from memory and surfaces `saved = false` plus a typed token until persistence recovers (see `docs/architecture/STUDIO_EDITOR.md`).

## Autostart mechanism (Windows, opt-in only)

- Key: `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`
- Value name: `OpenStream`
- Value: quoted absolute path of the running executable (`REG_SZ`)
- No elevation, no service, no scheduled task, no arguments.
- Enable/disable happen only from an explicit tray-menu action; disable is idempotent. The tray checkbox reflects read-back registry truth: ONLY a missing subkey/value reads as Disabled — access-denied or other registry I/O failures surface as an explicit query-failure state instead of masquerading as Disabled.
- macOS/Linux: not implemented this milestone (see matrix).

## Profile switching mechanisms (issue #19)

Full behavioral contract: `docs/architecture/PROFILE_SWITCHING.md`; security decision: `docs/adr/0006-profile-switching-mechanisms.md`. Summary:

- **No input capture.** Global hotkeys are `RegisterHotKey`-class REGISTRATIONS inside a pinned wrapper — the OS delivers presses for combinations OpenStream registered and nothing else is ever observed. Focused-app matching reads ONLY the foreground process image file name (lowercased identity token); titles, content, and keystrokes are never read anywhere.
- Both mechanisms start DENIED each launch (consent ledger is in-memory this milestone) and require explicit first-use consent per mechanism from the switching panel; revocation unregisters shortcuts / stops observation in the same call.
- A dedicated worker thread serializes every registration mutation; a second worker tick re-syncs authored rules, drains OS press deliveries, and applies focus changes through the deterministic engine (documented batch precedence: explicit hotkeys outrank focused-app automation; denials never rewind authorized switches).
- Unsupported platforms, contested combinations, and unreadable focus surface as typed visible states in the Studio live view; nothing degrades silently.

## Single-instance mechanism

The lock is an OS-level exclusive file lock (`std::fs`) held on `<data dir>/openstream.lock` for the process lifetime. The kernel releases it when the holder exits for any reason — including crashes — so a previous crash can never wedge startup behind a stale marker. A second launch exits silently BEFORE creating windows, tray, or store connections: there is never a second writer or double tray.

Fail-closed rule: if the guard cannot be acquired for ANY other reason (lock directory unwritable, unexpected I/O class), startup is REFUSED — logged typed reason `guard-unavailable`, non-zero exit — rather than running a shell that could not guarantee exclusivity. The refusal decision and both outcomes are covered by unit tests over real locks.

## Shutdown order

Quitting (tray menu or OS session end) runs these steps exactly once per process — an atomic compare-exchange gate admits a single winner BEFORE any task is built, so the first step genuinely renders and every later caller no-ops. All controlled exit paths funnel through the same gate:

1. Tray renders its shutting-down presentation (every item disabled).
2. Engine runtime drops → its SQLite connection closes cleanly (SQLite auto-checkpoints the WAL on close).
3. Explicit WAL checkpoint through the shared store handle.
4. Store handle drops (last reference closes the database).
5. Instance lock releases.

A step's failure is recorded and remaining steps still run; exit always proceeds. This is safe by construction because committed evidence already survives process death (`WAL` + `synchronous=FULL`, issue #15) — the sequencer adds order and honesty, not durability risk. A crash skips the sequencer entirely; that window belongs to restart recovery below, not to this path.

## Crash-recovery semantics

- A crash between `prepared` and terminal evidence leaves an unresolved preparation; the next restart closes it as `outcome_unknown` and the tray shows "N executions outcome unknown after restart (review required)".
- A store that went through the damage remedy ladder (backup restore or quarantine-and-recreate) NEVER presents as plain "running": the tray shows a distinct recovered state — "journal recovered from damage; some execution history may be missing" — plus a matching log line, because rolled-back history is a fact users must see.
- `outcome_unknown` executions stay pending human review; they are exempt from pruning and are never replayed automatically (replay requires idempotency-declared graphs through the engine's own gate, wired in a later milestone).
- Damaged stores go through the issue #15 remedy ladder: restore a validated backup, else quarantine damaged files (preserved byte-for-byte) and recreate fresh. The tray/degraded path states what happened; nothing is silently destroyed or guessed.

## Scope guard (binding constraint from PR #75 independent gate)

This milestone creates NO consent surface for source-visibility or input-mute OBS grants, and wires none into the tray, settings, or UI. Those capabilities require a security ADR, capability-taxonomy update, and human gate BEFORE any implementation exists. The tray model here cannot express them by construction.

## Testing posture

- Headless CI (Linux): all lifecycle logic tested via fakes and real temp-dir file operations — menu rendering, shutdown ordering, instance-lock acquire/refuse/release/reacquire, autostart fake semantics, full composition/crash-window round-trips over the real SQLite pipeline.
- Windows-gated real smoke tests (`#[cfg(target_os = "windows")]`): the REAL registry backend against scratch subkeys under `HKCU\Software\OpenStream\Tests` (cleaned up; the production `Run` key is never touched by tests), plus the same lock suite over native Windows locking.
