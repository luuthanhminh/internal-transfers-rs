---
name: debug
description: Find the root cause of a failing or flaky test, a 500, a 503 storm, a hang or wrong balances in this transfer service before changing any code - reproduce, localize, reduce, prove with a failing test, then fix once. Use whenever something is red or behaves unexpectedly, and after a fix attempt failed.
---

# Debug

No fix before the root cause is known. While a test is red, no new feature work.

## 1. Reproduce narrowly

- `cargo test --test api <name> -- --nocapture` (needs `DATABASE_URL` → the
  local database on 56432; check that first, see `test-qa`).
- Flaky: loop it (`for i in $(seq 1 50); do cargo test --test api <name> -q || break; done`);
  add `-- --test-threads=1` to rule out load from other tests (each
  `#[sqlx::test]` has its own database, so tests only share PostgreSQL itself).
- The running service: `make run RUST_LOG=debug,sqlx=info` (a make argument,
  because the Makefile loads `RUST_LOG` from `.env`/`.env.example`; `EnvFilter`
  in `src/main.rs`; `LOG_FORMAT=json` for structured logs; every request
  carries `x-request-id`). Tests do not initialise tracing: use a temporary
  `eprintln!` with `--nocapture` and remove it before committing.

## 2. Localize

| Symptom | Look at |
|---|---|
| 500 `internal_error` | which `AppError` arm produced it; the SQLSTATE via `db::sqlstate_of` (`src/db/mod.rs`); the server log line |
| 503 `temporarily_unavailable` | lock waits (`lock_timeout` 2 s, 3 attempts), `statement_timeout` 10 s (57014), pool exhaustion (`DB_MAX_CONNECTIONS`, `DB_ACQUIRE_TIMEOUT_SECS`) |
| Hang, lock wait, deadlock | while it is stuck: `SELECT pid, state, wait_event_type, wait_event, query FROM pg_stat_activity WHERE datname = current_database();` and `SELECT locktype, relation::regclass, mode, granted, pid FROM pg_locks WHERE NOT granted;` |
| Wrong balances, money created or lost | the reconciliation query in README; compare with `bench/run.sh`, which reproduces locking behaviour outside the app |
| Worked before | `git bisect start <bad> <good>` + `git bisect run cargo test --locked --test api <name>`; every commit is green (CI `every-commit`), so bisect is reliable |

## 3. Reduce, then prove

- Change one variable at a time; write the hypothesis down before testing it.
- Turn the reproduction into a test that fails for the root cause (see
  `test-qa`), then make one fix.

## Never

- Add a SQLSTATE to `is_retryable` to make a 500 disappear (that changes ADR
  0005 and can move money twice).
- Fix a flaky test with a sleep or a longer timeout.
- Stack a second fix on a first one that did not work: revert it first.

## Stop and ask

Three fix attempts failed: stop, write down what each attempt showed, question
the design (is an invariant or ADR wrong?) and bring it to the user.

## Done means

The reproducing test was red before and is green after, `make check` passes
(quote it), and the cause is stated in one sentence in the commit message.
