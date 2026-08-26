#!/usr/bin/env bash
# CI test GPG key generation for Linux artifact signing.
# Generates a throwaway GPG key for smoke testing only.
#
# This script is for CI smoke tests. It does NOT sign release artifacts.
# Production signing is BLOCKED pending human decision on key management.
#
# Usage: bash signing/ci/generate-test-key.sh

set -euo pipefail

if [[ "${SIGN_RELEASE:-false}" == "true" ]]; then
  echo "ERROR: SIGN_RELEASE=true is not allowed in CI test signing." >&2
  echo "Production signing is BLOCKED pending human decision." >&2
  exit 1
fi

KEYRING_DIR=$(mktemp -d)
KEYRING="$KEYRING_DIR/gpg-test-keyring"
KEY_FILE="$KEYRING_DIR/test-key.asc"

export GNUPGHOME="$KEYRING"

cat > "$KEYRING_DIR/key-params" <<'EOF'
%no-protection
Key-Type: RSA
Key-Length: 4096
Subkey-Type: RSA
Subkey-Length: 4096
Name-Real: OpenStream Test
Name-Email: test-ci@openstream.dev
Expire-Date: 1d
%commit
EOF

echo "Generating test GPG key..."
gpg --batch --gen-key "$KEYRING_DIR/key-params"
gpg --armor --export > "$KEY_FILE"

echo "Test GPG key generated: $KEY_FILE"
echo "Key fingerprint:"
gpg --fingerprint test-ci@openstream.dev

echo ""
echo "NOTE: This is a CI test key, NOT a production key."
echo "Keyring directory: $KEYRING_DIR"
echo "To sign an artifact: gpg --batch --armor --detach-sign --output <artifact>.sig <artifact>"
