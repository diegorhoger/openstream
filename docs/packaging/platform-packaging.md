# Platform Packaging Guide

OpenStream ships as platform-specific installers built by Tauri 2. This
document describes each platform artifact, its configuration, and testing.

## Artifacts

| Platform | Format | Target | Install Mode |
|----------|--------|--------|--------------|
| Windows | NSIS installer | x86_64-pc-windows-msvc | Per-user + per-machine |
| Windows | MSI installer | x86_64-pc-windows-msvc | Per-machine |
| macOS | DMG | aarch64-apple-darwin, x86_64-apple-darwin | Drag-to-Applications |
| Linux | DEB | x86_64-unknown-linux-gnu | apt/dpkg |
| Linux | RPM | x86_64-unknown-linux-gnu | yum/dnf |
| Linux | AppImage | x86_64-unknown-linux-gnu | Portable |

## Windows (NSIS + MSI)

### NSIS Installer
- Supports both per-user and per-machine installation
- Creates Start Menu and Desktop shortcuts
- Includes uninstaller
- Custom icon and branding

### MSI Installer
- Group Policy deployment support
- Per-machine installation only
- Standard Windows Installer rollback support

### Build
```bash
# Windows native
cargo tauri build --target x86_64-pc-windows-msvc

# Cross-compile from Linux (requires wine + cross toolchain)
cargo tauri build --target x86_64-pc-windows-msvc --runner cross
```

## macOS (DMG)

### Features
- Drag-to-Applications installation
- Standard macOS app bundle structure
- Code-signed with Developer ID (production)
- Notarized with Apple (production)

### Build
```bash
# Native macOS
cargo tauri build --target aarch64-apple-darwin
cargo tauri build --target x86_64-apple-darwin

# Universal binary
cargo tauri build --target universal2-apple-darwin
```

### Minimum System Version
- macOS 10.15 (Catalina) or later

## Linux (DEB/RPM/AppImage)

### DEB Package
- Dependencies: libgtk-3-0, libwebkit2gtk-4.1-0, libayatana-appindicator3-1, librsvg2-0, libxdo0
- Section: net
- Priority: optional

### RPM Package
- Dependencies: gtk3, webkit2gtk4.1, libappindicator-gtk3, librsvg2, libxdo

### AppImage
- Self-contained, no system dependencies required
- Includes bundled libraries
- Portable — runs from any location

### Build
```bash
# Native Linux
cargo tauri build --target x86_64-unknown-linux-gnu

# Specific format
cargo tauri build --bundles deb
cargo tauri build --bundles rpm
cargo tauri build --bundles appimage
```

## Cross-Compilation

### From Linux to Windows
Requires cross-rs and wine:
```bash
cargo install cross --git https://github.com/cross-rs/cross
cross build --target x86_64-pc-windows-msvc
```

### From Linux to macOS
Requires osxcross:
```bash
# Not officially supported in CI; use macOS runners
```

## CI Packaging

The CI packaging job (`.github/workflows/package.yml`) builds platform artifacts
on the appropriate runner:
- Windows: `windows-latest`
- macOS: `macos-latest`
- Linux: `ubuntu-latest`

Artifacts are uploaded as GitHub Actions artifacts with 7-day retention.

## Smoke Tests

Installer smoke tests are in `tests/packaging/`:
- `test-installer-output.sh` — verifies artifacts exist and are non-empty
- `test-signing-verification.sh` — verifies checksums and signatures
- `test-uninstall-cleanup.sh` — verifies uninstall removes all artifacts

Run locally:
```bash
bash tests/packaging/test-installer-output.sh
bash tests/packaging/test-signing-verification.sh
bash tests/packaging/test-uninstall-cleanup.sh
```

## Documentation

- `docs/packaging/platform-packaging.md` — this document
- `docs/packaging/auto-update.md` — auto-update architecture
- `signing/signing.md` — code signing framework
