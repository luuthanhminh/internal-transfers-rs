# Pitfalls and test plan

Known ways a transfer service like this one goes wrong, how the design avoids
each, and the test that proves it. Test names are the planned ones; the
integration tests live in `tests/api/`, unit tests next to the domain types.

## Money and parsing

| # | Pitfall | Avoided by | Test |
|---|---|---|---|
| M1 | Floats for money (`0.1 + 0.2`) | `BigDecimal` + `NUMERIC` ([ADR 0002](../adr/0002-money-as-decimal-strings.md)) | `keeps_exact_values_without_float_drift` |
| M2 | Lenient parsing accepts `"1e5"`, `"+5"`, `".5"`, `"5."`, `"1_000"`, `"00012"` | Strict grammar before `BigDecimal::from_str` | `rejects_forms_that_a_lenient_decimal_parser_would_accept` |
| M3 | Whitespace, empty, `NaN` | Same grammar | `rejects_whitespace_empty_and_non_numeric_input` |
| M4 | Silent rounding beyond the supported scale | Reject > 18 decimals; unconstrained `NUMERIC` | `rejects_more_decimal_places_than_supported_instead_of_rounding` |
| M5 | Overflow of huge values | Reject > 20 integer digits; arbitrary precision decode | `rejects_amounts_above_the_maximum`, largest amount round trip |
| M6 | JSON number accepted and parsed as float | Request fields are `String` | `an_amount_sent_as_a_json_number_is_rejected` |
| M7 | Zero or negative transfer | `parse_positive` | `transfer_amount_must_be_greater_than_zero` |
| M8 | Inconsistent rendering (`49.60` vs `49.6`) | One canonical form | `formats_balances_without_trailing_zeros_or_exponents` |

## Concurrency and integrity

| # | Pitfall | Avoided by | Test |
|---|---|---|---|
| C1 | Lost update: read, compute, write absolute | Row lock + relative update ([ADR 0003](../adr/0003-ordered-row-locking.md)) | `concurrent_withdrawals_never_spend_more_than_the_balance` |
| C2 | Deadlock on A→B and B→A | Lock rows in ascending id order | `opposite_direction_transfers_between_two_accounts_all_succeed_without_deadlock` |
| C3 | Balance checked before the lock (TOCTOU) | Check after `FOR UPDATE`, same transaction | as C1 |
| C4 | A statement run on the pool instead of the transaction | Data-access functions take `&mut PgConnection` | reconciliation after random concurrent transfers |
| C5 | Forgetting `COMMIT` | Tests read balances back | `a_transfer_moves_money_and_records_the_transaction` |
| C6 | Check-then-insert race on duplicate accounts | Insert and map unique violation to `409` | `creating_the_same_account_concurrently_succeeds_exactly_once` |
| C7 | Only the application guards the balance | `CHECK (balance >= 0)` | `the_database_refuses_a_negative_balance_even_if_the_application_check_is_bypassed` |
| C8 | Unbounded lock waits | `lock_timeout`, pool acquire timeout | busy-path tests answering `503` |
| C9 | Concurrency test that does not really overlap | Each request on its own task with its own connection; assert "no 5xx" | concurrency suite |
| C10 | Same idempotency key sent concurrently | `UNIQUE` + `ON CONFLICT DO NOTHING` ([ADR 0004](../adr/0004-idempotency-key.md)) | `concurrent_requests_with_the_same_key_transfer_exactly_once` |
| C11 | Key reused for a different transfer silently replays the old one | Compare source, destination and amount before replaying | `reusing_a_key_for_a_different_transfer_is_rejected`, `reusing_a_key_with_a_different_destination_is_rejected` |
| C12 | A retry waiting on the lock sees the drained balance and gets `insufficient_funds` instead of its replay | Claim the key before the balance check; replay fast path before taking locks | `replaying_a_completed_transfer_succeeds_even_after_the_source_was_drained`, `a_replay_is_answered_at_once_even_while_the_accounts_are_locked` |
| C13 | Retrying an error whose outcome is unknown (connection lost at `COMMIT`) moves money twice | Retry only 40001/40P01/55P03, which guarantee a rollback ([ADR 0005](../adr/0005-retries-and-error-model.md)); clients retry with the same key | `a_transfer_whose_first_attempt_times_out_on_a_lock_succeeds_on_retry` (guards the retry), `a_transfer_blocked_by_a_long_held_lock_is_retried_then_answered_with_service_unavailable` (guards the 503 mapping only); lost `COMMIT` itself not automated |
| C14 | Client disconnect drops the handler mid-transaction or mid-`COMMIT` | Transfer runs on its own `tokio::spawn` task | `a_transfer_completes_even_if_the_client_disconnects_while_it_runs` |
| C15 | Balances grow without bound (many credits into one account) and outgrow what clients expect | Destination checked under the lock: balance stays below 10^20; `CHECK (balance < 1e20)` backstop | `a_transfer_that_would_push_a_balance_past_the_limit_is_rejected_and_changes_nothing`, `a_balance_may_reach_just_below_the_limit`, `the_database_refuses_a_balance_past_the_limit_even_if_the_application_check_is_bypassed` |

## History

| # | Pitfall | Avoided by | Test |
|---|---|---|---|
| H1 | No opening balance stored, so nothing to reconcile against | Immutable `initial_balance` column | `an_initial_balance_cannot_be_changed_after_creation` |
| H2 | "Immutable" history only by convention | Triggers reject `UPDATE`, `DELETE`, `TRUNCATE` | `transaction_history_cannot_be_updated_deleted_or_truncated` |
| H3 | No proof the balances add up | `initial + incoming − outgoing = balance` for every account | `every_balance_reconciles_with_its_initial_balance_and_history` |

## HTTP

| # | Pitfall | Avoided by | Test |
|---|---|---|---|
| W1 | axum's default rejections are plain text | Custom `Json`/`Path` extractors returning `AppError` | malformed JSON, missing field, missing `Content-Type`, non-numeric id |
| W2 | Database details leaked in 5xx bodies | Generic message, details only in logs | error model unit of `AppError` |
| W3 | Account id beyond 64 bits or ≤ 0 | serde + `AccountId::new` | `an_account_id_beyond_64_bits_is_rejected`, `a_non_positive_account_id_is_rejected` |

## Operations

| # | Pitfall | Avoided by | Check |
|---|---|---|---|
| O1 | Only Ctrl-C handled; Docker sends SIGTERM | Listen for both | manual: `docker compose stop` during a request |
| O2 | Shell-form `CMD`, so the binary is not PID 1 | Exec-form `CMD` | Dockerfile review |
| O3 | Pool closed before in-flight requests finish | Close the pool after `serve` returns | manual |
| O4 | Fixed host port 5432 collides with a local PostgreSQL | Configurable, non-default host port | `make db-up` on a machine with PostgreSQL |
| O5 | `unwrap` on the request path | clippy `unwrap_used`/`expect_used`/`panic` denied | `make lint` |

## Proving the tests

A concurrency test that passes might pass by luck. Each safeguard is checked by
re-introducing the bug and watching the matching test fail, then restoring:
lock in request order, drop `FOR UPDATE`, omit `COMMIT`, claim the
idempotency key with a plain insert, run the transfer without `tokio::spawn`
(C14), allow a single attempt (C13), skip the balance-limit check (C15).
