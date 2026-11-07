#!/usr/bin/env bash
# Stop hook: an agent cannot finish its turn while formatting or clippy fail.
# Exit code 2 sends the output back to the agent, which must fix it first.
# Tests are not run here (they need PostgreSQL and take a while); the
# implement skill requires `make check` output as evidence instead.
set -uo pipefail

input="$(cat)"
# Already continuing because of this hook: do not loop forever.
if grep -q '"stop_hook_active": *true' <<<"$input"; then
  exit 0
fi

cd "${CLAUDE_PROJECT_DIR:-.}" || exit 0
# Nothing to check when no Rust file changed.
if git diff --quiet HEAD -- '*.rs' Cargo.toml Cargo.lock 2>/dev/null \
  && [ -z "$(git ls-files --others --exclude-standard -- '*.rs')" ]; then
  exit 0
fi

if ! output="$(cargo fmt --check 2>&1 && cargo clippy --all-targets --locked --quiet -- -D warnings 2>&1)"; then
  echo "Lint gate failed (cargo fmt --check / clippy -D warnings). Fix before finishing:" >&2
  echo "$output" | tail -n 60 >&2
  exit 2
fi
exit 0
