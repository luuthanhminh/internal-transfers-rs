//! Integration tests against a real PostgreSQL through the real HTTP router.
//!
//! `#[sqlx::test]` creates a fresh database per test and applies the migrations,
//! so tests are isolated and can run in parallel. Requires `DATABASE_URL` to
//! point at a server where the user may create databases (see README).
//!
//! All test modules live in one binary to keep link time down.

mod accounts;
mod api_docs;
mod concurrency;
mod helpers;
mod history;
mod idempotency;
mod migrations;
mod operations;
mod resilience;
mod transactions;
