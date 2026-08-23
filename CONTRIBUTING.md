# Contributing

Thank you for helping build OpenStream.

1. Search existing issues and claim one bounded issue before material changes.
2. Branch from current `main`; use one issue per branch and pull request.
3. Keep production authority in Rust unless a documented platform/UI boundary requires otherwise.
4. Add deterministic success, failure, crash-window, and denial tests as applicable.
5. Never add telemetry by default, secrets, proprietary assets, undocumented permissions, or hosted Cloud implementation to this public repository.
6. Add an ADR for a public protocol, irreversible architecture, security boundary, source boundary, dependency policy, or migration decision.
7. Run repository checks and include exact commands, results, and SHA in the PR.
8. Sign off every commit under the Developer Certificate of Origin with `git commit -s`.
9. Use distinct planner, implementer, verifier, reviewer, and evaluator context IDs in the PR provenance block.

The initial unsigned foundation commit predates DCO enforcement and remains an unresolved bootstrap gate. No exception is implied until the maintainer selects and executes Decision C from PR #62.

Security vulnerabilities must be reported privately as described in `SECURITY.md`, not in public issues.
