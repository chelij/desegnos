#!/usr/bin/env bash
set -euo pipefail
# Non-interactive shells may not have mise's Node installation on PATH.
if ! command -v node >/dev/null 2>&1; then
  export PATH="$(dirname -- "$(mise which node)"):$PATH"
fi
exec node "$(dirname -- "${BASH_SOURCE[0]}")/build-release.mjs" "$@"
