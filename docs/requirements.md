# Requirements

What the service must do, the assumptions made where the brief left room, and
how we decide it is done. Written before any business code; decisions that
needed more thought are recorded as ADRs in [`adr/`](adr/).

## Scope

An HTTP service, backed by one PostgreSQL database, that keeps a set of
internal accounts and moves money between them.

| Capability | Endpoint | Notes |
|---|---|---|
| Open an account with an opening balance | `POST /accounts` | Client chooses the account id |
| Read an account's current balance | `GET /accounts/{account_id}` | |
| Transfer an amount from one account to another | `POST /transactions` | The core of the service |

Non-functional requirements, in priority order:

1. **Correctness under concurrency.** Many transfers may touch the same
   accounts at the same time, including transfers in opposite directions
   between the same pair. No money is created or lost, no balance ever goes
   negative, and no request fails because of the service's own locking.
2. **Atomicity.** Debit, credit and the record of the transfer are committed
   together or not at all.
3. **Auditability.** Completed transfers are kept as an immutable history from
   which every balance can be reconciled.
4. **Predictable errors.** Every failure is a typed JSON error with a stable
   code and an appropriate HTTP status; no panics on the request path.
5. **Operability.** One command to start everything, graceful shutdown,
   configuration from the environment, formatting and lints enforced in CI.

Out of scope: authentication, listing transaction history over HTTP,
pagination, multiple currencies, more than one service instance (the design
must not *prevent* it, but it is not required).

## Assumptions and defaults

Points the brief does not settle. None of them blocks the work, so each gets a
documented default instead of a question; the answer can change the default
later without changing the design.

| # | Question | Default chosen | Why |
|---|---|---|---|
| 1 | How many decimal places can an amount have? | Up to 18 decimal places and 20 integer digits; anything beyond is **rejected, never rounded** | Covers fiat and 18-decimal tokens; rounding money silently is never acceptable ([ADR 0002](adr/0002-money-as-decimal-strings.md)) |
| 2 | Are amounts JSON numbers or strings? | Strings only, in plain decimal notation (`"100.5"`); numbers are rejected | A JSON number may already have lost precision as a float |
| 3 | Which spellings are valid? | Digits, optional fractional part; no sign, exponent, leading zeros, whitespace or `_` | Lenient decimal parsers accept `"1e5"`, `"+5"`, `".5"`, `"1_000"` — ambiguous for money |
| 4 | Can an account be opened with a zero balance? | Yes | Only negative or invalid balances are clearly wrong |
| 5 | What is a valid account id? | A positive 64-bit integer chosen by the client | Simple, maps to `BIGINT` |
| 6 | What does a successful transfer return? | `201` with the recorded transfer (`transaction_id`, parties, amount, `created_at`) | Lets the client correlate and audit |
| 7 | What does account creation return? | `201` with an empty body | Nothing to add beyond the id the client sent |
| 8 | How are balances rendered? | Canonical decimal string without trailing zeros (`"49.6"`, not `"49.60"`) | One representation per value makes responses and tests stable |
| 9 | Is a transfer to the same account allowed? | No, `422 same_account` | It cannot change anything and usually indicates a client bug |
| 10 | What if a client retries a transfer after a timeout? | Optional `Idempotency-Key` header; the same key and body returns the original transfer instead of moving money twice | Retries after an unknown outcome are the normal failure mode of payment APIs ([ADR 0004](adr/0004-idempotency-key.md)) |
| 11 | What happens under heavy contention? | Short bounded retries for transient lock errors, then `503` with `Retry-After` | The client can retry safely; a 500 would hide that ([ADR 0005](adr/0005-retries-and-error-model.md)) |
| 12 | Is an HTTP endpoint for history needed? | No; history is stored and reconcilable with SQL and tests | Not required; keeps the surface small |
| 13 | Is there a maximum balance? | Yes: every balance stays below 10^20; a transfer that would reach it is rejected with `422 balance_limit_exceeded` | Without it, credits into one account produce balances longer than the 20 integer digits clients are told to expect |
| 14 | What happens to balances that already exceed the limit when it is introduced? | Kept as they are: the constraint is added `NOT VALID`, so only new writes are checked; validating old rows waits for an owner decision (adjust, split or exempt) | The database cannot decide what to do with money; a failing migration would block the release |

## Acceptance criteria

Each criterion is covered by at least one automated test.

Accounts

- Creating an account returns `201` with an empty body; reading it returns the
  opening balance.
- A duplicate id is rejected with `409` and leaves the original account intact,
  also when the same id is created concurrently.
- Negative, malformed or over-precise balances, non-positive ids and amounts
  sent as JSON numbers are rejected with `422`; malformed JSON with `400`; a
  missing `Content-Type` with `415`. All errors are JSON.
- An unknown account returns `404`.

Transfers

- A transfer debits the source, credits the destination and records exactly
  one history entry, atomically.
- Insufficient funds, unknown accounts, same-account transfers and invalid
  amounts are rejected and change nothing.
- Transferring the whole balance leaves exactly zero.
- A transfer that would push the destination balance to 10^20 or more is
  rejected and changes nothing; the database refuses such a balance too.
- A transfer whose client disconnects mid-request still completes.
- The database itself refuses a negative balance and any update, delete or
  truncate of the history.
- Retrying with the same `Idempotency-Key` returns the original transfer;
  reusing a key for a different transfer is rejected; concurrent requests with
  one key move money once.

Concurrency (the core)

- 100 transfers A→B and 100 B→A at once all succeed, with no server errors and
  unchanged balances.
- 10 concurrent withdrawals of 30 from a balance of 100: exactly 3 succeed.
- Hundreds of random concurrent transfers across several accounts keep the
  total constant, never produce a negative balance or a 5xx, and every account
  reconciles: `initial_balance + incoming − outgoing = balance`.

Operations

- `docker compose up --build` starts the database and the service, applying
  migrations on startup.
- SIGTERM lets in-flight requests finish before the process exits.
- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` pass in CI.
