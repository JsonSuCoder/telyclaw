#!/usr/bin/env bash
set -euo pipefail

# Build a distributable OpenClaw runtime folder for embedding into Telegram-TT.
# Usage:
#   bash src/openclaw/scripts/build-openclaw-runtime.sh [target-id]
# Example:
#   bash src/openclaw/scripts/build-openclaw-runtime.sh mac-arm64

TARGET_ID="${1:-mac-arm64}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="${PROJECT_ROOT:-$(cd "$SCRIPT_DIR/../../.." && pwd)}"
OPENCLAW_SRC="${OPENCLAW_SRC:-$PROJECT_ROOT/../openclaw}"
OUT_DIR="${OUT_DIR:-$PROJECT_ROOT/vendor/openclaw-runtime/$TARGET_ID}"

TARGET_PLATFORM="${TARGET_ID%%-*}"
TARGET_ARCH="${TARGET_ID#*-}"
if [[ "$TARGET_PLATFORM" == "$TARGET_ID" || -z "$TARGET_ARCH" ]]; then
  echo "Invalid target id: $TARGET_ID (expected <platform>-<arch>, e.g. mac-arm64, win-x64, linux-x64)" >&2
  exit 1
fi

case "$TARGET_PLATFORM" in
  mac)
    NPM_TARGET_PLATFORM="darwin"
    ;;
  win)
    NPM_TARGET_PLATFORM="win32"
    ;;
  linux)
    NPM_TARGET_PLATFORM="linux"
    ;;
  *)
    echo "Unsupported target platform in TARGET_ID: $TARGET_PLATFORM" >&2
    exit 1
    ;;
esac

case "$TARGET_ARCH" in
  x64|arm64|ia32)
    NPM_TARGET_ARCH="$TARGET_ARCH"
    ;;
  *)
    echo "Unsupported target arch in TARGET_ID: $TARGET_ARCH" >&2
    exit 1
    ;;
esac

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

need_cmd node
need_cmd npm

if [[ ! -d "$OPENCLAW_SRC" ]]; then
  echo "OPENCLAW_SRC does not exist: $OPENCLAW_SRC" >&2
  exit 1
fi

# Check if pnpm is available, fallback to npm
if command -v pnpm >/dev/null 2>&1; then
  PKG_MANAGER="pnpm"
else
  PKG_MANAGER="npm"
fi

echo "[1/4] Building OpenClaw from source: $OPENCLAW_SRC"
pushd "$OPENCLAW_SRC" >/dev/null

# Install dependencies
if [[ "$PKG_MANAGER" == "pnpm" ]]; then
  corepack enable >/dev/null 2>&1 || true
  pnpm install --frozen-lockfile || pnpm install
else
  npm install --legacy-peer-deps || npm install --force
fi

# Build OpenClaw
if [[ "$PKG_MANAGER" == "pnpm" ]]; then
  pnpm build
  pnpm ui:build
else
  npm run build
  npm run ui:build
fi

echo "[2/4] Preparing output runtime dir"
rm -rf "$OUT_DIR"
mkdir -p "$(dirname "$OUT_DIR")"

# Copy the built OpenClaw to output directory
cp -R "$OPENCLAW_SRC" "$OUT_DIR"

echo "[3/4] Installing production dependencies"
pushd "$OUT_DIR" >/dev/null
rm -rf node_modules package-lock.json pnpm-lock.yaml

# Remove dev dependencies to avoid conflicts
if command -v npm >/dev/null 2>&1; then
  npm pkg delete devDependencies >/dev/null 2>&1 || true
fi

echo "[openclaw-runtime] npm target platform=$NPM_TARGET_PLATFORM arch=$NPM_TARGET_ARCH"
NPM_CONFIG_LEGACY_PEER_DEPS=true \
npm_config_platform="$NPM_TARGET_PLATFORM" \
npm_config_arch="$NPM_TARGET_ARCH" \
npm install --omit=dev --no-audit --no-fund --legacy-peer-deps || \
npm install --omit=dev --no-audit --no-fund --force

# Runtime sanity checks
[[ -f "openclaw.mjs" ]] || [[ -f "dist/entry.js" ]] || [[ -f "dist/entry.mjs" ]]
[[ -d "node_modules" ]]
popd >/dev/null

echo "[4/4] Verifying runtime layout"
[[ -d "$OUT_DIR/node_modules" ]]

popd >/dev/null

echo "[4/4] Done"
echo "Runtime output: $OUT_DIR"