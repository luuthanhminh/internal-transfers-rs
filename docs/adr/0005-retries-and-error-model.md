# ADR 0005: Retry only provably rolled-back errors; one JSON error model

Status: accepted

## Context

Even with ordered locking, a transfer can fail transiently (lock timeout under
heavy contention, serialization failure, deadlock if an invariant is ever
broken). Some failures are safe to retry, others are not: a connection lost
during `COMMIT` leaves the outcome unknown. Clients also need errors they can
act on, with stable codes, and no internal details.

## Decision

Retries

- The whole transfer transaction is re-run, up to 3 attempts with jittered
  backoff, only for SQLSTATE `40001` (serialization failure), `40P01`
  (deadlock) and `55P03` (lock not available) — errors after which PostgreSQL
  guarantees the transaction was rolled back. Each retry is logged as a
  warning so a regression is visible.
- Connection errors are never retried in-process; the idempotency key
  ([ADR 0004](0004-idempotency-key.md)) is how a client retries safely.
- `SET LOCAL lock_timeout` bounds each lock wait.

```mermaid
flowchart LR
    run[["run the transfer<br/>transaction"]]:::core --> res{"outcome"}:::check
    res -- "40001 · 40P01 · 55P03<br/>rolled back by PostgreSQL" --> left{"attempt &lt; 3?"}:::check
    left -- "yes · back off with jitter" --> run
    left -- no --> busy["503 temporarily_unavailable<br/><small>Retry-After: 1</small>"]:::busy
    res -- "57014 statement timeout<br/>or pool exhausted" --> busy
    res -- committed --> ok["201 Created"]:::ok
    res -- "404 · 422<br/>business error" --> biz["returned as is<br/><small>never retried</small>"]:::err
    res -- "connection lost<br/>maybe during COMMIT" --> unk["500 · outcome unknown<br/><small>client retries with its Idempotency-Key</small>"]:::err

    classDef core  fill:#DBEAFE,stroke:#1D4ED8,stroke-width:2.5px,color:#1E3A8A
    classDef check fill:#FEF9C3,stroke:#CA8A04,stroke-width:1.5px,color:#713F12
    classDef ok    fill:#DCFCE7,stroke:#16A34A,stroke-width:1.5px,color:#14532D
    classDef err   fill:#FEE2E2,stroke:#DC2626,stroke-width:1.5px,color:#7F1D1D
    classDef busy  fill:#FFEDD5,stroke:#EA580C,stroke-width:1.5px,color:#7C2D12
```

Error model

- One `AppError` enum; each variant maps to a status and a stable `code` in one
  place. Every error body is `{"error": {"code": "...", "message": "..."}}`.
- Extractor rejections (malformed JSON, missing `Content-Type`, bad path
  parameter) are converted to the same JSON shape.
- `400` malformed request, `404` unknown account, `409` duplicate account,
  `415` wrong content type, `422` well-formed but invalid (amount, id, same
  account, insufficient funds, key reuse), `503` busy (with `Retry-After`),
  `500` unexpected. 5xx bodies never contain database or internal details;
  those go to the logs.
- No `unwrap`/`expect`/`panic!` outside tests, enforced by clippy.

## Consequences

- A busy database produces a retryable `503`, not an opaque `500`.
- The worst-case latency of a transfer is bounded by attempts × lock timeout.

## Alternatives considered

- **Retry every database error**: can transfer twice after an ambiguous
  `COMMIT` failure.
- **No retries**: correct, but turns rare transient errors into client-visible
  failures.
- **Problem Details (RFC 9457)**: a reasonable standard; the simpler
  `{error: {code, message}}` shape was enough here.
