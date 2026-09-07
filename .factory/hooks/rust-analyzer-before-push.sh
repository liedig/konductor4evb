#!/usr/bin/env bash
# PreToolUse gate: block `git push` while rust-analyzer reports errors.
# Registered in .factory/hooks.json with a commandRegex matching git push.
set -uo pipefail

ROOT="${FACTORY_PROJECT_DIR:-$PWD}"

# `command -v` is not enough: rustup installs a rust-analyzer shim even when
# the component itself is missing, and the shim only fails at runtime.
if ! rust-analyzer --version >/dev/null 2>&1; then
  echo "rust-analyzer unavailable — install with: rustup component add rust-analyzer" >&2
  exit 2
fi

OUTPUT=$(cd "$ROOT" && rust-analyzer diagnostics . 2>&1)
STATUS=$?

if [ "$STATUS" -ne 0 ]; then
  [ -n "$OUTPUT" ] && echo "$OUTPUT"
  echo "rust-analyzer diagnostics failed (exit $STATUS); push blocked" >&2
  exit 2
fi
echo '{"suppressOutput": true}'
exit 0
