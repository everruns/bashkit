#!/usr/bin/env bash
# Smoke test: build, pack, install in isolated dir, run tests
set -e

cd "$(dirname "$0")/.."

echo "=== Building ==="
npm run build

# Find the .node file
NODE_FILE=$(ls bashkit.*.node 2>/dev/null | head -1)
if [ -z "$NODE_FILE" ]; then
  echo "ERROR: No .node file found after build"
  exit 1
fi
echo "Built: $NODE_FILE"

# Extract platform from filename (e.g., bashkit.darwin-arm64.node -> darwin-arm64)
PLATFORM=$(echo "$NODE_FILE" | sed 's/bashkit\.\(.*\)\.node/\1/')
echo "Platform: $PLATFORM"

echo "=== Creating npm dirs ==="
npm run create-npm-dirs

# Copy binary to platform package
PLATFORM_DIR="npm/${PLATFORM}"
if [ ! -d "$PLATFORM_DIR" ]; then
  echo "ERROR: Platform dir $PLATFORM_DIR not found"
  exit 1
fi
cp "$NODE_FILE" "$PLATFORM_DIR/"

echo "=== Preparing packages ==="
npx napi prepublish --skip-gh-release

# Pack platform package
PLATFORM_TGZ=$(cd "$PLATFORM_DIR" && npm pack 2>/dev/null | tail -1)
PLATFORM_TGZ_PATH="$(cd "$PLATFORM_DIR" && pwd)/$PLATFORM_TGZ"

# Pack main package
MAIN_TGZ=$(npm pack 2>/dev/null | tail -1)
MAIN_TGZ_PATH="$(pwd)/$MAIN_TGZ"

echo "=== Installing in smoke-test dir ==="
SMOKE_DIR="smoke-test"
rm -rf "$SMOKE_DIR"
mkdir -p "$SMOKE_DIR"
cd "$SMOKE_DIR"

npm init -y > /dev/null 2>&1
npm install "$PLATFORM_TGZ_PATH" "$MAIN_TGZ_PATH"

echo "=== Running smoke test ==="
node -e "
const { BashTool, getVersion } = require('@everruns/bashkit');
const tool = new BashTool();
console.log('Version:', getVersion());
const r = tool.executeSync('echo hello');
console.log('stdout:', JSON.stringify(r.stdout));
console.assert(r.exit_code === 0, 'Expected exit_code 0');
console.assert(r.stdout.trim() === 'hello', 'Expected hello');
console.log('Smoke test PASSED');
"

echo "=== Cleanup ==="
cd ..
rm -rf smoke-test npm
rm -f bashkit.*.node *.tgz
# Restore any package.json changes from napi prepublish
git checkout package.json 2>/dev/null || true

echo "=== Done ==="
