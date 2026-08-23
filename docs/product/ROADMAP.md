# Canonical roadmap

Roadmap order is dependency order. Agents resume the oldest open pull request first; otherwise they select the lowest-numbered unblocked issue. Dates remain absent until M1 velocity is measured.

| Milestone | Target | Outcome |
|---|---:|---|
| M0 Foundation | v0.1.0 | Product contract, governance, threat model, protocols, design, and build skeleton |
| M1 Local Desktop | v0.2.0-alpha | Useful offline desktop Stream Deck replacement with OBS-first magic loop |
| M2 OSCP Conformance | v0.3.0-contract | Public transport, pairing, simulators, generated clients, and conformance evidence; no phone PWA |
| M3 Cloud Contracts | v0.4.0-contract | Public Cloud contracts, clients, portability suites, and private-release tracking; no hosted source |
| M4 Native Mobile | v0.5.0-beta | Public native iOS/iPadOS and Android clients with secure LAN and optional relay |
| M5 Ecosystem/GA | v1.0.0 | Sandboxed plugins, registry contracts, conformance, audits, and GA |
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

## M2 OSCP, pairing, and simulator conformance

24. Implement the public OSCP codec and local transport testkit.
25. Implement the versioned pairing state machine, identity, revocation, and vectors.
26. Build deterministic Engine/surface simulators and discovery fixtures.
27. Implement resilient-session, bounded-admission, recovery, rate, and backpressure conformance.
28. Generate public Rust/TypeScript/Swift/Kotlin protocol clients.
29. Publish cross-language golden fixtures and portability tests.
30. Complete hostile-network, pairing, and multi-peer simulator security suite.
31. Publish OSCP v0.3 contract and conformance release.

M2 does not ship a browser/PWA LAN surface or claim phone/tablet control. It prepares public contracts and verified components consumed by Stage 2 Cloud and Stage 3 native mobile.

## M3 Public Cloud contracts and private-product tracking

32. Publish tenant, account, session, and device API contracts plus fake service.
33. Implement public E2EE sync client, schemas, vectors, and portability suite.
34. Publish web-editor/account/device generated clients and model portability package.
35. Implement public relay envelope/client, presence contract, and simulator.
36. Publish billing-event and subscription lifecycle contracts in test fixtures only.
37. Implement public entitlement, quota, downgrade, export, and local-forever invariant tests.
38. Publish OAuth credential-residency and provider-client contracts.
39. Publish data export, deletion, retention, and consent portability contracts.
40. Publish privacy-safe telemetry/health contracts and disaster-recovery evidence schema.
41. Complete public Cloud portability, isolation, abuse, and load conformance suite against fakes.
42. Publish v0.4 Cloud contract release and record human-approved opaque private-beta readiness evidence.

No M3 issue authorizes hosted Cloud server, tenancy, billing, operations, private marketplace, deployment, or production credentials in this repository.

## M4 Native mobile

43. Shared Rust mobile protocol and security SDK.
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
53. Signed plugin package and public registry/moderation contracts.
54. Starter integrations and migration documentation.
55. Cross-platform conformance and performance suite.
56. GA accessibility, privacy, dependency, and security audit.
57. Publish v1.0.0.

## M6 Optional open hardware

58. Open physical-device protocol v1.
59. Reference firmware and simulator.
60. Reference PCB, BOM, enclosure, and assembly guide.
61. Hardware/software conformance alpha.

Issue bodies define exact dependencies and evidence. Later milestones do not authorize premature implementation.
