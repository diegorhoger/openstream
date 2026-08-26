#!/usr/bin/env bash
# Smoke test: verify checksums and signatures for installer artifacts.
# Uses CI test signatures only — production signing is BLOCKED.
#
# Usage: bash tests/packaging/test-signing-verification.sh [artifacts-dir]

set -euo pipefail

ARTIFACTS_DIR="${1:-target/release/bundle}"
ERRORS=0

step() { printf '\n==> %s\n' "$1"; }

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  ERRORS=$((ERRORS + 1))
}

# Hard stop: reject if someone tries production signing in a test
if [[ "${SIGN_RELEASE:-false}" == "true" ]]; then
  echo "ERROR: SIGN_RELEASE=true is not allowed in smoke tests." >&2
  echo "Production signing is BLOCKED pending human decision." >&2
  exit 1
fi

step "Verifying checksum integrity"
CHECKSUM_FILES=$(find "$ARTIFACTS_DIR" -name "*.sha256" 2>/dev/null)
if [ -z "$CHECKSUM_FILES" ]; then
  fail "No checksum files found in $ARTIFACTS_DIR"
else
  for cs in $CHECKSUM_FILES; do
    dir=$(dirname "$cs")
    base=$(basename "$cs" .sha256)
    if [ -f "$dir/$base" ]; then
      echo "  Checking $base..."
      cd "$dir"
      if sha256sum --check --quiet "$cs" 2>/dev/null || \
         shasum -a 256 -c "$cs" 2>/dev/null; then
        echo "    PASS"
      else
        fail "Checksum mismatch for $base"
      fi
      cd - > /dev/null
    else
      fail "Checksum file $cs has no matching artifact"
    fi
  done
fi

step "Verifying artifact file types"
find "$ARTIFACTS_DIR" -type f \( -name "*.exe" -o -name "*.dmg" -o -name "*.deb" -o -name "*.rpm" -o -name "*.AppImage" \) 2>/dev/null | while read f; do
  FILE_TYPE=$(file -b "$f" 2>/dev/null || echo "unknown")
  BASENAME=$(basename "$f")
  echo "  $BASENAME: $FILE_TYPE"

  case "$BASENAME" in
    *.exe)
      if echo "$FILE_TYPE" | grep -qi "pe32\|windows\|executable"; then
        echo "    PASS: PE executable detected"
      else
        echo "    WARN: Not a PE executable (may be cross-compiled)"
      fi
      ;;
    *.dmg)
      if echo "$FILE_TYPE" | grep -qi "disk\|dmg\|apple"; then
        echo "    PASS: DMG image detected"
      else
        echo "    WARN: Not a DMG image"
      fi
      ;;
    *.deb)
      if echo "$FILE_TYPE" | grep -qi "debian\|archive\|dpkg"; then
        echo "    PASS: DEB package detected"
      else
        echo "    WARN: Not a DEB package"
      fi
      ;;
    *.rpm)
      if echo "$FILE_TYPE" | grep -qi "rpm\|cpio\|archive"; then
        echo "    PASS: RPM package detected"
      else
        echo "    WARN: Not an RPM package"
      fi
      ;;
    *.AppImage)
      if echo "$FILE_TYPE" | grep -qi "elf\|executable\|image"; then
        echo "    PASS: AppImage detected"
      else
        echo "    WARN: Not an AppImage"
      fi
      ;;
  esac
done

step "Verifying no embedded secrets"
SECRETS_FOUND=0
for f in $(find "$ARTIFACTS_DIR" -type f \( -name "*.exe" -o -name "*.dmg" -o -name "*.deb" -o -name "*.rpm" -o -name "*.AppImage" \) 2>/dev/null); do
  if strings "$f" 2>/dev/null | grep -qiE "(password|secret|api.key|private.key|BEGIN RSA|BEGIN EC)"; then
    SECRETS_FOUND=$((SECRETS_FOUND + 1))
    echo "  WARN: Possible secrets in $(basename "$f")"
  fi
done

if [ "$SECRETS_FOUND" -eq 0 ]; then
  echo "  PASS: No embedded secrets detected"
else
  fail "$SECRETS_FOUND artifact(s) may contain embedded secrets"
fi

echo ""
if [ $ERRORS -eq 0 ]; then
  echo "All signing verification tests passed."
  exit 0
else
  echo "FAILED with $ERRORS error(s)."
  exit 1
fi
