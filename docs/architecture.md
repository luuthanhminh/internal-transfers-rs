# Architecture

How the service is put together and how a request flows through it. The
*why* behind each decision is in the [ADRs](adr/); this page describes the
*what*.

## Components

```mermaid
flowchart LR
    client(["HTTP client"]):::ext

    subgraph svc["internal-transfers · axum + tokio"]
        direction LR
        mw["tower layers<br/><small>request id · trace span</small>"]:::edge
        routes["routes/<br/><small>extract · validate · OpenAPI</small>"]:::app
        domain["domain/<br/><small>Money · AccountId</small>"]:::app
        db["db/<br/><small>all SQL · transfer transaction</small>"]:::core
        err["error.rs<br/><small>AppError → JSON</small>"]:::err
    end

    pg[("PostgreSQL 17<br/><small>accounts · transactions</small>")]:::store

    client -->|"JSON / HTTP"| mw --> routes
    routes -->|"parse"| domain
    routes -->|"call"| db
    routes -.->|"on error"| err
    db -->|"sqlx pool"| pg

    classDef ext   fill:#F1F5F9,stroke:#64748B,stroke-width:1.5px,color:#0F172A
    classDef edge  fill:#EDE9FE,stroke:#7C3AED,stroke-width:1.5px,color:#3B0764
    classDef app   fill:#E0F2FE,stroke:#0284C7,stroke-width:1.5px,color:#0C4A6E
    classDef core  fill:#DBEAFE,stroke:#1D4ED8,stroke-width:2px,color:#1E3A8A
    classDef err   fill:#FEE2E2,stroke:#DC2626,stroke-width:1.5px,color:#7F1D1D
    classDef store fill:#DCFCE7,stroke:#16A34A,stroke-width:2px,color:#14532D
    style svc fill:transparent,stroke:#94A3B8,stroke-dasharray:4 3
```

`config.rs` turns the environment into a typed `Config` at startup and
`startup.rs` wires the pool, migrations, server and graceful shutdown.

| Module | Responsibility | Depends on |
|---|---|---|
| `main.rs` | Parse config, init logging, dispatch `serve` / `migrate` | `config`, `startup` |
| `startup.rs` | Connection pool (TLS via URL), migrations, server, SIGTERM/Ctrl-C drain | `routes` |
| `config.rs` | Environment variables → `Config`; password redacted in `Debug` | — |
| `routes/` | HTTP shapes, extractors, OpenAPI annotations; no SQL | `domain`, `db`, `error` |
| `domain/` | `Money` (strict decimal strings), `AccountId` (positive i64) | `bigdecimal` |
| `db/` | SQL, SQLSTATE classification (retryable / busy), transfer algorithm | `domain`, `error` |
| `error.rs` | One error enum, one status/code table, no internals in 5xx | `domain`, `db` |

The library (`lib.rs`) exposes everything; the binary is a thin wrapper, so the
integration tests build exactly the router that production runs.

## Request flow: `POST /transactions`

Everything above the dashed line is checked without touching the database.

```mermaid
flowchart TD
    req(["POST /transactions"]):::ext --> v

    subgraph nodb["checked before touching the database"]
        v["validate the request<br/><small>JSON body · ids &gt; 0 · source ≠ destination<br/>amount &gt; 0 with ≤ 18 decimals · Idempotency-Key format</small>"]:::check
    end
    v -- invalid --> e4xx["400 · 415 · 422<br/><small>invalid_request · invalid_account_id · same_account<br/>invalid_amount · invalid_idempotency_key</small>"]:::err
    v -- valid --> spawn["tokio::spawn<br/><small>a client disconnect cannot cancel COMMIT</small>"]:::app

    subgraph indb["PostgreSQL"]
        seen{"Idempotency-Key<br/>already recorded?"}:::db
        txn[["transfer transaction<br/><small>lock both rows · check · update · up to 3 attempts</small>"]]:::core
    end
    spawn --> seen
    seen -- "yes, same transfer" --> replay["201 replay<br/><small>Idempotent-Replayed: true</small>"]:::ok
    seen -- "yes, different transfer" --> reused["422 idempotency_key_reused"]:::err
    seen -- "no key / new key" --> txn

    txn -- committed --> ok201["201 + transfer record"]:::ok
    txn -- "account missing" --> e404["404 account_not_found"]:::err
    txn -- "balance too low" --> e422["422 insufficient_funds"]:::err
    txn -- "destination would reach 10^20" --> elim["422 balance_limit_exceeded"]:::err
    txn -- "still contended" --> e503["503 temporarily_unavailable<br/><small>Retry-After: 1</small>"]:::busy

    classDef ext   fill:#F1F5F9,stroke:#64748B,stroke-width:1.5px,color:#0F172A
    classDef check fill:#FEF9C3,stroke:#CA8A04,stroke-width:1.5px,color:#713F12
    classDef app   fill:#E0F2FE,stroke:#0284C7,stroke-width:1.5px,color:#0C4A6E
    classDef db    fill:#DBEAFE,stroke:#1D4ED8,stroke-width:1.5px,color:#1E3A8A
    classDef core  fill:#DBEAFE,stroke:#1D4ED8,stroke-width:2.5px,color:#1E3A8A
    classDef ok    fill:#DCFCE7,stroke:#16A34A,stroke-width:1.5px,color:#14532D
    classDef err   fill:#FEE2E2,stroke:#DC2626,stroke-width:1.5px,color:#7F1D1D
    classDef busy  fill:#FFEDD5,stroke:#EA580C,stroke-width:1.5px,color:#7C2D12
    style nodb fill:transparent,stroke:#CA8A04,stroke-dasharray:4 3
    style indb fill:transparent,stroke:#1D4ED8,stroke-dasharray:4 3
```

The response always carries `X-Request-Id` (the caller's value or a new UUID).

## Transfer transaction

`src/db/transfers.rs`, one transaction at `READ COMMITTED`:

```mermaid
sequenceDiagram
    autonumber
    participant H as Handler task
    participant PG as PostgreSQL

    H->>PG: BEGIN · SET LOCAL lock_timeout 2s, statement_timeout 10s
    rect rgba(37, 99, 235, 0.10)
        H->>PG: SELECT … WHERE account_id = ANY($ids)<br/>ORDER BY account_id FOR UPDATE
        PG-->>H: both rows, locked lowest id first
    end
    alt an account is missing
        H->>PG: ROLLBACK
        Note over H: 404 account_not_found
    else
        H->>PG: INSERT ledger row … ON CONFLICT (idempotency_key) DO NOTHING
        alt key already taken
            H->>PG: ROLLBACK
            Note over H: replay (201) or 422
        else balance below amount (read under the lock)
            H->>PG: ROLLBACK
            Note over H: 422 insufficient_funds
        else destination balance + amount reaches 10^20
            H->>PG: ROLLBACK
            Note over H: 422 balance_limit_exceeded
        else
            rect rgba(22, 163, 74, 0.10)
                H->>PG: UPDATE balance = balance − amount (source)
                H->>PG: UPDATE balance = balance + amount (destination)
                H->>PG: COMMIT
            end
            Note over H: 201 Created
        end
    end
```

| Step | SQL | On failure |
|---|---|---|
| 1 | `BEGIN; SET LOCAL lock_timeout = '2s'; SET LOCAL statement_timeout = '10s'` | — |
| 2 | `SELECT account_id, balance FROM accounts WHERE account_id = ANY($ids) ORDER BY account_id FOR UPDATE` | lock timeout → retry, then 503 |
| 3 | Both rows present? | `ROLLBACK`, 404 `account_not_found` |
| 4 | `INSERT INTO transactions … ON CONFLICT (idempotency_key) DO NOTHING RETURNING …` | no row → `ROLLBACK`, replay or 422 |
| 5 | `balance >= amount` (source) and `balance + amount < 10^20` (destination), read under the lock | `ROLLBACK`, 422 `insufficient_funds` / `balance_limit_exceeded` |
| 6 | `UPDATE accounts SET balance = balance - $amount` (source), `+ $amount` (destination) | `CHECK (balance >= 0)` and `CHECK (balance < 1e20)` as backstops |
| 7 | `COMMIT` | connection error → not retried; client retries with its key |

Locks are always taken lowest id first, so transfers in opposite directions
cannot deadlock ([ADR 0003](adr/0003-ordered-row-locking.md)). Every statement
runs on the transaction's connection; functions in `transfers.rs` take
`&mut PgConnection`, never the pool.

Two opposite transfers at the same moment, A = account 1, B = account 2:

```mermaid
sequenceDiagram
    participant T1 as T1 · 1 → 2
    participant A as row 1
    participant B as row 2
    participant T2 as T2 · 2 → 1

    par both start
        T1->>A: lock (lowest id first)
    and
        T2->>A: lock (lowest id first) — waits for T1
    end
    T1->>B: lock
    Note over T1: debit 1, credit 2, COMMIT
    T1-->>A: release
    T1-->>B: release
    T2->>A: lock granted
    T2->>B: lock
    Note over T2: sees T1's balances · debit 2, credit 1, COMMIT
```

Both transfers ask for row 1 first, so one simply waits for the other: no
cycle, no deadlock, and the second sees the first one's result.

Error classification (`db/mod.rs`):

| SQLSTATE / error | Meaning | Handling |
|---|---|---|
| `40001`, `40P01`, `55P03` | Rolled back by PostgreSQL | Re-run the transaction (3 attempts), then 503 |
| `57014`, pool acquire timeout | Busy | 503 + `Retry-After: 1`, not retried in-process |
| `23505` on `accounts` | Duplicate id | 409 `account_already_exists` |
| anything else | Unexpected | 500, details only in logs |

## Schema

```mermaid
erDiagram
    accounts ||--o{ transactions : "source_account_id"
    accounts ||--o{ transactions : "destination_account_id"

    accounts {
        BIGINT account_id PK "> 0, chosen by the client"
        NUMERIC initial_balance "≥ 0, immutable (trigger)"
        NUMERIC balance "≥ 0 (CHECK)"
        TIMESTAMPTZ created_at
    }
    transactions {
        BIGSERIAL transaction_id PK
        BIGINT source_account_id FK "ON DELETE RESTRICT"
        BIGINT destination_account_id FK "≠ source (CHECK)"
        NUMERIC amount "> 0"
        TEXT idempotency_key UK "nullable"
        TIMESTAMPTZ created_at
    }
```

- Triggers reject `UPDATE`, `DELETE` and `TRUNCATE` on `transactions`, and any
  change of `accounts.initial_balance`.
- Amount columns are unconstrained `NUMERIC`: `NUMERIC(p, s)` would round.
- Indexes on both account columns of `transactions` keep reconciliation cheap.
- Invariant, checked by tests:
  `balance = initial_balance + Σ incoming − Σ outgoing` for every account.

## Runtime and deployment

| Concern | How |
|---|---|
| Configuration | Environment only (`DATABASE_URL`, `BIND_ADDR`, `DB_MAX_CONNECTIONS`, `DB_ACQUIRE_TIMEOUT_SECS`, `RUN_MIGRATIONS`, `LOG_FORMAT`, `RUST_LOG`) |
| Migrations | Embedded; applied on startup (advisory lock) or as `internal-transfers migrate` |
| Probes | `/health` liveness (no DB), `/ready` readiness (DB ping) |
| Logs | stdout, text or JSON, request id on every request span |
| Shutdown | SIGTERM → stop accepting, drain in-flight requests, close pool |
| Image | Pinned multi-stage build, non-root user, exec-form `CMD` |
| Scaling | Stateless; all coordination in PostgreSQL. Budget: `instances × DB_MAX_CONNECTIONS < max_connections` |
