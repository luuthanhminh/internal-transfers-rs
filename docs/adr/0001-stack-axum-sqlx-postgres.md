# ADR 0001: Rust with axum, sqlx and PostgreSQL

Status: accepted

## Context

The service is small (three business endpoints) but its value is entirely in
correct concurrent money movement. We need an HTTP framework with typed request
extraction and built-in graceful shutdown, a database that gives us row locks,
constraints and transactions we can reason about, and a data-access layer that
lets us write the exact SQL we want.

## Decision

- **axum 0.8** on tokio for HTTP: typed extractors, tower middleware (tracing,
  request ids), `with_graceful_shutdown`.
- **PostgreSQL 17** as the only source of truth: `SELECT … FOR UPDATE`,
  `CHECK` constraints, triggers, unique indexes and `ON CONFLICT`.
- **sqlx 0.9** with plain SQL and runtime-checked queries (`query`/`query_as`),
  embedded migrations (`sqlx::migrate!`) and `#[sqlx::test]` for a fresh
  database per integration test.
- **thiserror** for typed errors, **tracing** for structured logs,
  **utoipa** for an OpenAPI document generated from the code.

## Consequences

- All coordination happens in the database, so correctness does not depend on
  the number of service instances.
- Runtime-checked queries mean the project builds without a running database
  (no `.sqlx` offline cache to keep in sync); SQL errors surface in tests
  instead of at compile time, so integration coverage matters.
- sqlx 0.9 raises the minimum supported Rust version; the toolchain is pinned
  in `rust-toolchain.toml`.

## Alternatives considered

- **actix-web**: equally capable; axum's tower integration and extractor model
  fit better with the rest of the stack.
- **Diesel / SeaORM**: an ORM hides the exact locking SQL that this service is
  about. Compile-time checked `query!` macros were rejected to keep builds
  database-free.
- **In-process mutex per account**: correct for one process only, and blind to
  other database clients. Rejected.
