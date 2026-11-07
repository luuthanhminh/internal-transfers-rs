use crate::helpers::TestApp;
use axum::http::StatusCode;
use sqlx::PgPool;

#[sqlx::test]
async fn retrying_with_the_same_key_returns_the_original_transfer_without_moving_money_again(
    pool: PgPool,
) {
    let app = TestApp::new(pool);
    app.given_account(1, "100").await;
    app.given_account(2, "0").await;

    let first = app.transfer_with_key(1, 2, "30", "key-1").await;
    let retry = app.transfer_with_key(1, 2, "30", "key-1").await;

    first.assert_status(StatusCode::CREATED);
    retry.assert_status(StatusCode::CREATED);
    assert_eq!(retry.transaction_id(), first.transaction_id());
    assert_eq!(retry.headers["idempotent-replayed"], "true");
    assert_eq!(app.balance_of(1).await, "70");
    assert_eq!(app.ledger_entry_count().await, 1);
}

#[sqlx::test]
async fn reusing_a_key_for_a_different_transfer_is_rejected(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "100").await;
    app.given_account(2, "0").await;
    app.transfer_with_key(1, 2, "30", "key-1")
        .await
        .assert_status(StatusCode::CREATED);

    app.transfer_with_key(1, 2, "31", "key-1")
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "idempotency_key_reused");
    assert_eq!(app.balance_of(1).await, "70");
}

#[sqlx::test]
async fn a_key_from_a_failed_transfer_can_be_used_once_funds_are_available(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "10").await;
    app.given_account(2, "0").await;
    app.given_account(3, "100").await;

    app.transfer_with_key(1, 2, "50", "key-1")
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "insufficient_funds");
    app.transfer(3, 1, "40")
        .await
        .assert_status(StatusCode::CREATED);

    app.transfer_with_key(1, 2, "50", "key-1")
        .await
        .assert_status(StatusCode::CREATED);
    assert_eq!(app.balance_of(2).await, "50");
}

#[sqlx::test]
async fn concurrent_requests_with_the_same_key_transfer_exactly_once(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "100").await;
    app.given_account(2, "0").await;

    let requests = (0..20).map(|_| {
        let app = app.clone();
        tokio::spawn(async move { app.transfer_with_key(1, 2, "10", "same-key").await })
    });
    let responses = futures::future::join_all(requests).await;

    let mut transaction_ids = Vec::new();
    for joined in responses {
        let response = joined.expect("task completes");
        response.assert_status(StatusCode::CREATED);
        transaction_ids.push(response.transaction_id());
    }
    transaction_ids.dedup();
    assert_eq!(
        transaction_ids.len(),
        1,
        "every response must describe the same transfer"
    );
    assert_eq!(app.balance_of(1).await, "90");
    assert_eq!(app.ledger_entry_count().await, 1);
}

#[sqlx::test]
async fn an_invalid_idempotency_key_is_rejected(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "100").await;
    app.given_account(2, "0").await;

    let too_long = "k".repeat(256);
    for key in ["", "has space", too_long.as_str()] {
        let response = app.transfer_with_key(1, 2, "1", key).await;
        assert_eq!(response.status, StatusCode::BAD_REQUEST, "key {key:?}");
        assert_eq!(response.body["error"]["code"], "invalid_idempotency_key");
    }
    assert_eq!(app.ledger_entry_count().await, 0);
}

#[sqlx::test]
async fn requests_without_a_key_are_independent_transfers(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "100").await;
    app.given_account(2, "0").await;

    app.transfer(1, 2, "10")
        .await
        .assert_status(StatusCode::CREATED);
    app.transfer(1, 2, "10")
        .await
        .assert_status(StatusCode::CREATED);
    assert_eq!(app.balance_of(1).await, "80");
}

#[sqlx::test]
async fn replaying_a_completed_transfer_succeeds_even_after_the_source_was_drained(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "100").await;
    app.given_account(2, "0").await;

    let first = app.transfer_with_key(1, 2, "100", "drain").await;
    let retry = app.transfer_with_key(1, 2, "100", "drain").await;

    first.assert_status(StatusCode::CREATED);
    retry.assert_status(StatusCode::CREATED);
    assert_eq!(retry.transaction_id(), first.transaction_id());
    assert_eq!(app.balance_of(1).await, "0");
}

#[sqlx::test]
async fn reusing_a_key_with_a_different_destination_is_rejected(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "100").await;
    app.given_account(2, "0").await;
    app.given_account(3, "0").await;
    app.transfer_with_key(1, 2, "10", "key-1")
        .await
        .assert_status(StatusCode::CREATED);

    app.transfer_with_key(1, 3, "10", "key-1")
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "idempotency_key_reused");
    assert_eq!(app.balance_of(3).await, "0");
}

#[sqlx::test]
async fn concurrent_requests_sharing_a_key_with_different_bodies_transfer_exactly_once(
    pool: PgPool,
) {
    let app = TestApp::new(pool);
    app.given_account(1, "1000").await;
    app.given_account(2, "0").await;

    let requests = (1..=10).map(|amount| {
        let app = app.clone();
        tokio::spawn(async move {
            app.transfer_with_key(1, 2, &amount.to_string(), "shared")
                .await
                .status
        })
    });
    let statuses: Vec<StatusCode> = futures::future::join_all(requests)
        .await
        .into_iter()
        .map(|joined| joined.expect("task completes"))
        .collect();

    assert_eq!(
        statuses
            .iter()
            .filter(|s| **s == StatusCode::CREATED)
            .count(),
        1
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|s| **s == StatusCode::UNPROCESSABLE_ENTITY)
            .count(),
        9
    );
    assert_eq!(app.ledger_entry_count().await, 1);
}

/// Same key on account pairs that share no row lock: the second request must
/// wait for the in-flight claim of the key, then report the conflict.
#[sqlx::test]
async fn a_key_claimed_by_an_in_flight_transfer_on_other_accounts_is_rejected_once_it_commits(
    pool: PgPool,
) {
    let app = TestApp::new(pool);
    for id in 1..=4 {
        app.given_account(id, "100").await;
    }
    let in_flight = app
        .given_another_request_is_mid_transfer_with_key(1, 2, "10", "shared")
        .await;

    let second = {
        let app = app.clone();
        tokio::spawn(async move { app.transfer_with_key(3, 4, "10", "shared").await })
    };
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(!second.is_finished(), "must wait for the in-flight key");
    in_flight.commit().await.expect("commit in-flight transfer");

    let response = second.await.expect("task completes");
    response.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "idempotency_key_reused");
    assert_eq!(app.balance_of(3).await, "100");
}

#[sqlx::test]
async fn a_key_released_by_a_rolled_back_transfer_can_be_used_by_the_waiting_request(pool: PgPool) {
    let app = TestApp::new(pool);
    for id in 1..=4 {
        app.given_account(id, "100").await;
    }
    let in_flight = app
        .given_another_request_is_mid_transfer_with_key(1, 2, "10", "shared")
        .await;

    let second = {
        let app = app.clone();
        tokio::spawn(async move { app.transfer_with_key(3, 4, "10", "shared").await })
    };
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    in_flight
        .rollback()
        .await
        .expect("roll back in-flight transfer");

    let response = second.await.expect("task completes");
    response.assert_status(StatusCode::CREATED);
    assert_eq!(app.balance_of(4).await, "110");
    assert_eq!(app.ledger_entry_count().await, 1);
}

#[sqlx::test]
async fn a_replay_is_answered_at_once_even_while_the_accounts_are_locked(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "100").await;
    app.given_account(2, "0").await;
    let first = app.transfer_with_key(1, 2, "10", "key-1").await;
    let _lock = app.given_another_client_holds_a_lock_on(1).await;

    let retry = app.transfer_with_key(1, 2, "10", "key-1").await;

    retry.assert_status(StatusCode::CREATED);
    assert_eq!(retry.transaction_id(), first.transaction_id());
}
