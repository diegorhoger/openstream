# Upgrade Path: Local-Parity Install to Packaged Install

This document describes the upgrade path from a local source/build (local-parity) installation of OpenStream to the packaged install distributed via the CI artifact pipeline.

## Prerequisites
- Existing local-parity OpenStream installation built from source (branch main or earlier).
- This release package: platform-specific installer (Windows NSIS, macOS DMG, Linux DEB/RPM/AppImage).

## Upgrade Steps

### 1. Export Settings and Profiles
Before uninstalling the local build, export your settings, profiles, and any local bundles.

- Open the local OpenStream desktop app.
- Use File > Export Settings (if available) or manually back up configuration files from the local install directory.
- Note any custom capabilities or profiles configured in the local build.

### 2. Uninstall Previous Local Build (Recommended)
Removing the previous local build avoids conflicts between local source artifacts and packaged binaries.

- Windows: Remove the source build directory and any manual shortcuts.
- macOS: Drag the previous local app bundle to Trash.
- Linux: Remove any manually installed binaries or desktop entries from the source build.

### 3. Install Packaged Release
Run the platform-specific installer from this release.

- Windows: Execute the .exe NSIS installer.
- macOS: Open the .dmg and drag OpenStream to Applications.
- Linux: Install via dpkg -i (DEB), pm -i (RPM), or run the .AppImage directly.

### 4. Verify Installation
After installation, verify the packaged install is functioning correctly.

- Confirm the app launches and displays the version 0.1.0-alpha.
- Run smoke verification tests (see 	ests/packaging/) to confirm artifact integrity.
- Verify checksums (sha256sum -c *.sha256) for the downloaded package.

### 5. Import Settings/Profiles
Import previously exported settings into the packaged install.

- Open the packaged OpenStream app.
- Use File > Import Settings or manually restore backed-up configuration files to the appropriate platform directory (see docs/packaging/auto-update.md for paths).
- Confirm profiles, capabilities, and preferences are restored.

### 6. Confirm Upgrade Success
- Check that no stale artifacts from the previous local build remain.
- Confirm all smoke tests pass (ash tests/packaging/test-installer-output.sh).
- Confirm checksum verification passes (ash tests/packaging/test-signing-verification.sh).
- Confirm uninstall cleanup passes (ash tests/packaging/test-uninstall-cleanup.sh).

## Rollback
If the packaged install does not work correctly:

1. Uninstall the packaged release.
2. Reinstall the previous local build from source (checkout the previous commit SHA).
3. Restore settings/profiles from the backup made in Step 1.
4. Report the issue via GitHub (see docs/engineering/REVIEW_GATES.md).

## Hard Stop
This upgrade path does not include any production deployment, store submission, or production signing actions. Production signing and store submission remain BLOCKED pending separate approval (signing/signing.md, AGENTS.md).
