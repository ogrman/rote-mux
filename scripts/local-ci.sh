#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "==> Running build..."
"$SCRIPT_DIR/build.sh"

echo "==> Checking formatting..."
"$SCRIPT_DIR/fmt.sh"

echo "==> Running clippy..."
"$SCRIPT_DIR/clippy.sh"

echo "==> Running tests..."
"$SCRIPT_DIR/test.sh"

echo "==> All checks passed!"
