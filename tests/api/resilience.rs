//! Behaviour when the database is busy: the service must answer 503 with
//! `Retry-After` (the client may retry) instead of a 500, and change nothing.

use crate::helpers::TestApp;
use axum::{http::StatusCode, response::IntoResponse};
use internal_transfers::error::AppError;
use sqlx::PgPool;
use std::time::Duration;

#[sqlx::test]
async fn a_transfer_blocked_by_a_long_held_lock_is_retried_then_answered_with_service_unavailable(
    pool: PgPool,
) {
    let app = TestApp::new(pool);
    app.given_account(1, "100").await;
    app.given_account(2, "0").await;
    let _lock = app.given_another_client_holds_a_lock_on(1).await;

    // Each attempt waits lock_timeout (2s) and is retried, so this takes ~6s.
    // Guards the retry loop and the 503 mapping for lock contention.
    let response = app.transfer(1, 2, "10").await;

    response.assert_error(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable");
    assert_eq!(response.headers["retry-after"], "1");
    drop(_lock);
    assert_eq!(app.balance_of(1).await, "100");
    assert_eq!(app.ledger_entry_count().await, 0);
}

#[sqlx::test]
async fn a_transfer_whose_first_attempt_times_out_on_a_lock_succeeds_on_retry(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "100").await;
    app.given_account(2, "0").await;
    let lock = app.given_another_client_holds_a_lock_on(1).await;

    let transfer = app.start_transfer(1, 2, "10");
    // The first attempt gives up after lock_timeout (2 s); the lock is released
    // only once the retry is queued, so only a retry can produce a success.
    app.wait_until_the_blocked_transfer_is_retried().await;
    drop(lock);

    let response = transfer.await.expect("task completes");
    response.assert_status(StatusCode::CREATED);
    assert_eq!(app.balance_of(1).await, "90");
    assert_eq!(app.balance_of(2).await, "10");
    assert_eq!(app.ledger_entry_count().await, 1);
}

#[sqlx::test]
async fn a_transfer_completes_even_if_the_client_disconnects_while_it_runs(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "100").await;
    app.given_account(2, "0").await;
    let lock = app.given_another_client_holds_a_lock_on(1).await;

    app.transfer_then_disconnect(1, 2, "10").await;
    drop(lock);

    // The transfer runs on its own task, so it finishes instead of being cut
    // off mid-transaction (or mid-COMMIT) together with the request.
    app.wait_until_ledger_has(1).await;
    assert_eq!(app.balance_of(1).await, "90");
    assert_eq!(app.balance_of(2).await, "10");
}

#[sqlx::test]
async fn requests_are_answered_with_service_unavailable_when_no_connection_is_free(pool: PgPool) {
    let app = TestApp::with_connection_pool_of(&pool, 1, Duration::from_millis(300));
    let _busy = app.given_all_database_connections_are_busy().await;

    let response = app.create_account(1, "10").await;

    response.assert_error(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable");
    assert_eq!(response.headers["retry-after"], "1");
}

#[sqlx::test]
async fn a_statement_cancelled_by_statement_timeout_is_answered_with_service_unavailable(
    pool: PgPool,
) {
    let app = TestApp::new(pool);
    let error = app.error_from_a_statement_cancelled_by_timeout().await;

    let response = AppError::Database(error).into_response();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["retry-after"], "1");
}
