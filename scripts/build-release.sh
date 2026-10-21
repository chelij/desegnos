#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

# Non-interactive shells may not have mise's Node installation on PATH.
if ! command -v node >/dev/null 2>&1; then
  export PATH="$(dirname -- "$(mise which node)"):$PATH"
fi

# Tauri embeds the freshly built UI and keeps the default portal-dialogs feature.
node scripts/tauri.mjs build --no-bundle \
  --config '{"build":{"beforeBuildCommand":"npm run build"}}'

stat -c '%n: %s bytes; modified %y' target/release/desegnos
node scripts/startup-test.mjs --binary "$PWD/target/release/desegnos"
