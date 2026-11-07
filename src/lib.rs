//! Internal transfers service: create accounts, query balances and move money
//! between accounts atomically, backed by PostgreSQL.
//!
//! The binary in `main.rs` is a thin wrapper; everything is exposed from this
//! library so the integration tests can build the exact same router.

// No panics on the request path: unwrap/expect/panic are compile errors outside tests.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod routes;
pub mod startup;
