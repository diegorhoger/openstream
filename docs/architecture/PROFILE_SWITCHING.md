# Profile switching

Status: implemented contract (M1, issue #19)  
Authority: `crates/openstream-domain/src/switching.rs` (pure model) plus the
desktop composition in `apps/desktop/src-tauri/src/switching.rs` with its
platform ports (`hotkeys.rs`, `focus.rs`). Security decisions live in
`docs/adr/0006-profile-switching-mechanisms.md`; permission rows live in
`docs/security/CAPABILITY_TAXONOMY.md`.

## 1. What switching is

A **profile** is a named arrangement of decks. One profile is *active* at a
time. Switching changes which profile is active; triggers are always
EXPLICIT rules authored by the user — nothing switches without a rule.

Two trigger mechanisms exist, each behind its own explicit grant:

| Mechanism | Trigger | Authority | Platform support |
|---|---|---|---|
| Global shortcuts | The OS delivers a registered combination press | `os.hotkey.register` | Windows shipped; macOS/Linux report honest unavailability |
| Focused-app matcher | The configured application identity holds keyboard focus | `os.focus.read` | Windows shipped; macOS/Linux report honest unavailability |

## 2. Hard boundary: no input capture

Global hotkeys are REGISTRATION-based only. A pinned wrapper performs
`RegisterHotKey`-class registration and the operating system delivers press
notifications for exactly the combinations OpenStream registered. No
keyboard hooks, message-stream listeners, keystroke logging, or polling of
user input exist anywhere in this repository. Focused-app matching observes
ONLY the foreground window's process image file name (e.g. `obs64.exe`,
lowercased). Window titles, window content, keystrokes, and anything beyond
that single identity token are never read, stored, or logged.

## 3. Rule model

- Rules are stored ON their target profile as an optional field
  (`switch_rules`, additive-minor per DOMAIN_MODEL.md §1); pre-#19 documents
  decode unchanged.
- A trigger is either:
  - `hotkey:<combo>` — at least one modifier (`ctrl`, `alt`, `shift`,
    `meta`) plus exactly one key from the closed vocabulary `a-z`, `0-9`,
    `f1-f24`. Bare keys reject: a global bare-key shortcut would shadow
    ordinary typing on the whole desktop. Serialization is canonical
    (`ctrl+alt+shift+meta` order) regardless of input order.
  - `app_focus:<identity>` — an exact lowercase application identity token
    (`[a-z0-9._-]{1,64}`, no leading/trailing dot or dash, no `..`, no
    wildcards anywhere).
- Every rule carries an `enabled` flag. Disabled rules stay stored but
  inert — AND they still reserve their trigger so re-enabling can never
  introduce a surprise conflict.

## 4. Deterministic priority and conflict rules (total order)

1. **Configuration conflicts resolve by typed rejection.** Two rules
   anywhere in one workspace may never bind the same trigger. The authoring
   op that would create the second binding refuses with
   `conflicting_switch_rule:<kind>`; a persisted set that violates this
   (only possible outside normal authoring) loads as an EMPTY board with a
   visible conflict flag — never a silent partial pick. Within-class
   ambiguity therefore cannot exist at runtime, and no tie-break is needed.
2. **Batch precedence.** Events resolved within one evaluation pass apply
   lowest-precedence class first, so among authorized switches the highest-
   precedence class present determines the final active profile: explicit
   hotkey presses outrank focused-app automation.
3. **Denial never rewinds.** An event whose mechanism lacks an active grant
   resolves to a typed denial and changes nothing; it cannot mask or undo an
   authorized switch from the same pass.
4. **Across passes, chronology decides.** Each authorized switch updates the
   active profile immediately.

## 5. Grants, consent, revocation

- Both capabilities require recorded first-use consent (`ConsentEvidence`);
  silent, bundled, or pre-toggled consent is invalid per taxonomy §3.
- Effective authority is recomputed from the ledger before EVERY evaluation;
  it is never cached.
- **Revocation stops matching immediately:** revoking a mechanism's grant
  unregisters that mechanism's shortcuts / stops focus observation inside
  the same call, appends audit evidence, and denies at the very next
  evaluation.
- Consent records live in the in-memory shell ledger this milestone: after a
  restart both mechanisms start DENIED until re-granted. This is deliberate
  fail-closed behavior; durable grant storage arrives with its own milestone.

## 6. Lifecycle and reconciliation

Every configuration or consent change reconciles the *applied* registration
set against the *desired* set derived from board × authority:

1. Removals first — stale registrations are unregistered immediately, so a
   revoked/disabled mechanism stops listening even if later steps fail. A
   refused removal keeps its applied marker (the OS still delivers those
   presses; pretending otherwise would be dishonest) and surfaces a typed
   issue.
2. Additions second — desired-but-missing combinations register. Conflicts
   (combination owned by another application) and platform refusals land in
   the visible-issue list and keep retrying on every future reconciliation
   until they converge.

The OS also releases every registration automatically when the process exits
for any reason, including crashes.

Focus observation runs only while the mechanism holds BOTH an active grant
and platform support — revocation stops the observation loop itself, not
just matching. Observations poll at a fixed interval and only fire switches
on identity CHANGE.

## 7. Visible degradation

Every degraded condition renders as typed text state; absence of issue text
is the contract for healthy:

- `unsupported:<os>` — no backend shipped for this mechanism/platform build.
- `register-conflict:<combo>` — another application owns the combination.
- `register-refused:<combo>` / `unregister-refused:<combo>` — OS refused.
- `focus-unreadable` — transient observation failure (secure desktop, ...);
  cleared automatically by the first healthy observation afterwards.
- Board conflict banner — automatic switching paused until the duplicate
  trigger is fixed in the editor.

## 8. Testing posture

- Deterministic fakes drive conflicts, revocation immediacy, batch
  precedence, lifecycle convergence (exact register/unregister traces), and
  degradation visibility on every CI platform.
- Windows-gated real tests exercise genuine registration round-trips
  (including re-registration races against actual OS truth) and real focus
  identities.
- Golden fixtures pin the deterministic wire form of switch-rule documents.

## 9. Rollback

Remove the surfaces and stop issuing grants; registrations die with the
process and no persisted state needs migration. Pre-#19 documents remain
valid forever because the new profile field is optional.
