# Install and Smoke Verification

This document describes how to verify a packaged installation of OpenStream v0.1.0-alpha.

## Verification Steps

### 1. Download Artifacts
Download the platform-specific artifact from the CI pipeline or release tag.
- Windows: .exe installer
- macOS: .dmg image
- Linux: .deb, .rpm, or .AppImage

### 2. Verify Checksums
Every artifact is accompanied by a .sha256 file. Verify before installation:

`ash
sha256sum -c *.sha256
`

Or on macOS:

`ash
shasum -a 256 -c *.sha256
`

### 3. Verify Test Signature (Smoke Only)
For smoke tests, test signatures may be applied using signing/ci/test-sign.ps1 (Windows) or signing/ci/generate-test-key.sh (Linux). Production signatures are BLOCKED.

### 4. Run Smoke Tests
Run the packaging smoke tests to confirm artifact integrity and install behavior:

`ash
bash tests/packaging/test-installer-output.sh
bash tests/packaging/test-signing-verification.sh
bash tests/packaging/test-uninstall-cleanup.sh
`

### 5. Confirm No Embedded Secrets
Smoke tests include a check for embedded secrets (	ests/packaging/test-signing-verification.sh). Confirm no secrets are detected.

### 6. Confirm Artifact Non-Empty
Artifacts should be larger than 1KB (verified by 	ests/packaging/test-installer-output.sh).

### 7. Confirm Checksum Integrity
All .sha256 files must match their artifacts. Any mismatch indicates corruption or tampering.

### 8. Confirm Uninstall Scripts
The repository includes uninstall-related files. Confirm uninstall commands exist or that the platform installer supports removal.

## Evidence Requirements
Verification must record:
- Platform
- Artifact filenames and checksums
- Commit SHA of the source
- Results of smoke tests (PASS/FAIL for each)
- Any errors or warnings

See AGENTS.md for evidence recording requirements.

## Hard Stop
No installation verification replaces production security review. Production signing, deployment, and store submission remain BLOCKED and require separate approval (AGENTS.md, signing/signing.md).
