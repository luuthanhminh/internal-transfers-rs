---
name: test-qa
description: Write and prove tests for this service - given/when/then through tests/api/helpers.rs, RED-proof against the exact regression, concurrency on the 16-connection pool, 503 resilience paths, schema upgrades with data, and a Docker smoke run. Use after implementing, before review, and whenever a test fails or flakes.
---

# Test / QA

## Where a test goes

| File | Owns |
|---|---|
| `#[cfg(test)]` in `money.rs`, `account_id.rs`, `config.rs` | parsing, digit limits, config validation, password redaction |
| `tests/api/accounts.rs`, `transactions.rs` | endpoint outcomes and error codes (incl. `balance_limit_exceeded`) |
| `idempotency.rs` | replay + `Idempotent-Replayed`, `idempotency_key_reused`, in-flight key, invalid key |
| `history.rs` | append-only triggers, reconciliation |
| `concurrency.rs` | `app_with_many_connections` (16; `#[sqlx::test]` pools have 5) |
| `resilience.rs` | lock held / 57014 / pool busy → 503 `temporarily_unavailable`; retry; client disconnect |
| `operations.rs` | `/health`, `/ready` (`not_ready`), `x-request-id` |
| `api_docs.rs` | OpenAPI lists every route, `/docs` served |
| `migrations.rs` | upgrade an older schema that holds data (`#[sqlx::test(migrations = false)]`) |

## Before writing a test

- Name the break: write down which production change must make it fail (e.g.
  "drop `ORDER BY account_id`"). If you cannot name one, the test is not
  needed or only detects change.
- One behaviour per test; the name reads as a sentence ("and" in it → split).
  The always-on state assertions below do not count as a second behaviour.

## Writing it

- Bodies call `tests/api/helpers.rs` only (`given_account`, `transfer`,
  `transfer_with_key`, `start_transfer`, `assert_error(status, code)`,
  `balance_of`, `ledger_entry_count`, `total_balance`,
  `assert_no_negative_balances`, `assert_all_accounts_reconcile`, …). New HTTP,
  JSON or SQL plumbing goes into `helpers.rs`.
- Expected values are literals (`"70.5"`), never computed with `Money::parse`,
  `format_amount` or any production code.
- Every money test also asserts state: balances, ledger count, totals unchanged.
- No mocks for PostgreSQL: produce failures for real
  (`given_another_client_holds_a_lock_on`, `given_all_database_connections_are_busy`,
  `with_unreachable_database`, `error_from_a_statement_cancelled_by_timeout`).
  A new failure mode gets a new `given_*` helper.
- No sleeps for ordering: wait on the database with
  `wait_until_a_transfer_waits_for_a_row_lock`,
  `wait_until_the_blocked_transfer_is_retried`, `wait_until_ledger_has`.
- Don't assert constants (`MAX_ATTEMPTS == 3`); assert what a client sees.
- Lock-timeout tests take about 6 s by design (3 attempts × 2 s).

## Prove it (RED)

Re-introduce the exact bug, see that test (ideally only that test) fail,
restore from a backup copy - never `git checkout`, it discards other
uncommitted work. Guard against vacuity: assert the ledger is non-empty before
comparing totals. Catalogue: locking in request order; no `FOR UPDATE`; plain
`INSERT` for the key; no balance or `balance_limit()` check; `MAX_ATTEMPTS = 1`;
no `tokio::spawn`; replay without the fast path; retrying 57014;
`Money::parse` accepting `"1e3"`, `"-1"`, `"01"`, 21 integer digits or 19
decimals; a constraint added without `NOT VALID`.

## When tests fail

- A wall of failures: first check the target. Run one unrelated test
  (`cargo test --test api the_service_is_ready`) and confirm `DATABASE_URL` points at the
  local database on 56432 (the Makefile loads `.env`, else `.env.example`;
  plain `cargo test` needs it exported).
- A flaky test is a bug: loop it
  (`for i in $(seq 1 20); do cargo test --test api <name> -q || break; done`)
  and debug it (`debug` skill). Never "fix" it with a sleep or a longer timeout.
- A defect that is not yours to fix: do not assert the wrong behaviour silently
  or skip the test. Add a separate `…_is_wrongly_…` test that pins it, with a
  doc comment saying what should happen and "when this is fixed this test goes
  red: update it", and report it.

## Docker smoke

`docker compose up --build -d --wait` → `/ready` 200, `/docs` 200 → create two
accounts → transfer 201 → insufficient funds 422 → same `Idempotency-Key`
again → 201 + `Idempotent-Replayed: true` → `docker compose stop app` exits 0
→ `docker compose down -v`.

## Report

Table: test file × added/changed × RED-proven (✅/❌, with the regression
used), then the `make test` summary line quoted.
