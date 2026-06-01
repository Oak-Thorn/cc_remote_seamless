#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SIDECAR_DIR="$PROJECT_ROOT/sidecar/feishu-gateway"

# Detect target triple
ARCH=$(uname -m)
case "$ARCH" in
    arm64) TARGET_TRIPLE="aarch64-apple-darwin" ;;
    x86_64) TARGET_TRIPLE="x86_64-apple-darwin" ;;
    *) echo "Unsupported arch: $ARCH"; exit 1 ;;
esac

echo "=== CC Remote Seamless Build ==="
echo "  Target: $TARGET_TRIPLE"
echo "  Project: $PROJECT_ROOT"
echo ""

# Step 1: Build Go sidecar
echo "[1/3] Building feishu-gateway sidecar..."
cd "$SIDECAR_DIR"
CGO_ENABLED=0 go build -o "feishu-gateway-${TARGET_TRIPLE}" .
echo "  -> feishu-gateway-${TARGET_TRIPLE}"

# Step 2: Build frontend
echo "[2/3] Building Vue frontend..."
cd "$PROJECT_ROOT"
npm run build

# Step 3: Build Tauri app
echo "[3/3] Building Tauri application..."
cd "$PROJECT_ROOT"
npm run tauri build

echo ""
echo "=== Build Complete ==="
echo "  App: src-tauri/target/release/bundle/macos/CC Remote Seamless.app"
echo "  DMG: src-tauri/target/release/bundle/dmg/"
