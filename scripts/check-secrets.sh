#!/usr/bin/env bash
# Fail if tracked files contain likely private network data or credentials.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

pattern='(192\.168\.|10\.[0-9]+\.[0-9]+\.[0-9]+|172\.(1[6-9]|2[0-9]|3[01])\.|camarasN007|password[[:space:]]*=[[:space:]]*"[^"]+")'

if git grep -nEI "$pattern" -- \
    ':!Cargo.lock' \
    ':!docs/example-config.toml' \
    ':!scripts/check-secrets.sh'; then
    echo
    echo "Potential private data or credentials found. Remove them before committing." >&2
    exit 1
fi

echo "Secret scan: no obvious private data or credentials found."
