# Security and capability model

OpenStream is deny-by-default and host-authoritative. Effective authority must never exceed the intersection of the platform capability, plugin manifest request, user grant, action-instance narrowing, workspace policy, and current runtime context.

## Representative capabilities

```text
obs.read
obs.control.scene
obs.control.stream
os.keyboard.emit
os.media.emit
os.application.launch
process.execute:<approved executable>
clipboard.read | clipboard.write
filesystem.read:<handle> | filesystem.write:<handle>
network.connect:<scheme,host,port>
midi.send:<device> | osc.send:<endpoint>
audio.control:<device>
secret.read:<named reference>
notification.show
```

Capability additions or widening require a security ADR and human gate.

## Threat controls

| Threat | Mandatory control |
|---|---|
| LAN discovery | No secrets in mDNS; pairing disabled until user enables LAN |
| Pairing MITM | QR certificate pin, standardized authenticated transcript, matching SAS, desktop confirmation |
| Replay/delay | Message-ID dedupe, sequence tracking, short TTL |
| WebView compromise | Tauri capability allowlist, strict CSP, local assets, narrow commands |
| Malicious plugin | Wasmtime isolation, narrow imports, memory/fuel/time limits, signed distribution |
| SSRF | Exact network grants, redirect/DNS validation, private-address denial |
| Dangerous process action | Built-in only, executable allowlist, argument preview, explicit grant |
| Stolen device | OS-protected key, app lock option, revocation, short Cloud session |
| Compromised Cloud | E2EE content, absent local secrets, Engine reauthorization |
| Supply chain | Lockfiles, cargo-deny, SBOM, provenance, signed artifacts |
| Log leakage | Structured allowlist; exclude labels, configs, paths, URLs, tokens, scene names |
| Update compromise | Signed updates, platform signing/notarization, rollback protection |

## Hard rules

- No homegrown cryptographic primitive.
- No remote shell.
- No raw secret in SQLite, sync, logs, telemetry, bundles, or crash reports.
- No unsigned automatic update.
- No success before Engine acknowledgement.
- No browser/cloud statement can substitute for Engine authorization.
- No telemetry without explicit consent.

## Security review triggers

Security review is mandatory for authentication, network exposure, pairing, cryptography, OS permissions, secrets, remote control, plugins, billing, updates, signing, privacy, data retention, and tenant isolation.
