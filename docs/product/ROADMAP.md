# Canonical roadmap

Roadmap order is dependency order. Agents resume the oldest open pull request first; otherwise they select the lowest-numbered unblocked issue. Dates are intentionally absent until M1 velocity is measured.

| Milestone | Target | Outcome |
|---|---:|---|
| M0 Foundation | v0.1.0 | Product contract, governance, threat model, protocols, design, and deterministic build skeleton |
| M1 Local Desktop | v0.2.0-alpha | Useful local Stream Deck replacement with OBS-first magic loop |
| M2 LAN Companion | v0.3.0-beta | Secure browser/PWA control from a supporting phone or tablet |
| M3 Cloud | v0.4.0-private-beta | Accounts, E2EE sync, web editor, relay, teams, and subscriptions |
| M4 Native Mobile | v0.5.0-beta | Native iOS/iPadOS and Android surfaces |
| M5 Ecosystem/GA | v1.0.0 | Sandboxed plugins, registry, conformance, audits, and GA |
| M6 Open Hardware | v1.1.0-alpha | Optional open device protocol, firmware, PCB/BOM, and reference device |

## M0 Foundation

1. Bootstrap product charter and repository governance.
2. Threat model and capability taxonomy.
3. Versioned domain model and action protocol ADR.
4. Rust-first monorepo and Tauri build scaffold.
5. Design tokens and accessibility contract.
6. Deterministic quality and release harness.

## M1 Local Desktop

7. Profiles, pages, folders, and controls model.
8. Capability grants and OS secret storage.
9. Action runtime and registry.
10. Keyboard shortcut action.
11. Application, file, and URL launch actions.
12. Media and volume actions.
13. OBS WebSocket discovery, actions, and live state.
14. Sequencing, delay, conditions, cancellation, and failure semantics.
15. Local persistence and schema migrations.
16. Desktop shell, tray, autostart, and single-instance behavior.
17. Visual deck editor.
18. Live deck surface and action-state feedback.
19. Profile switching and global hotkeys.
20. Portable import/export and local backup.
21. Privacy-safe diagnostics and support bundle.
22. Installers, verified updates, checksums, and signing plan.
23. Publish local desktop alpha.

## M2 LAN Companion

24. Versioned local control transport.
25. Pairing, revocation, and device identity.
26. Local discovery and manual fallback.
27. Resilient sessions, idempotency, rate limits, and backpressure.
28. Installable companion PWA shell.
29. Phone/tablet deck UI.
30. LAN security and multi-device end-to-end suite.
31. Publish LAN companion beta.

## M3 Subscription Web

32. Cloud tenancy and account authentication.
33. End-to-end encrypted synchronization.
34. Web editor and account/device dashboard.
35. Secure remote relay and presence.
36. Subscription billing integration.
37. Entitlements, quotas, downgrade behavior, and customer portal.
38. Streaming-provider OAuth credential framework.
39. Data export, deletion, retention, and consent.
40. Observability, backups, disaster recovery, and incident runbooks.
41. Isolation, abuse, security, and load audit.
42. Publish Cloud private beta.

## M4 Native Mobile

43. Shared mobile protocol SDK.
44. Native iOS/iPadOS shell.
45. Native Android shell.
46. Offline cache and reconnect reconciliation.
47. Native deck UX, accessibility, and haptics.
48. OS lifecycle, background, and notification behavior.
49. Mobile privacy and security review.
50. Store packaging, signing, and beta distribution.
51. Publish native mobile beta.

## M5 Ecosystem and v1

52. Sandboxed plugin SDK.
53. Signed plugin package, registry, moderation, and revocation.
54. Starter integrations and migration documentation.
55. Cross-platform conformance and performance suite.
56. GA accessibility, privacy, dependency, and security audit.
57. Publish v1.0.0.

## M6 Optional Open Hardware

58. Open physical-device protocol v1.
59. Reference firmware and simulator.
60. Reference PCB, BOM, enclosure, and assembly guide.
61. Hardware/software conformance alpha.

Issue bodies define exact dependencies and acceptance evidence. Later milestones do not authorize premature implementation.
