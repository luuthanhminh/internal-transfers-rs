# internal-transfers

[![CI](https://github.com/luuthanhminh/internal-transfers-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/luuthanhminh/internal-transfers-rs/actions/workflows/ci.yml)
[![Security](https://github.com/luuthanhminh/internal-transfers-rs/actions/workflows/security.yml/badge.svg)](https://github.com/luuthanhminh/internal-transfers-rs/actions/workflows/security.yml)

An HTTP service for moving money between internal accounts, written in Rust
(axum + sqlx) on PostgreSQL. It creates accounts, reports balances, and executes
transfers that stay correct under concurrent load.

Live demo: **https://transfers.minhlt.com/docs** (interactive API reference;
rate-limited, data is disposable).

## For reviewers: a 10-minute tour

| If you want to see | Look at |
|---|---|
| It runs | `docker compose up --build`, then the curl commands under *Quick start* |
| How correctness under concurrency is achieved | *Transfer algorithm and concurrency* below, then [`src/db/transfers.rs`](src/db/transfers.rs) |
| That it is proven, not assumed | [`tests/api/concurrency.rs`](tests/api/concurrency.rs), the property tests in [`src/domain/money.rs`](src/domain/money.rs), the [locking benchmark](docs/research/locking-benchmark.md) |
| Behaviour under real HTTP load | [loadtest/](loadtest/README.md): k6 in CI on every push, money conserved, no 5xx |
| How the work was done with an AI agent, and how its code was reviewed | [docs/ai-assisted-delivery.md](docs/ai-assisted-delivery.md), [AGENTS.md](AGENTS.md), [`.claude/`](.claude/) |
| Decisions and their alternatives | [docs/approach.md](docs/approach.md), [docs/adr/](docs/adr/) |
| Supply chain and deployment | [docs/deployment.md](docs/deployment.md): pinned and signed images, SBOM, provenance, tunnel-only host |

## Docs

| Document | What it covers |
|---|---|
| [docs/ai-assisted-delivery.md](docs/ai-assisted-delivery.md) | How an AI coding agent was set up, constrained and reviewed to build this |
| [docs/approach.md](docs/approach.md) | Engineering approach: timeline of activities, reasoning and outcomes |
| [docs/requirements.md](docs/requirements.md) | Scope, assumptions with chosen defaults, acceptance criteria |
| [docs/architecture.md](docs/architecture.md) | Components, request flow, transfer algorithm, schema |
| [docs/adr/](docs/adr/) | Decisions: stack, money representation, locking, idempotency, retries and errors, migrations |
| [docs/research/](docs/research/) | Locking benchmark, pitfalls and the test plan |
| [docs/plan.md](docs/plan.md) | Delivery stages |
| [docs/deployment.md](docs/deployment.md) | Release pipeline, supply-chain controls, single-host deployment, rollback |
| [loadtest/README.md](loadtest/README.md) | HTTP load test: scenarios, pass criteria, how to read the results |
| [CHANGELOG.md](CHANGELOG.md) | Release notes |
| [AGENTS.md](AGENTS.md) | How to change the code safely (for agents and humans) |

## Quick start

Requirements: Docker with Compose v2.

```bash
docker compose up --build
```

This starts PostgreSQL, builds the service, applies the database migrations on
startup and serves on `http://localhost:8080`. Ports are configurable:
`APP_PORT=9090 DB_PORT=5433 docker compose up --build`.

```bash
curl -i -X POST localhost:8080/accounts \
  -H 'Content-Type: application/json' -d '{"account_id": 123, "initial_balance": "100.23344"}'
curl -i -X POST localhost:8080/accounts \
  -H 'Content-Type: application/json' -d '{"account_id": 456, "initial_balance": "0"}'
curl -i -X POST localhost:8080/transactions \
  -H 'Content-Type: application/json' -H 'Idempotency-Key: 4f1c2a' \
  -d '{"source_account_id": 123, "destination_account_id": 456, "amount": "100.12345"}'
curl -s localhost:8080/accounts/123   # {"account_id":123,"balance":"0.10999"}
```

## Running locally with cargo

Requirements: Rust 1.94+ (stable; required by sqlx 0.9), Docker (for PostgreSQL), `make`.
Install Rust with [rustup](https://rustup.rs) (`rustup update stable` if it is
older). Crates are fetched by cargo on first build; no other tools (such as
`sqlx-cli`) are needed, because migrations are embedded in the binary.

```bash
cp .env.example .env    # optional; the Makefile falls back to .env.example
make run                # starts PostgreSQL on localhost:56432, migrates, serves on :8080
make test               # unit + integration tests (needs PostgreSQL, started automatically)
make lint               # cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings
```

Without `make`:

```bash
docker compose up -d --wait db                                          # database only
export DATABASE_URL=postgres://transfers:transfers@localhost:56432/transfers
cargo run --release                                                     # migrate + serve on :8080
cargo test                                                              # all tests
```

The tests need `DATABASE_URL` to point at a PostgreSQL user that may create
databases: each integration test creates and migrates its own throwaway
database, so they never touch the service's data.

Configuration (environment variables):

| Variable | Default | Meaning |
|---|---|---|
| `DATABASE_URL` | — (required) | PostgreSQL connection string |
| `BIND_ADDR` | `0.0.0.0:8080` | Listen address |
| `DB_MAX_CONNECTIONS` | `20` | Connection pool size |
| `DB_ACQUIRE_TIMEOUT_SECS` | `5` | Max wait for a pooled connection |
| `RUN_MIGRATIONS` | `true` | Apply migrations on startup; set `false` when they run as a release step (`internal-transfers migrate`) |
| `LOG_FORMAT` | `text` | `text` or `json` (one JSON object per line, for log aggregators) |
| `RUST_LOG` | `info` | Log filter |

All configuration comes from the environment; invalid values stop the process
at startup. Secrets are never logged (the database password is redacted).
For managed PostgreSQL that requires TLS, append `?sslmode=require` (or
`verify-full`) to `DATABASE_URL`.

## API

| Endpoint | Success | Body |
|---|---|---|
| `POST /accounts` | `201`, empty body | `{"account_id": 123, "initial_balance": "100.23344"}` |
| `GET /accounts/{account_id}` | `200` | → `{"account_id": 123, "balance": "100.23344"}` |
| `POST /transactions` | `201` + transfer record | `{"source_account_id": 123, "destination_account_id": 456, "amount": "100.12345"}` |
| `GET /health` | `200` | → `{"status": "ok"}` — liveness, never touches the database |
| `GET /ready` | `200` / `503` | → `{"status": "ready"}` — readiness, pings the database |
| `GET /docs`, `GET /api-docs/openapi.json` | `200` | API reference page, OpenAPI document |

`POST /transactions` returns
`{"transaction_id", "source_account_id", "destination_account_id", "amount", "created_at"}`
and accepts an optional `Idempotency-Key` header (see below).

Interactive API reference: `http://localhost:8080/docs` (Scalar), generated
from the code with `utoipa`; the raw OpenAPI 3.1 document is at
`/api-docs/openapi.json`. A test checks that it lists every endpoint, so it
cannot silently drift from the router.

Every error has the same shape: `{"error": {"code": "...", "message": "..."}}`.

| Status | `code` | When |
|---|---|---|
| 400 | `invalid_request` | Malformed JSON, non-numeric path id |
| 400 | `invalid_idempotency_key` | Header is empty, too long or not visible ASCII |
| 415 | `invalid_request` | Missing `Content-Type: application/json` |
| 422 | `invalid_request` | Missing field or wrong JSON type (e.g. amount sent as a number) |
| 422 | `invalid_amount` | Bad format, negative, zero transfer, too many digits |
| 422 | `invalid_account_id` | `account_id` ≤ 0 |
| 422 | `same_account` | Source equals destination |
| 422 | `insufficient_funds` | Source balance lower than the amount |
| 422 | `balance_limit_exceeded` | Destination balance would reach 10^20 |
| 422 | `idempotency_key_reused` | Key already used for a different transfer |
| 404 | `account_not_found` | Account (or either transfer party) does not exist |
| 409 | `account_already_exists` | Duplicate `account_id` |
| 503 | `temporarily_unavailable` | Busy: lock contention persisted after retries, statement timeout, or no pooled connection free in time (`Retry-After: 1`) |
| 500 | `internal_error` | Unexpected failure (details only in server logs) |

## Assumptions

> Defaults to be confirmed with the product owner before relying on them;
> decimal precision and the balance limit in particular are business rules,
> not technical choices.

- Amounts are JSON **strings** in plain decimal notation: digits, an optional
  fractional part, no sign, exponent or leading zeros (`"0.5"`, `"100"`,
  `"100.23344"`). JSON numbers are rejected because they may already have lost
  precision as floats.
- Up to 20 integer digits and 18 decimal places. Values outside that range are
  **rejected, never rounded**.
- A balance stays below 10^20 (the same 20 integer digits as an amount); a
  transfer that would push the destination past it is rejected.
- Balances are returned in canonical form without trailing zeros
  (`"49.60"` → `"49.6"`, `"100.00"` → `"100"`).
- An account may be opened with a zero balance. Account ids are client-chosen
  positive 64-bit integers.
- A failed request changes nothing; only completed transfers are recorded.
- The `Idempotency-Key` header is optional so plain requests keep working.

## Design

```
src/
  main.rs         thin binary: logging, config, startup::run
  startup.rs      pool, migrations, server, graceful shutdown
  config.rs       environment configuration
  routes/         HTTP layer: handlers, JSON-error extractors, request/response types
  domain/         validated types (Money, AccountId) — invalid values cannot be constructed
  db/             all SQL; transfers.rs holds the transfer transaction
  error.rs        AppError → status code + JSON body
migrations/       schema, constraints, immutability triggers
tests/api/        integration tests (one binary, one fresh database per test)
```

Libraries: **axum** (tower-based, typed extractors, built-in graceful shutdown),
**sqlx** (async, embedded migrations, plain SQL; runtime-checked queries so the
project builds without a database), **bigdecimal** (arbitrary precision, so
large balances can never overflow when read back from `NUMERIC`), **thiserror**
for typed errors.

### Schema

- `accounts(account_id, initial_balance, balance)` with
  `CHECK (balance >= 0)` and `CHECK (balance < 1e20)`. `initial_balance` never changes (enforced by a trigger).
- `transactions(transaction_id, source_account_id, destination_account_id, amount, idempotency_key, created_at)`
  — the ledger. Append-only: triggers reject `UPDATE`, `DELETE` and `TRUNCATE`.
- Amount columns are unconstrained `NUMERIC`; `NUMERIC(p, s)` would silently round.

Any balance can be reconciled from history:

```sql
SELECT a.account_id, a.balance,
       a.initial_balance
     + coalesce((SELECT sum(amount) FROM transactions WHERE destination_account_id = a.account_id), 0)
     - coalesce((SELECT sum(amount) FROM transactions WHERE source_account_id      = a.account_id), 0)
       AS reconciled
FROM accounts a;
```

### Transfer algorithm and concurrency

One database transaction (READ COMMITTED) per transfer, in `src/db/transfers.rs`:

```
validate input (amount > 0, source != destination)       -- before touching the DB
BEGIN; SET LOCAL lock_timeout = '2s', statement_timeout = '10s'
SELECT account_id, balance FROM accounts
  WHERE account_id = ANY($ids) ORDER BY account_id FOR UPDATE   -- lock both, lowest id first
missing account?              → ROLLBACK, 404
INSERT ledger row ... ON CONFLICT (idempotency_key) DO NOTHING
  key already used?           → ROLLBACK, return the original transfer (or 422 if it differs)
balance < amount?             → ROLLBACK, 422
UPDATE balance = balance - amount (source); UPDATE balance = balance + amount (destination)
COMMIT
```

Why this is correct with many concurrent requests:

- **No double spending.** The balance is read under a row lock, so two transfers
  from the same account are serialised and the second sees the first's result.
- **No deadlocks.** Every transfer locks its two accounts in the same global
  order (ascending id), so A→B and B→A running together cannot wait on each
  other in a cycle.
- **Atomic.** The ledger row and both balance updates commit together or not at all.
- **Defence in depth.** `CHECK (balance >= 0)` rejects a negative balance even if
  the application check were bypassed; updates are relative (`balance - amount`).

I measured the alternatives with `pgbench` (16 clients, 4 accounts, transfers in
both directions) before choosing. Reproduce with `bench/run.sh` (Docker; the
scripts are in `bench/scripts/`):

| Approach | Failed requests | Total money afterwards (correct: 400) |
|---|---|---|
| Read balance, compute in app, write absolute value | 52–61 % (deadlocks) | **981–1051** — money created |
| `FOR UPDATE` in request order (source, then destination) | 75–78 % (deadlocks) | 400 |
| `UPDATE ... WHERE balance >= amount`, no ordering | 44–52 % (deadlocks) | 400 |
| `SERIALIZABLE`, up to 20 retries | 8–11 % still failing | 400 |
| **`FOR UPDATE` in id order (chosen)** | **0 %** | **400** |

Ranges are from two runs; transfers are random, so exact numbers vary.

### Retries, rollback and idempotency

- Deadlock, serialization failure and lock timeout (`40P01`, `40001`, `55P03`)
  are errors after which PostgreSQL guarantees a rollback, so the whole
  transaction is re-run (up to 3 attempts with jittered backoff) before
  answering 503. With ordered locking they should not occur; retries are logged
  as warnings so a regression is visible.
- A statement timeout or an exhausted connection pool is also answered with
  503 + `Retry-After`, but not retried in-process (that would only lengthen the
  queue). `statement_timeout` (10s) is set above 4 × `lock_timeout`, because
  locking two rows typically involves up to four separate lock waits, so lock
  waits normally fail first and get retried.
- On business errors the transaction is rolled back explicitly; a failed
  rollback is only logged, so it cannot turn a 404/422 into a 500.
- Connection errors are **not** retried: if the connection drops during
  `COMMIT`, the outcome is unknown and a blind retry could transfer twice. That
  case is what `Idempotency-Key` is for — the client can safely resend the same
  request with the same key.
- The idempotency key is claimed with `INSERT ... ON CONFLICT DO NOTHING`
  before any balance changes. A plain insert would abort the transaction on a
  duplicate key; `ON CONFLICT` waits for an in-flight request holding the same
  key and then reports the conflict cleanly. Replays return the original
  transfer with `Idempotent-Replayed: true`. A retry whose key is already
  recorded is answered before any account lock is taken, so it is not delayed
  (or refused) by traffic on the same accounts.
- The transfer runs on its own Tokio task, so a client disconnecting mid-request
  cannot cancel it half-way through `COMMIT`.

### Operational behaviour

- Graceful shutdown on SIGTERM / Ctrl-C: the server stops accepting connections,
  lets in-flight requests finish, then closes the pool. The Docker image runs
  the binary as PID 1 (exec form) so it receives SIGTERM.
- Migrations are embedded and applied on startup; sqlx guards them with an
  advisory lock, so several instances can start at once. They can also run as a
  one-off release step: `internal-transfers migrate`, with `RUN_MIGRATIONS=false`
  on the service. Migrations are forward-only and must not block traffic; add
  one with `make migration name=<what_it_does>` (rules in
  [ADR 0006](docs/adr/0006-forward-only-migrations.md)).
- Probes: `/health` (liveness) and `/ready` (readiness, database reachable).
- Every response carries `X-Request-Id` (the caller's value, or a new UUID); it
  is recorded on the request's log span, so one request can be followed
  through the logs.
- Docker base images are pinned by tag and digest; BuildKit cache mounts avoid
  recompiling dependencies on every build.
- Security checks (`.github/workflows/security.yml`, on every push and weekly):
  `cargo deny check` for RustSec advisories, yanked crates, licenses and crate
  sources (`deny.toml`); gitleaks over the whole git history; Trivy over the
  runtime image (HIGH/CRITICAL with a fix); CodeQL static analysis of the Rust
  code; zizmor over the workflows themselves. Every action is pinned to a
  commit SHA and every job gets a read-only token unless it needs more.
  Dependabot opens weekly update PRs for Cargo (patch releases grouped, minors
  separately), Docker, docker compose digests and GitHub Actions; the Rust
  build image is bumped by hand together with `rust-toolchain.toml`.
- Releases (`.github/workflows/release.yml`, on a `v*` tag) publish the image
  to GHCR with an SBOM, SLSA build provenance and a keyless cosign signature;
  `deploy/deploy.sh` refuses any image not signed by that workflow
  ([deployment](docs/deployment.md)).
- No `unwrap`/`expect`/`panic!` outside tests (enforced by clippy lints).

## Tests

`make test` runs 20 unit tests (amount parsing and formatting, ids,
configuration), including 5 property-based tests that check the amount parser
on 2,000 generated inputs each, and 63
integration tests that go through the real router against PostgreSQL. Each
integration test gets its own freshly migrated database (`#[sqlx::test]`);
`tests/api/migrations.rs` instead starts from an older schema with data in it
and upgrades it.

Covered: successful operations, malformed and invalid input, missing and
duplicate accounts, insufficient funds, same-account transfers, idempotent
replays (including a request that must wait for another, still uncommitted
request holding the same key on different accounts), ledger immutability and
reconciliation, the busy paths (a lock held past the retries, a statement
timeout, an exhausted connection pool → 503 with `Retry-After`), a retry that
succeeds once a lock is released, a transfer that completes after the client
disconnects, the balance limit, plus three
concurrency tests that give the service a 16-connection pool so 16
transactions really overlap:

- 100 A→B and 100 B→A transfers at once: all succeed, no 5xx, balances unchanged.
- 10 concurrent withdrawals of 30 from a balance of 100: exactly 3 succeed.
- 400 random transfers across 5 accounts: total unchanged, no negative balance,
  every account reconciles with its history, no 5xx.

Each concurrency safeguard was checked by re-introducing the bug and watching
the matching test fail: locking in request order (deadlocks → 503), dropping
`FOR UPDATE` (overspend blocked only by the CHECK → 500), omitting `COMMIT`, and
claiming the idempotency key with a plain insert (→ 500). The property tests
were checked the same way (no normalisation of the output, one decimal place
too many).

Beyond `make test`, CI runs on every push:

- **every-commit**: each commit of the push builds, passes fmt, clippy and the
  tests, and starts and stops cleanly on its own;
- **coverage**: line coverage of all tests (`cargo llvm-cov`), on the run
  summary and as an lcov artifact;
- **load-test**: the Docker image under concurrent HTTP traffic with k6
  (`make loadtest` locally): total balance unchanged, no negative balance, no
  5xx, replays move money once, p95 latency within budget; then SIGTERM must
  drain and exit 0. Details in [loadtest/README.md](loadtest/README.md).

## Limitations and possible improvements

- **Hot accounts.** Transfers touching the same account are serialised by its
  row lock; a single very busy account limits throughput. Options: sub-accounts,
  batching, or an append-only ledger with periodic balance snapshots.
- **Idempotency.** The key is optional and only successful transfers are
  stored; a production API would require it and also persist failed outcomes.
  Keys are kept forever (they live on the ledger row) and are global, not
  scoped per client; with authentication they should be scoped to the caller.
  While the original request is still in flight, reusing its key for a
  transfer to a missing account is answered `404` rather than
  `idempotency_key_reused` (once it has committed, the key check comes first).
- **Scaling.** The service is stateless, so it scales horizontally; correctness
  relies only on PostgreSQL. Beyond one primary: read replicas for balance
  queries, partitioning the ledger by time, and sharding by account (cross-shard
  transfers would need a saga or two-phase protocol).
- **Ledger model.** A double-entry ledger (one debit and one credit row per
  transfer) would be the accounting-standard representation.
- **Not included:** authentication, listing transaction history over HTTP,
  pagination, metrics endpoint (Prometheus), rate limiting inside the service
  (the deployment applies it at the edge), an overall HTTP request timeout,
  multiple currencies.
- **Connection budget.** Each instance opens up to `DB_MAX_CONNECTIONS`;
  `instances × DB_MAX_CONNECTIONS` must stay below PostgreSQL's
  `max_connections` (or put PgBouncer in transaction mode in front).
- Graceful shutdown waits for in-flight requests without its own deadline; it
  relies on the orchestrator's grace period (`stop_grace_period: 30s`).
  Database timeouts bound each step of a transfer, but in the worst case (pool
  wait + lock waits on every one of the 3 attempts) a single transfer can run
  longer than that grace period; an overall request deadline would be the next
  step.
- `transaction_id` values may have gaps (a sequence value is consumed when a
  transfer is rolled back).
