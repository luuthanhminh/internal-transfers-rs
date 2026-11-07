---
name: review
description: Review a diff of this transfer service with fresh eyes against docs/requirements.md, the AGENTS.md invariants, ADRs 0001-0006, the AppError/OpenAPI contract and 12-factor config - and handle review feedback you receive. Use before merging or pushing, and when acting on someone's review.
---

# Review

## Set-up

- The reviewer did not write the change: a teammate, or a new agent session
  with no history of it. Give them the commit range, `docs/requirements.md`,
  AGENTS.md, the ADRs the diff touches and this checklist - the code, not your
  conclusions.
- The reviewer is read-only: `git diff/show/log <base>..<head>`; to run another
  revision use `git worktree add <tmp> <sha>`, never move the author's HEAD.

## Order

1. Tests first: does each new test fail without the change (RED-proof stated)?
2. Verify the verification: re-run `make check`, or read the CI `every-commit`
   run for the range (`gh run list --workflow ci.yml`), instead of trusting
   "tests pass".
3. Then the code, with the checklist.

| Area | Questions |
|---|---|
| Money | `f64`, rounding, or parsing outside `Money::parse`? 20/18 digits and `balance_limit()` kept? |
| Locking | `ORDER BY account_id FOR UPDATE`; balances checked after the lock; every statement of `try_transfer` on `&mut *tx` |
| Ledger | triggers `transactions_are_append_only`, `transactions_cannot_be_truncated`, `accounts_initial_balance_is_immutable` intact; reconciliation holds |
| Idempotency | key claimed with `ON CONFLICT` before balances move; replay compares the body (`idempotency_key_reused`); fast path before locks |
| Retries | only 40001/40P01/55P03; 57014 / `PoolTimedOut` → 503 not retried; nothing retried after COMMIT |
| Errors | stable code + status, README table, `#[utoipa::path]`, generic 5xx message |
| Migrations | ADR 0006: new file, `lock_timeout`, `NOT VALID` (never validated in the same file), `CONCURRENTLY`, upgrade test; does the previous binary still run on the new schema? |
| Requirements | every acceptance criterion in `docs/requirements.md` has a test; input the spec does not mention (18-decimal zero, `i64::MAX` id, empty key) is judged by what a reasonable client expects |
| Structure | SQL outside `src/db/`; validation outside domain types; a one-feature conditional bolted onto `try_transfer`; a near-duplicate helper in `tests/api/helpers.rs` |
| Ops / 12-factor | config only from env (`src/config.rs`, `.env.example` complete); password redacted; TLS via `sslmode`; `/health` vs `/ready`; `RUN_MIGRATIONS` / `migrate`; `DB_MAX_CONNECTIONS` × instances < PostgreSQL `max_connections`; SIGTERM drain |
| Conventions | naming, needless clones (`redundant_clone` is denied), allocations on the transfer path, AGENTS.md *Rust style* |

## Output

- Each finding: severity, `file:line`, one sentence, a concrete failure
  scenario (input or state → wrong result). Verify it against the code first;
  drop what does not survive.
- Severity: **blocker** = money created or lost, an invariant broken, data
  loss, internals in a 5xx, a broken commit in the range; **should-fix** =
  missing test or doc, unclear error mapping; **nit** = style.
- *Declined to judge*: what you looked at and set aside, one line + reason.
- Verdict: merge, or fix first.

## Receiving a review

- Check each finding against the code before changing anything; if several are
  unclear, ask about all of them first.
- A wrong finding gets a reply with a test or a `file:line`, not a change. No
  performative agreement.
- Fix one finding at a time and re-run its test each time.
- After three review/fix rounds on the same issue, stop and bring it to the user.
