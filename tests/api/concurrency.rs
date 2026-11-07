//! Many transfers in flight at once. Each request runs on its own task and the
//! service gets a pool of `CONCURRENT_TRANSACTIONS` connections, so that many
//! database transactions genuinely overlap and contend for the same rows (the
//! default `#[sqlx::test]` pool would cap the overlap at 5).
//!
//! A deadlock or lock failure would surface as a 5xx response, so "no 5xx" is
//! how these tests detect broken lock ordering.

use crate::helpers::{TestApp, TestResponse, decimal};
use axum::http::StatusCode;
use sqlx::PgPool;
use std::time::Duration;

const CONCURRENT_TRANSACTIONS: u32 = 16;

/// The service under test with a pool large enough for real overlap.
fn app_with_many_connections(pool: &PgPool) -> TestApp {
    TestApp::with_connection_pool_of(pool, CONCURRENT_TRANSACTIONS, Duration::from_secs(30))
}

/// Fires all transfers concurrently and waits for every response.
async fn run_concurrently(app: &TestApp, transfers: Vec<(i64, i64, String)>) -> Vec<TestResponse> {
    let tasks = transfers.into_iter().map(|(from, to, amount)| {
        let app = app.clone();
        tokio::spawn(async move { app.transfer(from, to, &amount).await })
    });
    futures::future::join_all(tasks)
        .await
        .into_iter()
        .map(|joined| joined.expect("task completes"))
        .collect()
}

fn count(responses: &[TestResponse], status: StatusCode) -> usize {
    responses.iter().filter(|r| r.status == status).count()
}

#[track_caller]
fn assert_no_server_errors(responses: &[TestResponse]) {
    let failures: Vec<_> = responses
        .iter()
        .filter(|r| r.status.is_server_error())
        .map(|r| &r.body)
        .collect();
    assert!(
        failures.is_empty(),
        "server errors (deadlock?): {failures:?}"
    );
}

#[sqlx::test]
async fn opposite_direction_transfers_between_two_accounts_all_succeed_without_deadlock(
    pool: PgPool,
) {
    let app = app_with_many_connections(&pool);
    app.given_account(1, "1000").await;
    app.given_account(2, "1000").await;

    let mut transfers = Vec::new();
    for _ in 0..100 {
        transfers.push((1, 2, "1".to_owned()));
        transfers.push((2, 1, "1".to_owned()));
    }
    let responses = run_concurrently(&app, transfers).await;

    assert_no_server_errors(&responses);
    assert_eq!(count(&responses, StatusCode::CREATED), 200);
    assert_eq!(app.balance_of(1).await, "1000");
    assert_eq!(app.balance_of(2).await, "1000");
    assert_eq!(app.ledger_entry_count().await, 200);
}

#[sqlx::test]
async fn concurrent_withdrawals_never_spend_more_than_the_balance(pool: PgPool) {
    let app = app_with_many_connections(&pool);
    app.given_account(1, "100").await;
    app.given_account(2, "0").await;

    let responses = run_concurrently(&app, vec![(1, 2, "30".to_owned()); 10]).await;

    assert_no_server_errors(&responses);
    assert_eq!(count(&responses, StatusCode::CREATED), 3);
    assert_eq!(count(&responses, StatusCode::UNPROCESSABLE_ENTITY), 7);
    assert_eq!(app.balance_of(1).await, "10");
    assert_eq!(app.balance_of(2).await, "90");
}

#[sqlx::test]
async fn random_concurrent_transfers_preserve_the_total_and_never_go_negative(pool: PgPool) {
    const ACCOUNTS: i64 = 5;
    const TRANSFERS: usize = 400;
    let app = app_with_many_connections(&pool);
    for id in 1..=ACCOUNTS {
        app.given_account(id, "100").await;
    }

    // Deterministic pseudo-random pairs and amounts (1.5 .. 60.5): balances are
    // small, so many transfers hit insufficient funds, and every pair of
    // accounts sees traffic in both directions.
    let mut seed: u64 = 0x5eed;
    let mut next = |bound: u64| {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33) % bound
    };
    let transfers: Vec<(i64, i64, String)> = (0..TRANSFERS)
        .map(|_| {
            let from = next(ACCOUNTS as u64) as i64 + 1;
            let offset = next(ACCOUNTS as u64 - 1) as i64 + 1;
            let to = (from - 1 + offset) % ACCOUNTS + 1;
            (from, to, format!("{}.5", next(60) + 1))
        })
        .collect();

    let responses = run_concurrently(&app, transfers).await;

    assert_no_server_errors(&responses);
    let succeeded = count(&responses, StatusCode::CREATED);
    assert_eq!(
        succeeded + count(&responses, StatusCode::UNPROCESSABLE_ENTITY),
        TRANSFERS
    );
    assert!(succeeded > 0, "the scenario must move some money");
    assert_eq!(app.total_balance().await, decimal("500"));
    assert_eq!(app.ledger_entry_count().await, succeeded as i64);
    app.assert_no_negative_balances().await;
    app.assert_all_accounts_reconcile().await;
}
