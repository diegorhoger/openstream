# Security and capability model

OpenStream is deny-by-default and host-authoritative. Effective authority never exceeds the intersection of platform capability, built-in/plugin manifest request, user grant, action-instance narrowing, workspace policy, and runtime context.

## Representative capabilities

```text
obs.read
obs.control.scene
obs.control.stream
os.keyboard.emit
os.media.emit
os.application.launch
process.execute:<approved executable identity>
clipboard.read | clipboard.write
filesystem.read:<handle> | filesystem.write:<handle>
network.connect:<scheme,host,port>
midi.send:<device> | osc.send:<endpoint>
audio.control:<device>
notification.show
```

`secret.read` is an internal Engine/integration-broker capability and is never grantable to a third-party plugin. Capability additions or widening require a security ADR and human gate.

## Threat controls

| Threat | Mandatory control |
|---|---|
| LAN discovery | No secrets in mDNS; listener disabled until native-LAN consent |
| Pairing MITM/downgrade | Fixed Noise IKpsk2 suite, 32-byte single-use QR PSK, exact prologue, SAS, desktop confirmation, no fallback |
| Established peer spoofing | Noise IK with OS-protected static keys, scoped peer record, pause/revoke/revoke-all |
| Replay/delay | Durable message-ID admission dedupe, sequence tracking, short TTL, no offline command queue |
| Crash around side effect | Durable prepared/result records; `outcome_unknown`; no non-idempotent automatic retry |
| WebView compromise | Tauri capability allowlist, strict CSP, local assets, narrow commands |
| Malicious plugin | Wasmtime isolation, narrow imports, memory/fuel/time limits, signed distribution |
| Plugin secret theft | No raw-secret import; opaque integration connection handles; broker performs approved operation |
| SSRF | Exact network grants, redirect/DNS validation, private-address denial |
| Dangerous process action | Built-in only; no shell; pinned executable identity; clean env/CWD; typed argv; no elevation |
| Executable replacement | Revalidate platform file identity/signature/hash immediately before launch; fail on change |
| Stolen mobile device | OS-protected key, app lock option, revocation, short Cloud session |
| Compromised Cloud | E2EE content, absent local secrets, Engine reauthorization |
| Supply chain | Lockfiles, cargo-deny, SBOM, provenance, signed artifacts |
| Log leakage | Structured allowlist; exclude labels, configs, paths, URLs, tokens, scene names |
| Update compromise | Signed updates, platform signing/notarization, rollback protection |

## Process execution boundary

V1 does not expose a general command runner. A later built-in adapter may execute only an explicitly user-selected executable bound to a platform-stable identity. It accepts a typed argument vector with no interpolation, uses an explicit working directory, inherits no ambient environment or standard handles, cannot request elevation, never invokes a shell (`sh`, `bash`, `cmd`, PowerShell, AppleScript, or equivalents), and revalidates executable identity immediately before launch. Any unsupported platform control fails closed. Remote surfaces may trigger only a preconfigured binding and cannot author or alter executable, arguments, environment, or directory.

## Hard rules

- No homegrown cryptographic primitive or silent suite fallback.
- No remote shell or direct browser-to-LAN control.
- No raw secret in SQLite, sync, logs, telemetry, bundles, crash reports, or plugin memory.
- No unsigned automatic update.
- No success before authoritative Engine result.
- No automatic retry after `outcome_unknown` without adapter idempotency/reconciliation.
- No browser/Cloud statement can substitute for Engine authorization.
- No telemetry without explicit consent.

Security review is mandatory for authentication, networking, pairing, cryptography, OS permissions, secrets, remote control, plugins, billing, updates, signing, privacy, retention, and tenant isolation.
