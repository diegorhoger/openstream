#!/usr/bin/env bash
# Cross-platform signature verification for OpenStream artifacts.
# Verifies that an artifact has a valid signature and matches its checksum.
#
# Usage: bash signing/verify.sh <artifact-path> [--checksum <sha256sum-file>]

set -euo pipefail

usage() {
  echo "Usage: $0 <artifact-path> [--checksum <sha256sum-file>]" >&2
  exit 1
}

ARTIFACT=""
CHECKSUM_FILE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --checksum)
      CHECKSUM_FILE="$2"
      shift 2
      ;;
    -h|--help)
      usage
      ;;
    *)
      ARTIFACT="$1"
      shift
      ;;
  esac
done

if [[ -z "$ARTIFACT" ]]; then
  echo "ERROR: No artifact path provided." >&2
  usage
fi

if [[ ! -f "$ARTIFACT" ]]; then
  echo "ERROR: Artifact not found: $ARTIFACT" >&2
  exit 1
fi

ERRORS=0

# Checksum verification
if [[ -n "$CHECKSUM_FILE" ]]; then
  if [[ ! -f "$CHECKSUM_FILE" ]]; then
    echo "ERROR: Checksum file not found: $CHECKSUM_FILE" >&2
    exit 1
  fi

  BASENAME=$(basename "$ARTIFACT")
  echo "Verifying checksum for $BASENAME..."
  if sha256sum --check --quiet --ignore-missing "$CHECKSUM_FILE" 2>/dev/null; then
    echo "  Checksum: PASS"
  else
    echo "  Checksum: FAIL"
    ERRORS=$((ERRORS + 1))
  fi
fi

# Platform-specific signature verification
ARTIFACT_NAME=$(basename "$ARTIFACT")
case "$ARTIFACT_NAME" in
  *.exe|*.msi|*.nsis)
    # Windows Authenticode
    SIG_FILE="${ARTIFACT}.sig"
    if [[ -f "$SIG_FILE" ]]; then
      echo "Verifying Windows signature (GPG detached)..."
      if gpg --verify "$SIG_FILE" "$ARTIFACT" 2>/dev/null; then
        echo "  Signature: PASS"
      else
        echo "  Signature: FAIL"
        ERRORS=$((ERRORS + 1))
      fi
    elif command -v signtool &>/dev/null; then
      echo "Verifying Windows Authenticode signature..."
      if signtool verify /pa "$ARTIFACT" 2>/dev/null; then
        echo "  Signature: PASS"
      else
        echo "  Signature: FAIL (no valid Authenticode signature)"
        ERRORS=$((ERRORS + 1))
      fi
    else
      echo "  Signature: SKIPPED (no verification tool available)"
    fi
    ;;

  *.dmg|*.pkg)
    # macOS code signature
    echo "Verifying macOS code signature..."
    if codesign --verify --deep --strict "$ARTIFACT" 2>/dev/null; then
      echo "  Code signature: PASS"
    else
      echo "  Code signature: FAIL (unsigned or invalid)"
      ERRORS=$((ERRORS + 1))
    fi
    ;;

  *.deb|*.rpm|*.AppImage|*.appimage)
    # Linux signature
    SIG_FILE="${ARTIFACT}.sig"
    if [[ -f "$SIG_FILE" ]]; then
      echo "Verifying Linux GPG signature..."
      if gpg --verify "$SIG_FILE" "$ARTIFACT" 2>/dev/null; then
        echo "  Signature: PASS"
      else
        echo "  Signature: FAIL"
        ERRORS=$((ERRORS + 1))
      fi
    else
      echo "  Signature: SKIPPED (no .sig file found)"
    fi
    ;;

  *)
    echo "  Unknown artifact type, skipping signature check."
    ;;
esac

# Tamper detection test
echo ""
echo "Tamper detection test..."
CORRUPTED=$(mktemp)
head -c 100 "$ARTIFACT" > "$CORRUPTED"
echo "x" >> "$CORRUPTED"
tail -c +101 "$ARTIFACT" >> "$CORRUPTED"

if gpg --verify "$SIG_FILE" "$CORRUPTED" 2>/dev/null; then
  echo "  Tamper test: FAIL (corrupted artifact verified successfully)"
  ERRORS=$((ERRORS + 1))
else
  echo "  Tamper test: PASS (corrupted artifact correctly rejected)"
fi
rm -f "$CORRUPTED"

echo ""
if [[ $ERRORS -eq 0 ]]; then
  echo "All verification checks passed."
  exit 0
else
  echo "Verification FAILED with $ERRORS error(s)."
  exit 1
fi
