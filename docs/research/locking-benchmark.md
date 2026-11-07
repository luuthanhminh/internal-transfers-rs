# Locking strategy benchmark

Question: which way of writing the transfer transaction stays correct and
does not fail requests when many transfers hit the same accounts in both
directions? Measured before any service code was written; the result drives
[ADR 0003](../adr/0003-ordered-row-locking.md) and
[ADR 0004](../adr/0004-idempotency-key.md).

## Setup

- `bench/run.sh` starts a throwaway PostgreSQL 17 container and runs each
  strategy in `bench/scripts/` with `pgbench`: 16 clients × 50 random transfers
  between 4 accounts holding 100 each, in both directions, no retries unless
  stated.
- The scripts use their own minimal tables without a `CHECK` constraint, so a
  strategy that corrupts data shows it. `deadlock_timeout` is lowered to 100 ms
  only to make the run faster.
- After each run `check.sql` reports the total (must stay 400), the minimum
  balance (must be ≥ 0) and the number of accounts that do not reconcile with
  their history.

Reproduce with `bench/run.sh` (requires Docker; takes a few minutes).

## Results

| Script | Approach | Failed requests | Total afterwards (correct: 400) |
|---|---|---|---|
| `v1_naive` | Read balance, compute in app, write absolute value | 52–61 % (deadlocks) | **981–1051** — money created |
| `v2_unordered_lock` | `FOR UPDATE` in request order (source, then destination) | 75–78 % (deadlocks) | 400 |
| `v4_conditional_update` | `UPDATE … WHERE balance >= amount`, no ordering | 44–52 % (deadlocks) | 400 |
| `v5_serializable` | `SERIALIZABLE`, up to 20 retries | 8–11 % still failing | 400 |
| `v3_ordered_lock` | **`FOR UPDATE` in id order (chosen)** | **0 %** | **400** |

Ranges are from two runs; transfers are random, so exact numbers vary.
`SERIALIZABLE` without retries failed most requests (serialization failures
plus deadlocks).

Idempotency variants, both on top of ordered locking:

| Script | Approach | Outcome |
|---|---|---|
| `v6_idem_claim_first` | Claim the key with `INSERT … ON CONFLICT DO NOTHING` right after locking | 0 % failures; one ledger row per key |
| `v7_idem_plain_insert` | Plain `INSERT` of the key at the end | Duplicate keys abort the transaction (`23505`); data stays correct only thanks to rollback, and a replay cannot return the original transfer |

## Conclusions

- Locking is necessary (v1 creates money) but not sufficient: the lock order
  must be global (v2 and v4 deadlock heavily).
- `SERIALIZABLE` is correct but pushes a retry loop onto every request and
  still fails under hot contention.
- Ordered `FOR UPDATE` at `READ COMMITTED` is correct with zero failures and is
  easy to explain.
- The idempotency key must be claimed with `ON CONFLICT DO NOTHING`, before
  balances change.
