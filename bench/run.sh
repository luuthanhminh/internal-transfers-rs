#!/usr/bin/env bash
# Reproduces the locking-strategy comparison quoted in the README.
#
# Starts a throwaway PostgreSQL container, runs each strategy in bench/scripts
# with pgbench (16 clients x 50 random transfers between 4 accounts holding 100
# each, both directions, no retries), then prints the failure rate and checks the
# invariants: total stays 400, no negative balance, every account reconciles.
#
# The scripts use their own minimal tables (acc, tx) and have no CHECK
# constraint, so a strategy that corrupts data shows it. deadlock_timeout is
# lowered to 100ms only to make the run faster.
#
# Usage: bench/run.sh            (requires Docker; takes a few minutes)
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
NAME="transfers-bench-$$"
CLIENTS=16
PER_CLIENT=50

docker run -d --rm --name "$NAME" -e POSTGRES_PASSWORD=bench postgres:17-alpine \
  -c max_connections=100 -c deadlock_timeout=100ms >/dev/null
trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true' EXIT
until docker exec "$NAME" pg_isready -U postgres >/dev/null 2>&1; do sleep 1; done
sleep 2
docker exec "$NAME" mkdir -p /bench
for f in "$DIR"/scripts/*.sql; do docker cp -q "$f" "$NAME:/bench/"; done

run() { # $1 = script, $2 = extra pgbench flags
  docker exec "$NAME" psql -U postgres -X -q -f /bench/schema.sql >/dev/null
  echo "=== $1 $2"
  docker exec "$NAME" pgbench -U postgres -n -c "$CLIENTS" -j 4 -t "$PER_CLIENT" $2 \
    -f "/bench/$1.sql" postgres 2>&1 \
    | grep -E "failed transactions|serialization failures|deadlock failures|retried|aborted" || true
  docker exec "$NAME" psql -U postgres -X -A -F ' | ' -f /bench/check.sql
  echo
}

for s in v1_naive v2_unordered_lock v3_ordered_lock v4_conditional_update v5_serializable; do
  run "$s" "--max-tries=1 --failures-detailed"
done
run v5_serializable "--max-tries=20 --failures-detailed"
run v6_idem_claim_first "--max-tries=1 --failures-detailed"
run v7_idem_plain_insert "--max-tries=1 --failures-detailed"
