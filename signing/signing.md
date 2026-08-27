# Code Signing Framework

This document describes the OpenStream code signing framework. **Signing is NOT
activated in CI or production.** Only CI test signing (self-signed certificates)
is used for smoke tests.

## Status

- [x] Signing framework configured
- [x] CI test signing (self-signed certs) for smoke tests
- [ ] Production signing BLOCKED — pending human decision on key storage/CA
- [ ] macOS notarization — BLOCKED — pending Apple Developer credentials
- [ ] Windows EV signing — BLOCKED — pending HSM/HSM token decision

## Platforms

### Windows (Authenticode)

- **Tool:** `signtool.exe` (Windows SDK) or `osslsigncode` (cross-platform)
- **CI test:** Self-signed certificate generated in CI via `New-SelfSignedCertificate`
- **Production:** BLOCKED — pending decision on:
  - Key storage: cloud HSM (Azure Key Vault / AWS CloudHSM) vs. local HSM
  - Certificate authority: DigiCert / Sectigo / other
  - Signing flow: direct upload vs. CI gateway service

### macOS (Apple code signing + notarization)

- **Tool:** `codesign` + `notarytool` (Xcode CLI tools)
- **CI test:** N/A (cross-signed in CI on Linux)
- **Production:** BLOCKED — pending:
  - Apple Developer Program membership
  - Developer ID Application certificate
  - App-specific password for notarization
  - Keychain storage strategy (CI keychain vs. CI gateway)

### Linux (GPG signing)

- **Tool:** GnuPG for detached signatures
- **CI test:** Self-generated GPG key in CI
- **Production:** BLOCKED — pending:
  - GPG key management strategy
  - Key distribution (keyserver vs. bundled)

## CI Integration

The CI packaging job (`package.yml`) performs:

1. Build platform-specific artifacts (unsigned by default)
2. Generate checksums (SHA-256)
3. CI test signing (self-signed certs) for smoke tests
4. Upload artifacts

Production signing is gated behind GitHub secrets and an explicit
`SIGN_RELEASE=true` flag that is **never set in CI today**.

## Files

- `signing/signing.md` — this document
- `signing/ci/test-sign.ps1` — Windows CI test signing script
- `signing/ci/generate-test-key.sh` — Linux CI test GPG key generation
- `signing/verify.sh` — cross-platform signature verification

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SIGN_RELEASE` | `false` | Must be `true` to activate production signing |
| `WINDOWS_CERTIFICATE_PATH` | — | Path to `.pfx` file (Windows signing) |
| `WINDOWS_CERTIFICATE_PASSWORD` | — | Password for `.pfx` file |
| `MACOS_SIGNING_IDENTITY` | — | Developer ID identity string |
| `MACOS_NOTARIZATION_APPLE_ID` | — | Apple ID for notarization |
| `MACOS_NOTARIZATION_PASSWORD` | — | App-specific password |
| `MACOS_NOTARIZATION_TEAM_ID` | — | Apple Developer Team ID |
| `GPG_PRIVATE_KEY` | — | ASCII-armored GPG private key (Linux signing) |
| `GPG_PASSPHRASE` | — | Passphrase for GPG key |
