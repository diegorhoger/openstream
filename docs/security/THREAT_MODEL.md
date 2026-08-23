# OpenStream threat model

Status: M0 authoritative draft for review
Issue: #2
Supersedes: nothing; extends `docs/architecture/SECURITY.md` with per-boundary detail. Where the two disagree, this document and a security ADR govern.

## 1. Scope, method, and honesty rules

This is the authoritative STRIDE-informed threat model for OpenStream across its product stages (desktop-local, native LAN, Cloud relay, browser via Cloud, native mobile, plugins). It defines assets, actors, trust boundaries, abuse cases, mandatory mitigations, and fail-closed behavior. The capability vocabulary it references is defined in [`CAPABILITY_TAXONOMY.md`](CAPABILITY_TAXONOMY.md).

Method constraints:

- **Deny by default.** Every requester is untrusted until the Engine grants otherwise. Authority never exceeds the intersection of platform capability, manifest request, user grant, action-instance narrowing, workspace policy, and runtime context.
- **Host authority.** Only the desktop Engine executes privileged actions (`docs/adr/0002`). `relayed` is transport evidence, not execution success.
- **No claimed-but-unbuilt controls.** Each mitigation carries an implementation status. At M0 no runtime exists; controls marked *specified* are contracts that later milestones must satisfy before the related surface ships.
- **Fail closed.** Ambiguity, expiry, mismatch, or missing evidence resolves to denial, never to success.
- No homegrown cryptography, no silent suite fallback, no new arbitrary process authority may be introduced by any change to this model without a security ADR and human gate.

## 2. System description

| Component | Trust role |
|---|---|
| Desktop Engine (Rust) | Authoritative executor. Owns grants, secrets references, state, durable admission dedupe, execution journal, pairing trust store. |
| Studio UI (Tauri WebView) | Untrusted renderer. May only invoke narrowly scoped Tauri commands. |
| Built-in adapters + integration broker | Engine-internal executors for OBS, OS automation, HTTP, MIDI, OSC. Broker resolves secret operations without exposing raw secret bytes. |
| Plugin host (Wasmtime) | Sandboxed third-party code. Capability-scoped imports only. |
| OSCP transports | Desktop-local IPC; native LAN over Noise; Cloud over TLS 1.3 WebSocket with E2EE relay envelopes; browser reaches Engines only through Cloud. |
| Cloud service | Untrusted coordination tier in a separate private repository. Server-readable routing/entitlement metadata only; payloads opaque. |
| Native mobile clients | Paired untrusted requesters holding OS-protected device identity. |
| Persistence | SQLite WAL (local), OS credential vault (secret values), content-addressed SHA-256 assets. |

## 3. Assets

| ID | Asset | Impact if compromised |
|---|---|---|
| A1 | OBS session control (scenes, sources, stream, recording, replay buffer) | Live-production disruption, public broadcast manipulation |
| A2 | OS input synthesis rights (keyboard/mouse emit) | Arbitrary input injection into any local application |
| A3 | Media/audio output devices | Noise injection, audio exfiltration paths, nuisance output |
| A4 | Application/process launch rights | Malware launch, persistence |
| A5 | Clipboard contents | Confidential-data theft or injection |
| A6 | Filesystem handles granted to plugins/adapters | Local file read/write outside intent |
| A7 | Network reachability from adapters/plugins | SSRF into LAN/cloud services |
| A8 | Secret values (OBS credentials, API keys, OAuth tokens) | Account takeover; must never exist outside OS credential storage |
| A9 | Pairing identities: Engine static X25519 key, peer records, QR PSKs | Peer impersonation, LAN session takeover |
| A10 | Deck/workspace configuration and sync history | Tampered workflows; privacy of team content |
| A11 | Execution journal and audit events | Forensic blindness, forged success evidence |
| A12 | Plugin packages and signatures | Malicious distribution under a trusted publisher identity |
| A13 | Update artifacts and signing/notarization keys | Persistent code-execution compromise of installed base |
| A14 | Logs, diagnostics, support bundles, crash reports | Secret/config/path leakage; user privacy loss |
| A15 | Cloud account, sessions, entitlement tokens | Unauthorized relay access, billing abuse |
| A16 | E2EE sync payloads and encryption keys | Workspace content exposure despite Cloud compromise |
| A17 | Telemetry (opt-in only) | Privacy loss; must be absent unless consented |

## 4. Actors

| Actor | Privilege | Assumed hostile when… |
|---|---|---|
| User / operator | Ultimate consent authority | Not assumed; destructive-action confirmation protects against accident too |
| Desktop Engine | Full local authority | Compromised desktop ⇒ game over locally; model minimizes blast radius outward |
| Studio WebView | None beyond narrow IPC commands | Always (renderer compromise is assumed possible) |
| Built-in adapter | Scoped engine-internal executor | Buggy; bounded by typed inputs, deadlines, journaling |
| Plugin (Wasm) | Only explicitly imported capabilities | Always |
| Native mobile peer | Granted OSCP scopes over paired session | Lost, stolen, or maliciously modified |
| Cloud service | Routing/entitlement metadata only | Always (compromise must not become OS authority) |
| Browser client | Cloud-relayed E2EE requests only | Always |
| LAN attacker | On-path or adjacent network position | Always on shared networks |
| Supply-chain actor | Build/publish pipeline positions | Compromised dependency, CI, registry, or maintainer identity |
| Update infrastructure | Distribution channel | Compromised mirror/channel |

## 5. Trust boundaries

| ID | Boundary | Crossing rule |
|---|---|---|
| TB1 | Process: Studio WebView ↔ Engine | Typed allowlisted Tauri commands only; strict CSP; local assets; no raw secrets cross |
| TB2 | Sandbox: plugin Wasm ↔ Engine host | Narrow WIT imports; memory/fuel/time/concurrency limits; no ambient OS access |
| TB3 | Device: LAN peers ↔ Engine | Noise transport session; every command revalidated locally; dedupe at admission |
| TB4 | Segment: internet ↔ Engine | Outbound-only Engine connections to Cloud; no inbound listener until native-LAN consent |
| TB5 | Tenant: browser ↔ Cloud ↔ Engine | E2EE envelopes; Cloud sees server-readable metadata only; never direct-to-LAN |
| TB6 | Storage: process ↔ SQLite/OS vault | Explicit migrations; secret values only in OS credential storage; no plaintext secrets |
| TB7 | Supply chain: source ↔ build ↔ publish ↔ install | Lockfiles, policy checks, SBOM/provenance, signed artifacts |
| TB8 | Update: installed client ↔ update channel | Signed updates, platform signing/notarization, rollback protection |

## 6. Threat coverage by surface

### 6.1 LAN — discovery, pairing, and established sessions

Trust assumption: the local network is hostile. mDNS and Noise frames are observable and forgeable.

| Threat ID | Abuse case | Mitigation (status) |
|---|---|---|
| TM-LAN-01 | Discovery enumeration of users/decks/actions | mDNS `_openstream._tcp.local` advertises instance UUID, OSCP major, port, label only — no accounts, decks, actions, tokens, keys, PSKs, or membership; listener stays disabled until the user enables native-LAN control (specified; enforced from M2/M4 gates) |
| TM-LAN-02 | MITM during QR pairing | Fixed enrollment suite `Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s`; exact prologue `OpenStream-Pairing-v1 || oscp_major || pairing_id || engine_instance_uuid`; two-minute single-use QR with 32-byte random PSK erased on success/expiry/cancel/five failures (specified; M2 implementation + independent crypto review required before any real listener ships) |
| TM-LAN-03 | Downgrade to weaker pattern/cipher/plaintext | Pattern and cipher fixed per OSCP major; no fallback path exists; major mismatch fails closed (specified; M2 conformance vectors) |
| TM-LAN-04 | Evil-twin Engine tricks user into trusting attacker | Six-word/30-bit SAS derived as `BLAKE2s("OpenStream-SAS-v1" || handshake_hash)`; desktop displays candidate device + requested scopes + SAS and requires explicit confirmation before persisting the peer key (specified; M2) |
| TM-LAN-05 | Established-session spoofing with stolen static keys | Established sessions use `Noise_IK_25519_ChaChaPoly_BLAKE2s`; static keys live in OS-protected storage; scoped peer record binds device identity to granted scopes (specified; M2/M4) |
| TM-LAN-06 | Rogue peer floods commands | Per-peer rate limits, short command deadlines, expired commands rejected and never queued; admission dedupe (§6.9) (specified; M2 conformance) |

Residual risk: a compromised desktop defeats all local controls; physical-access attacks on an unlocked machine are out of model scope but revocation limits a stolen paired phone (§6.5).

### 6.2 WebView — Studio desktop UI

Trust assumption: the renderer can be compromised by crafted content; it must never hold authority.

| Threat ID | Abuse case | Mitigation (status) |
|---|---|---|
| TM-WEB-01 | XSS → invoking privileged commands beyond intent | Tauri capability allowlist; strict CSP; local assets only; each command narrowly typed and Engine-side reauthorized against grants (specified; M1 build scaffold enforces) |
| TM-WEB-02 | Renderer forges execution success claims | UI renders Engine-journal evidence only; "no success before authoritative Engine result" is a hard rule; `relayed`/transport events cannot render as success (specified) |
| TM-WEB-03 | Secret display/exfiltration through renderer | Raw secret bytes never enter the WebView; exports redact secrets; clipboard/secret capabilities require their own grants (specified; M1) |
| TM-WEB-04 | Deep-link/file-import confusion | Import validates schema version, hashes, and signatures; destructive import requires explicit confirmation (specified; M1 portability issue) |

### 6.3 Plugins

Trust assumption: plugin publishers range from careless to malicious; the sandbox is the only defense line besides consent.

| Threat ID | Abuse case | Mitigation (status) |
|---|---|---|
| TM-PLG-01 | Sandbox escape / resource exhaustion | Wasmtime Component Model, narrow WIT world, 64 MiB default memory (256 MiB marketplace max), 10 MiB component cap, fuel + epoch interruption, 2 s synchronous invocation, concurrency 4 per plugin / 32 global (specified; M5 runtime, limits already contractual) |
| TM-PLG-02 | Ambient access (files, sockets, env, clipboard, processes) | No such imports exist; default grant set is empty; each import maps 1:1 to a taxonomy capability (specified) |
| TM-PLG-03 | Secret theft via integration use | No raw-secret import; user selects an Engine-owned connection; plugin receives only an opaque handle; broker performs the typed approved operation and returns a redacted result; `secret.read` is never grantable to plugins (specified) |
| TM-PLG-04 | SSRF via domain-restricted HTTP | Exact scheme/host/port grants, redirect policy, response limit, DNS-rebinding defense, private/link-local denial unless a separately reviewed local-network capability is granted (specified) |
| TM-PLG-05 | Malicious marketplace package | Signed packages; publisher identity; permission increases on update require new consent; unsigned sideload requires developer mode plus per-install warning; moderation contracts tracked at M5 (partially specified; registry/moderation is M5 scope) |
| TM-PLG-06 | Privilege creep across versions | Manifest capability diff shown at update; widening needs fresh consent and, for new capability kinds, a security ADR (specified) |

### 6.4 Cloud and browser access

Trust assumption: the Cloud service and the browser are fully compromisable; neither may gain OS authority.

| Threat ID | Abuse case | Mitigation (status) |
|---|---|---|
| TM-CLD-01 | Cloud turns delivery into execution | Engine-only execution; Cloud verifies membership/entitlement/revocation/target/rate/TTL/route metadata then relays opaque ciphertext; Engine decrypts, revalidates local grants and graph revision, dedupes, executes (ADR-0002; contract specified; hosted impl is out of this repository) |
| TM-CLD-02 | Direct browser-to-LAN control | Unsupported by protocol design; browsers speak only authenticated Cloud TLS 1.3 WebSocket; Engine accepts relayed envelopes only after full local revalidation (specified) |
| TM-CLD-03 | Cloud reads workspace content | E2EE envelopes for sync/relay content; Cloud stores ciphertext plus server-readable metadata; keys stay with user devices (contract specified; M3 vectors/portability tests) |
| TM-CLD-04 | Captivity: outage/expiry locks local control | Anti-captivity invariant: subscription expiry or Cloud outage cannot impair local execution; local-forever downgrade tested publicly (M3 conformance) |
| TM-CLD-05 | Stolen account relays commands to victim Engines | Device membership binding, revocation propagation, rate/TTL checks server-side, plus grant revalidation Engine-side (contract specified) |
| TM-CLD-06 | Relay replay/duplication | Same durable admission dedupe as LAN (§6.9); short-lived command TTLs (specified) |

### 6.5 Mobile devices

Trust assumption: phones are lost, stolen, and app-modified; the device identity is the trust anchor.

| Threat ID | Abuse case | Mitigation (status) |
|---|---|---|
| TM-MOB-01 | Stolen device drives live production | X25519 static key in OS-protected keystore/Secure Enclave-class storage; optional app lock; remote/local revocation deletes trust and terminates live sessions; short Cloud sessions (specified; M4) |
| TM-MOB-02 | Modified client bypasses UX safety | All safety is Engine-side (arming, deadlines, dedupe); client is only a requester (structural; holds for all surfaces) |
| TM-MOB-03 | Cached decks execute while offline/disconnected | Host-dependent controls show unavailable rather than fake success; operational presses expire and never queue (specified; M4) |
| TM-MOB-04 | Mobile runs downloaded code | Mobile does not execute downloaded third-party plugins initially (PRD Stage 3 boundary) |
| TM-MOB-05 | Local-network permission abuse | Native OS permission gates discovery/listener activation; manual QR fallback works without broadcast (specified; M4) |

### 6.6 Supply chain

| Threat ID | Abuse case | Mitigation (status) |
|---|---|---|
| TM-SUP-01 | Compromised dependency introduces backdoor | Lockfiles committed; cargo-deny/license/advisory policy in CI; dependencies pinned and reviewed (CI present; enforcement grows with code) |
| TM-SUP-02 | CI tampering | Actions default read-only, granted per job; third-party Actions pinned by commit SHA (enforced in `.github/workflows/governance.yml`) |
| TM-SUP-03 | Artifact substitution between build and install | SBOM + provenance + checksums + signed artifacts for releases (release system; specified, gated at first release) |
| TM-SUP-04 | Generated-code drift hides protocol changes | Generated clients/golden fixtures committed; CI rejects stale generation (specified; activates with M2 codegen) |
| TM-SUP-05 | Prohibited-language/tool smuggling (e.g., Python in product paths) | Repository contract forbids Python in shipped paths; governance CI rejects `.py` under shipped dirs (enforced today) |

### 6.7 Updates and distribution

| Threat ID | Abuse case | Mitigation (status) |
|---|---|---|
| TM-UPD-01 | Unsigned/malicious update pushed to installed base | Automatic updates are signed only; platform signing/notarization; no unsigned automatic update ever (hard rule) |
| TM-UPD-02 | Rollback to vulnerable version | Rollback protection in update verification (specified; M1 item 22 plan) |
| TM-UPD-03 | Update channel downgrade/MITM | TLS-pinned channel endpoints; signature verified against embedded public keys before apply; failed verification aborts install (specified) |
| TM-UPD-04 | Store/beta distribution leaks signing keys | Keys restricted to Release role behind protected environments and human authorization; never used autonomously (governance; hard stop) |

### 6.8 Logs, diagnostics, telemetry, crash reports

| Threat ID | Abuse case | Mitigation (status) |
|---|---|---|
| TM-LOG-01 | Secrets/tokens in logs or bundles | Structured logging allowlist; excludes labels, configs, file paths, URLs, tokens, scene names; secret values never serialized anywhere (hard rule; facade specified, enforced from M1 observability work) |
| TM-LOG-02 | Silent telemetry growth | Telemetry/analytics are explicit opt-in only; absence is the default and tested (contract) |
| TM-LOG-03 | Crash reports capture sensitive buffers | Crash reporter filters to stack/metadata; no deck content, scene names, or secret material (specified) |
| TM-LOG-04 | Support bundle over-collection | Bundles are user-initiated, previewed, and redaction-tested before export (specified; M1 diagnostics item) |

### 6.9 Replay, admission, and crash windows

| Threat ID | Abuse case | Mitigation (status) |
|---|---|---|
| TM-RPL-01 | Captured/replayed command re-executes | Envelope carries globally unique message ID + monotonic sequence + expiry; Engine admission dedupes `(source_device_id, message_id)` within a bounded durable window; expired commands rejected, never queued (specified; M2 conformance incl. fault injection) |
| TM-RPL-02 | Delay-and-release during a live show | Short deadlines + sequence tracking + graph-revision revalidation immediately before effect (specified) |
| TM-RPL-03 | Crash between prepare and side effect reported as success | Durable prepared record before requesting the effect, terminal result persisted after; the gap is `outcome_unknown` — never inferred success; no non-idempotent automatic retry; replay permitted only with adapter idempotency key or reconciliation proof (specified; M1 engine + M2 simulator crash-window tests) |
| TM-RPL-04 | Duplicate effects under transport at-least-once redelivery | Idempotency-key conformance for adapters; zero-duplicate-effect targets in test gates (TECHNICAL_SPEC §10) |

### 6.10 Privileged actions — process execution boundary

Trust assumption: launching anything on the host is the highest-value abuse target; V1 exposes no general runner.

| Threat ID | Abuse case | Mitigation (status) |
|---|---|---|
| TM-PRC-01 | Remote/browser/plugin authorship of arbitrary commands | Remote surfaces may trigger only preconfigured bindings; they can never author or alter executable, arguments, environment, or directory; plugins have no process import at all (specified) |
| TM-PRC-02 | Shell interpolation/injection | No shell invocation (`sh`, `bash`, `cmd`, PowerShell, AppleScript, equivalents); typed argv with no interpolation; clean environment/CWD; no inherited standard handles; no elevation (specified) |
| TM-PRC-03 | Binary swapped after approval | Pinned executable identity bound to a platform-stable identity; platform file identity/signature/hash revalidated immediately before launch; change fails closed (specified) |
| TM-PRC-04 | User tricked into approving a launcher for malware | Explicit user selection of the executable; destructive/launch actions require confirmation or arming; capability `process.execute:<approved executable identity>` is per-binary (taxonomy §CAPABILITY registry) |
| TM-PRC-05 | Scope creep into "arbitrary process authority" | Hard stop per AGENTS.md: any widening requires a security ADR and human decision; unsupported platform control fails closed (standing rule) |

## 7. Cross-cutting invariants

1. Authority intersection (platform ∩ manifest ∩ grant ∩ instance-narrowing ∩ workspace policy ∩ runtime context) is recomputed at the Engine immediately before every side effect.
2. Every capability has scope, consent, enforcement, revocation, and evidence rules ([CAPABILITY_TAXONOMY.md](CAPABILITY_TAXONOMY.md)); a capability lacking any row cannot ship.
3. Evidence of success originates only from Engine journal states `accepted`, `running`, `succeeded`, `failed`, `cancelled`, `expired`, `outcome_unknown`.
4. Revocation is immediate in effect for trust and sessions; grants persist only where explicitly recorded and auditable.
5. Any high/critical unresolved finding blocks dependent surfaces from shipping (issue #2 hard-stop condition).

## 8. Compatibility notes and evidence status

- This model adds no protocol wire changes; it constrains implementers of issues #3–#61. Compatibility surface remains PROTOCOL.md v1 semantics (additive minor changes only; fixed suites per major).
- Implementation status markers (*specified* vs *enforced*) must be updated by the implementing milestone's PR; a control may not be described as active before its enforcing code merges. At HEAD `53ecf5027f409be8d9856b4c404cad40600650ec` no runtime code exists; everything above is contract.
- Threat IDs are stable; retirements/deprecations are recorded here, never silently deleted.

## 9. Review triggers

Independent security review is mandatory before shipping changes touching authentication, networking, pairing, cryptography, OS permissions, secrets, remote control, plugins, billing, updates, signing, privacy, retention, or tenant isolation — and before any real LAN listener (per ADR-0003).

## References

- `docs/architecture/SECURITY.md` — summary model and hard rules
- `docs/architecture/PROTOCOL.md` — OSCP transports, envelope, pairing, delivery semantics
- `docs/architecture/TECHNICAL_SPEC.md` — engine, storage, cloud/mobile boundaries, test system
- `docs/architecture/PLUGIN_SDK.md` — sandbox and manifest rules
- `docs/product/NON_GOALS.md`, `docs/product/OPEN_CORE.md` — scope defenses
- `docs/adr/0001`–`0004` — authority, protocol, boundary decisions
