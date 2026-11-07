---
name: implement
description: Change this axum/sqlx transfer service test-first without breaking Money, the 10^20 balance limit, ordered FOR UPDATE locking, the append-only ledger, Idempotency-Key or the AppError contract. Use for any edit under src/, migrations/ or tests/, including "small" fixes.
---

# Implement

Read AGENTS.md *Invariants*, *Migrations* and *Rust style* first; this skill
adds the exact names to touch and the gates.

## Iron law: test first

1. Write the test (`tests/api/…`, or `#[cfg(test)]` in `money.rs`,
   `account_id.rs`, `config.rs`). For a bug, the test reproduces it.
2. Run it and watch it fail for the right reason - a failed assertion, not a
   compile error or a typo. A test that passes on its first run tests existing
   behaviour: fix the test before going on.
3. Smallest change that makes it pass: no new settings, options or
   abstractions, no "while I'm here" edits; mention unrelated problems in the
   report instead.
4. Code written before its test is thrown away and redone test-first.

Narrow loop: `cargo test --test api <substring>`; then `make check`.

## Layers

- `src/routes/`: extract (`extractors::{Json, Path}`), validate into `Money` /
  `AccountId`, map failures to `AppError`. No SQL.
- `src/domain/`: `Money::parse` / `parse_positive` (strict decimal strings,
  `MAX_INTEGER_DIGITS = 20`, `MAX_FRACTION_DIGITS = 18`), `balance_limit()`,
  `format_amount`, `AccountId` (> 0).
- `src/db/`: all SQL. `transfers::transfer` → `find_by_idempotency_key` (fast
  path, before any lock) → `try_transfer`, whose statements all run on the
  transaction (`&mut *tx`); the pool is used only outside it.

## Transfer checklist (`src/db/transfers.rs`)

- Keep `SET LOCAL lock_timeout = '2s'` and `statement_timeout = '10s'`.
- `lock_accounts`: `WHERE account_id = ANY($1) ORDER BY account_id FOR UPDATE`.
- `insert_ledger_entry` (`ON CONFLICT (idempotency_key) DO NOTHING`) before
  `apply_balance_delta`; a conflict → `rollback` + `replay`.
- Check the source balance and `balance_limit()` for the destination after
  locking; updates are relative (`balance ± amount`).
- Retry only `db::is_retryable` (40001, 40P01, 55P03), `MAX_ATTEMPTS = 3`;
  57014 and `PoolTimedOut` → 503 without retry; never retry after a lost COMMIT.
- The handler keeps `tokio::spawn` so a client disconnect cannot cancel COMMIT.

## New error, endpoint, setting, migration

- Error: `AppError` variant + stable snake_case code + status in
  `src/error.rs`, matched without a `_` arm; README error table;
  `#[utoipa::path]` responses. Busy → `temporarily_unavailable` + `Retry-After: 1`.
- Endpoint: the four steps in AGENTS.md (`#[utoipa::path]`, `routes::router` +
  `ApiDoc`, tests, README).
- Setting: `Config::from_lookup` + `.env.example` + README configuration table
  + unit test in `config.rs`; secrets only through `redact_password`.
- Migration: `make migration name=<snake_case>`; never edit an applied file;
  constraints `NOT VALID`, indexes `CONCURRENTLY` with `-- no-transaction`; an
  upgrade test in `tests/api/migrations.rs` (`given_a_database_migrated_up_to`).

## Stop and ask

- The change contradicts an ADR or an AGENTS.md invariant.
- A migration would edit an applied file or drop/rename a column.
- A new public error code or status is needed and is not in the plan.
- Three fix attempts failed: stop and use the `debug` skill.

## Done means (evidence in the same message)

- `make check` run AFTER the last edit, exit 0, its summary lines quoted
  (`test result: ok. N passed; 0 failed`). "Should pass", an earlier run or
  someone else's claim is not evidence.
- Any failing test is named, even one you believe you did not cause: `main`
  is always green.
- The commit contains the code, the tests, the docs and the `CHANGELOG.md`
  Unreleased line, and nothing else (stage files by name, not `git add -A`).

## Red flags (reject on sight)

- Reading a balance and writing an absolute value back.
- Any query on the pool inside `try_transfer`.
- `f64`, `as f64`, `to_f64`, `.round`, `with_scale` on money.
- Retrying an error `is_retryable` rejects; adding a code to `is_retryable`
  without an ADR 0005 update.
- A `match` on `AppError` with `_ =>`; `#[allow]` instead of
  `#[expect(lint, reason = "…")]`; `unwrap`/`expect`/`panic!` outside tests.
- Cloning on the transfer path where a borrow works (`redundant_clone` is denied).
