# OpenStream capability taxonomy

Status: M0 authoritative draft for review
Issue: #2
Authority: defines the permission vocabulary referenced by `docs/architecture/SECURITY.md` ("Representative capabilities") and `docs/security/THREAT_MODEL.md`. Capability additions or widening require a security ADR and human gate.

## 1. Purpose and grammar

A **capability** is the smallest independently grantable authority unit. Identifiers use lowercase dotted vocabulary with optional typed qualifiers:

```text
<domain>.<resource>[.<verb>][:<qualifier>=<value>{,<qualifier>=<value>}]
```

- Qualifiers always *narrow*; a grant never exceeds its requested qualifier set.
- Wildcards are invalid inside grants and inside action-instance bindings. Manifests may request exact values only.
- Unknown capability identifiers fail closed at manifest validation, grant creation, and enforcement time.

Reserved domains: `obs.*` (OBS integration), `os.*` (host OS effects), `process.execute`, `clipboard.*`, `filesystem.*`, `network.connect`, `midi.send`, `osc.send`, `audio.control`, `notification.show`, `secret.*` (internal), `workspace.*` (sync/data), `plugin.state.*` (plugin-scoped state). New domains require a security ADR.

## 2. Evaluation model

Effective authority is recomputed by the Engine immediately before every side effect and equals the intersection of:

1. **Platform capability** - the OS actually permits it now.
2. **Manifest request** - the plugin/built-in declared this exact capability.
3. **User grant** - a recorded, user-consented grant exists, unnarrowed and unrevoked.
4. **Action-instance narrowing** - the specific action binding carries no broader parameters than the grant.
5. **Workspace policy** - team/fleet policy (Stage 2+) does not further restrict it.
6. **Runtime context** - session validity, expiry, rate limits, graph revision, dedupe state.

Any missing or ambiguous layer denies. Denial surfaces in the execution journal as `failed` with a typed reason; it is never silently converted into success.

## 3. Universal lifecycle rules

Every capability must define scope, consent, enforcement, revocation, and evidence. This section fixes the defaults; the registry (Section 5) records specifics per capability.

| Column | Default rule |
|---|---|
| Scope | Exact qualifiers from the request, narrowed by action-instance binding. No inheritance across plugins, workspaces, or versions. |
| Consent | Explicit user action in Studio: install-time manifest review plus first-use confirmation for effectful capabilities; destructive actions additionally require confirmation or explicit arming at press time. Silent, bundled, or pre-toggled consent is invalid. |
| Enforcement | Engine-side check of the Section 2 intersection immediately before the side effect; platform permission checked first; every result journaled. |
| Revocation | User-revocable at any time from Studio (per grant, per peer, or revoke-all); revocation deletes the grant record, emits an audit event, applies at next evaluation without restart; live peer sessions using revoked authority terminate immediately. |
| Evidence | Durable audit events for grant create/narrow/revoke plus execution states `accepted`, `running`, `succeeded`, `failed`, `cancelled`, `expired`, `outcome_unknown`; evidence obeys redaction rules (no labels, configs, paths, URLs, tokens, scene names). |

Standing rules:

- Permission increases across a plugin update require new consent (`PLUGIN_SDK.md`).
- Cloud can never widen a grant (`OPEN_CORE.md`); grants originate only from local user consent.
- An import whose hash/signature checks fail restores no grants.
- Capability additions or semantic widening require a security ADR and human gate.

## 4. Internal-only capabilities

| Capability | Rule |
|---|---|
| `secret.read:<secret_ref>` | Engine/integration-broker internal. Never appears in any manifest schema; never grantable to plugins, WebView commands, mobile peers, Cloud, or sync payloads. Secret bytes resolve only inside the broker performing one approved typed operation; caller-visible results are redacted. |
| `workspace.sync.read` / `workspace.sync.write` | Public sync client internals over E2EE envelopes; not exposed as third-party capabilities. Listed so every durable workspace write path has an owner. |
| `plugin.state.<scope>` | Plugin-scoped key/value state imports; isolated per plugin identity; never shared across publishers. |

## 5. Capability registry

Status = milestone whose PR must make the row enforced and evidenced before related surfaces ship. All rows are contract at M0; no runtime exists yet, so none are enforced.

| Capability | Scope | Consent | Enforcement | Revocation | Evidence | Status |
|---|---|---|---|---|---|---|
| `obs.read` | Read-only OBS state: scene list, program/preview, stream/recording flags | First-use prompt when an action binds it | Engine adapter mediates all reads; callers never touch the OBS socket directly | Unbind action or delete grant; applies at next evaluation | Adapter activity in execution timeline; redacted in logs/diagnostics | M1 |
| `obs.control.scene` | Switch to scenes named in the binding only | Install-time manifest review plus first-use confirmation | Pre-effect grant revalidation plus OBS connection health; default 30 s deadline | Unbind action; revocation kills pending switches | Journal entry per switch with outcome state | M1 |
| `obs.control.stream` | Start/stop streaming, recording, replay buffer | Destructive class: explicit arming/confirmation at press time in addition to first-use consent | Same as scene control plus destructive-action gate before effect | Revocation blocks future triggers; an interrupted stop surfaces `outcome_unknown`, never fake success | Full execution states journaled | M1 |
| `os.keyboard.emit[:app=<identity>]` | Synthetic key events, optionally window-scoped | Platform accessibility/input permission plus explicit user grant with high-risk disclosure | OS permission checked first; scope filter applied before each emit batch | Grant delete disables emitting adapters at next evaluation | Each emit batch journaled with outcome | M1 |
| `os.media.emit` | Media playback/soundboard output on engine-managed audio path | First-use confirmation | Mediation through the engine audio path only | Revoke silences the adapter | Outcome states journaled | M1 |
| `audio.control:<device>` | Volume/mute on named device(s) | First-use per device class | Named-device match enforced; unknown device fails closed | Per-device revoke | Journaled outcomes | M1 |
| `os.application.launch:<identity>` | Launch one user-selected application identity | Explicit per-application selection dialog | Identity revalidated immediately before launch; mismatch fails closed (Section 6) | Per-application revoke | Launch attempts and outcomes journaled | M1 |
| `process.execute:<approved executable identity>` | Execute exactly that pinned executable: typed argv, explicit working directory, clean environment, no inherited handles, no shell, no elevation | Explicit user selection of the executable plus per-binary approval; remote surfaces can trigger preconfigured bindings only and can never author executable/arguments/environment/directory | Identity/signature/hash revalidated immediately before launch; any unsupported platform control fails closed; plugins hold no process import ever | Revoke removes the binding; pending invocations are cancelled where already admitted | Prepared record persisted before side effect, terminal result after; crash gap yields `outcome_unknown` | M1 (items 11/22); hard-stop protected |
| `clipboard.read` / `clipboard.write` | Clipboard read or write as a discrete action step | Separate explicit grants for read and write; never bundled | Read returns content only into the action context, not logs; write requires typed payload from the action graph | Independent per-direction revoke | Outcomes journaled without clipboard contents | M1 |
| `filesystem.read:<handle>` / `filesystem.write:<handle>` | One user-selected handle (file/folder) per grant; string paths are invalid | Native picker selection by the user; plugin sees handle token only | Path resolution confined to the granted handle; traversal outside fails closed; response-size limits apply | Revoke invalidates the handle token everywhere including cached plugin state | Access outcomes journaled; paths redacted in evidence | M1 built-ins; M5 plugins |
| `network.connect:<scheme,host,port>` | Exact scheme/host/port tuple; HTTPS-only for third parties unless separately reviewed | Install-time review of exact tuples; local/private/link-local targets need a separate reviewed capability | Redirect policy, DNS-rebinding defense, private-address denial, response limits enforced in the adapter | Revoke closes adapter use of the tuple | Connection outcomes journaled with host redaction rules applied | M1 HTTP builtin; M5 plugins |
| `midi.send:<device>` / `osc.send:<endpoint>` | Emit messages to one named MIDI device or OSC endpoint | First-use per device/endpoint | Named-target match enforced; unknown target fails closed | Per-target revoke | Send outcomes journaled | M1+ integrations |
| `notification.show` | Post desktop notifications | First-use | Content templated from action graph; no secret interpolation | Revoke silences notifications | Show outcomes journaled | M1 |

Registry integrity rules:

- A capability row missing any column is invalid and cannot ship.
- The registry table is the single source of truth for the vocabulary; `SECURITY.md` remains the summary. Disagreements are resolved here first, then reconciled by PR.
- Deprecations keep their rows with a strikethrough note; identifiers are never reused.

## 6. Qualifier identity rules

- `<approved executable identity>` is a platform-stable identity (Windows: signed-file identity or absolute path plus hash; macOS/Linux: absolute path plus SHA-256). It is bound at approval time and revalidated before every launch.
- `<handle>` tokens are opaque, non-exportable, and die with the grant.
- `<scheme,host,port>` tuples are matched exactly after DNS resolution checks; redirects must stay inside the tuple or fail closed per redirect policy.

## 7. Compatibility and migration notes

- Vocabulary versioning follows OSCP discipline: additive minor additions are allowed within a reserved domain; changing a qualifier's meaning or removing a domain is a breaking change requiring protocol-major-style migration and an ADR.
- Import/export files carry capability requests, never grants; grants do not travel in portable profiles. Imported decks referencing ungranted capabilities import disabled and surface a typed denial until consented (`PRD.md` portability requirements).
- Schema impact: grant records, audit events, and dedupe entries are core entities (`TECHNICAL_SPEC.md` Section 4); their tables arrive with M1 persistence migrations.

## 8. Status honesty statement

At base commit `53ecf5027f409be8d9856b4c404cad40600650ec` this repository contains no runtime enforcement. Every "Enforcement" cell above is a contract that the named milestone's issue, tests, and independent security review must satisfy. This section must be updated, not deleted, as controls become enforced.

Enforced as of issue #8 (`openstream-domain` grant/capability/audit modules, `openstream-persistence` vault boundary):

- Capability identifiers parse against the closed v1 vocabulary only; unknown identifiers, wildcards, unknown/duplicate qualifier keys, and grammar violations reject fail closed at every entry point (§1).
- Deny-by-default grants are typed records created only with the consent class each registry row requires (§5 Consent column); narrowing can never widen; revocation (per grant, per subject, revoke-all) deletes records and applies at the next evaluation without restart (§3).
- Effective authority is recomputed per request as manifest ∩ user-grant ∩ instance-narrowing; platform capability, workspace policy, and runtime-context layers arrive with their own milestones and still deny downstream when absent (§2).
- Audit evidence events (grant create/narrow/revoke plus execution journal states) are append-only and carry qualifier-free capability kinds only; no labels, configs, paths, URLs, tokens, scene names, or qualifier values enter an event (§3 Evidence).
- `secret.read:<secret_ref>` is internal-only and rejects at manifest declaration, grant creation, and evaluation (§4); references validate against one structural grammar shared with the vault boundary.
- Secret values exist only behind the OS credential-vault abstraction: Windows ships a real Credential Manager backend; macOS/Linux report explicit `Unsupported` with no fallback storage. Values are never serializable by domain types and their buffers zeroize on drop (TB6, TM-LOG-01).

Enforced as of issue #10 (`integrations/os-automation` keyboard shortcut adapter):

- `os.keyboard.emit` ships its first concrete adapter behind engine action type `os.keyboard.shortcut`: typed, bounded shortcut configuration (closed key vocabulary, ≤4 chords × ≤4 tokens) validated fail closed at authoring time and revalidated per dispatch; untyped or off-vocabulary input fails with a typed error and never reaches synthesis.
- Explicit grant remains mandatory: an empty ledger denies before any dispatch (`NoActiveGrant`); revocation applies at the next evaluation; a scoped grant never covers an unqualified request.
- Platform matrix (honest reporting): Windows synthesizes through a real SendInput-class backend (pinned audited `enigo` wrapper; this crate stays `unsafe_code = "forbid"`). macOS and Linux return explicit typed `unsupported_platform` failures with no fallback of any kind. Wayland limitation: global synthetic input has no stable compositor-independent protocol under Wayland's security model, so Linux reports Unsupported regardless of session type until a reviewed platform milestone ships a backend.
- Window-scoped delivery is not implemented this milestone: the registration declares exactly the unqualified scope, so app-qualified nodes reject at the manifest intersection (`not_requested_by_manifest`) before dispatch, and the port refuses them defensively as well. No silent foreground delivery under a scoped grant.
- The adapter only ever sends synthetic events; no capture, hooking, polling, or logging of user input exists anywhere in it.

Enforced as of issue #11 (`integrations/os-automation` launch adapters):

- The `os.application.launch` row ships its first concrete adapters behind engine action types `os.launch.application`, `os.launch.file`, and `os.launch.url`. Typed target policies are validated fail closed at registration and revalidated per dispatch: application identities match a bounded lowercase token grammar; file targets must be absolute (POSIX root, drive-letter, or UNC form) rejecting traversal, empty/`.` components, device namespaces, and forbidden characters; URL targets must be absolute `scheme://…` form with a non-empty host, no userinfo, no whitespace/control bytes, and a scheme from the closed `{http, https}` vocabulary narrowed further by a per-registration allowlist (default HTTPS-only). Every accepted target token must embed verbatim into its capability qualifier value: bytes the domain qualifier grammar forbids reject with typed errors (paths refuse commas and surrounding whitespace; URLs refuse wildcards `?`/`*` and commas), so approved capabilities always round-trip byte-exact through configuration and persistence boundaries. No new capability vocabulary was added: opening a file or URL delegates to OS default-handler resolution, so all three kinds scope under the existing `os.application.launch:<identity>` row.
- Authority is per exact target: a node binds `os.application.launch:<identity>` where identity is the validated target token; grants cover exactly that token; the registration declares exactly the approved identities per kind, so any other target rejects at graph validation before grants are consulted; the port recomputes the token from parameters and fails closed on drift (`capability_mismatch`) before spawn.
- Revalidation before spawn (§6): the Windows backend resolves each application identity through its user-approved selection map and verifies the executable still exists immediately before launching; failure is a typed `missing_target`, never silent success.
- Process-execution constraints (SECURITY.md / TM-PRC-02): no shell interpreter anywhere; zero arguments passed to launched applications this milestone (typed argv belongs exclusively to the separately gated `process.execute` row); clean environment, explicit working directory, and nulled standard handles on direct spawns; executable-, script-, and interpreter-associated file targets refuse as policy violations (`policy_refused`) so the default-handler path can never collapse into direct process execution. The denial list covers every class a default Windows association can execute — native executables/installers plus `.py`/`.pyw` (Python), `.bat`/`.cmd` (command interpreter), `.ps1` (PowerShell), WSH classes (`.vbs`, `.js`, `.jse`, `.wsf`, `.wsh`), `.hta`, screen savers, DOS stubs, and shortcut/applet formats — and the extension lookup normalizes trailing dots/spaces exactly like Win32 path resolution, so aliases such as `payload.py.` cannot bypass it.
- Launches are journaled as non-idempotent effects with no safe compensation; failures carry structural codes only (`invalid_launch_config`, `policy_refused`, `missing_target`, `unsupported_platform`, `platform_refused`, `capability_mismatch`). Platform matrix stays honest: Windows launches via CreateProcess-class spawns plus direct ShellExecuteW-class handler opens inside pinned audited wrappers (`enigo`/`open`; the `open` wrapper runs with its opt-in `shellexecute-on-windows` feature and detached calls, so opens take the direct `ShellExecuteW` path with no shell intermediary; workspace-wide `unsafe_code = "forbid"` holds); macOS/Linux report typed `unsupported_platform` with no fallback of any kind.

Adapter-side rows above (OBS, keyboard/media, launch/process, clipboard, filesystem, network, MIDI/OSC, notifications): `os.keyboard.emit` is enforced per its paragraph above and `os.application.launch` joins it per the issue #11 paragraph above; the remaining rows remain contracts for their named milestone issues.

## References

- `docs/security/THREAT_MODEL.md` - threats mitigated by these capabilities
- `docs/architecture/SECURITY.md` - summary model
- `docs/architecture/PLUGIN_SDK.md` - manifest and sandbox constraints
- `docs/product/ROADMAP.md` - milestone mapping
