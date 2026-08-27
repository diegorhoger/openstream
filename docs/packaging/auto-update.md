# Auto-Update Scaffolding

This document describes the OpenStream auto-update architecture. The scaffolding
is in place but **not wired live**. No updater is active in production.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    OpenStream Desktop                        │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Tauri Updater Plugin (tauri-plugin-updater)          │  │
│  │  - Checks endpoint for latest version                 │  │
│  │  - Downloads update bundle                            │  │
│  │  - Verifies Ed25519 signature                         │  │
│  │  - Applies update with rollback support               │  │
│  └──────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                              │
                    ┌─────────▼─────────┐
                    │  Update Endpoint   │
                    │  (GitHub Releases  │
                    │   or CDN)          │
                    └───────────────────┘
```

## Update Flow

1. App checks update endpoint on launch (configurable interval)
2. Endpoint returns JSON with version, release notes, download URL, signature
3. App verifies Ed25519 signature against embedded public key
4. App downloads update bundle
5. App applies update (platform-specific)
6. App restarts into new version

## Rollback Support

- Update bundles include a backup of the previous version
- If the new version fails to start, the updater rolls back automatically
- Rollback state is stored in platform-specific locations:
  - Windows: `%APPDATA%/openstream/updater/`
  - macOS: `~/Library/Application Support/openstream/updater/`
  - Linux: `~/.config/openstream/updater/`

## Configuration

The updater is configured in `tauri.conf.json`:

```json
{
  "plugins": {
    "updater": {
      "pubkey": "<Ed25519 public key>",
      "endpoints": [
        "https://api.openstream.dev/updates/{{target}}/{{arch}}/{{current_version}}"
      ]
    }
  }
}
```

## Platform-Specific Notes

### Windows
- NSIS installer supports in-place updates
- MSI installer requires full reinstall (update via NSIS recommended)
- Windows Defender may flag unsigned updates — signing is required for production

### macOS
- DMG updates require notarization for Gatekeeper
- App must be code-signed for auto-update to work
- Updates are applied via `tauri-plugin-updater` with bundled signature

### Linux
- AppImage updates are self-contained
- DEB/RPM updates use package manager integration
- Flatpak/Snap updates follow their respective channels

## Production Activation (BLOCKED)

Production activation requires:
1. Ed25519 key pair generation and secure storage
2. Update endpoint deployment (GitHub Releases or CDN)
3. Platform signing (Windows Authenticode, macOS notarization, Linux GPG)
4. Update URL configuration in `tauri.conf.json`
5. Rollback testing across all platforms

**Status:** BLOCKED — pending signing framework completion and key storage decision.

## Files

- `docs/packaging/auto-update.md` — this document
- `apps/desktop/src-tauri/tauri.conf.json` — updater configuration (endpoints empty)
- `signing/signing.md` — signing framework for update verification
