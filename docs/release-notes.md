# OpenStream v0.1.0-alpha Release Notes

Release date: 2026-08-27
Tag: v0.1.0-alpha
Branch: m1/release-alpha
Target SHA: see CI artifact manifest

## Scope
First public alpha desktop build. Versions all artifacts for Windows, macOS, and Linux platforms.

## Dependencies
Requires merged issues: #17 (studio-editor), #18 (desktop-surface), #19 (profiles-hotkeys), #20 (portability), #21 (diagnostics), #22 (desktop-release-system).

## Platforms
- Windows: NSIS installer (x86_64-pc-windows-msvc)
- macOS: DMG (aarch64-apple-darwin)
- Linux: DEB, RPM, AppImage (x86_64-unknown-linux-gnu)

## Signing
- Production signing BLOCKED per signing/signing.md and AGENTS.md hard stop.
- CI test signing uses self-signed certificates (signing/ci/test-sign.ps1, signing/ci/generate-test-key.sh) for smoke verification only.
- No production Authenticode, Apple Developer ID, or GPG production keys activated.

## Checksums
Every artifact has an individual .sha256 file. A combined manifest (artifact-checksums.txt) is generated during CI packaging.

## Upgrade Path
Users upgrading from a local-parity install (local source/build) to the packaged install should:
1. Export settings/profiles from the current desktop install (see docs/upgrade-local-to-packaged.md).
2. Uninstall any previous local build (recommended but not required).
3. Install the platform-specific package from this release.
4. Import settings/profiles into the packaged install.
5. Verify installation with smoke tests (tests/packaging/).

## Known Limitations
- Auto-update is wired but NOT activated (docs/packaging/auto-update.md).
- Production signing, notarization, and store submission are BLOCKED.
- No cloud, native mobile, or production-wide rollout included.
- CI artifacts are unsigned except for smoke-test signatures.

## Security
- No embedded secrets in artifacts (verified by tests/packaging/test-signing-verification.sh).
- No production signing keys in CI.
- All permissions are deny-by-default per Rust crate unsafe_code = forbidden policy.
