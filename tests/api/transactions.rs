use crate::helpers::TestApp;
use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test]
async fn a_transfer_moves_money_and_records_the_transaction(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(123, "100.23344").await;
    app.given_account(456, "0").await;

    app.transfer(123, 456, "100.12345")
        .await
        .assert_status(StatusCode::CREATED);

    assert_eq!(app.balance_of(123).await, "0.10999");
    assert_eq!(app.balance_of(456).await, "100.12345");
    assert_eq!(app.ledger_entry_count().await, 1);
}

#[sqlx::test]
async fn the_transfer_response_describes_the_recorded_transaction(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "10").await;
    app.given_account(2, "0").await;

    let response = app.transfer(1, 2, "2.50").await;
    response.assert_status(StatusCode::CREATED);
    assert!(response.transaction_id() > 0);
    assert_eq!(response.body["source_account_id"], 1);
    assert_eq!(response.body["destination_account_id"], 2);
    assert_eq!(response.body["amount"], "2.5");
    assert!(response.body["created_at"].is_string());
}

#[sqlx::test]
async fn transferring_the_entire_balance_leaves_exactly_zero(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "50.5").await;
    app.given_account(2, "0").await;

    app.transfer(1, 2, "50.5")
        .await
        .assert_status(StatusCode::CREATED);
    assert_eq!(app.balance_of(1).await, "0");
}

#[sqlx::test]
async fn a_transfer_with_insufficient_funds_is_rejected_and_changes_nothing(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "10").await;
    app.given_account(2, "0").await;

    app.transfer(1, 2, "10.00001")
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "insufficient_funds");

    assert_eq!(app.balance_of(1).await, "10");
    assert_eq!(app.balance_of(2).await, "0");
    assert_eq!(app.ledger_entry_count().await, 0);
}

#[sqlx::test]
async fn a_transfer_from_a_missing_account_is_rejected_and_changes_nothing(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(2, "5").await;

    let response = app.transfer(999, 2, "1").await;
    response.assert_error(StatusCode::NOT_FOUND, "account_not_found");
    assert_eq!(
        response.body["error"]["message"],
        "account 999 does not exist"
    );
    assert_eq!(app.balance_of(2).await, "5");
    assert_eq!(app.ledger_entry_count().await, 0);
}

#[sqlx::test]
async fn a_transfer_to_a_missing_account_is_rejected_and_changes_nothing(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "5").await;

    app.transfer(1, 999, "1")
        .await
        .assert_error(StatusCode::NOT_FOUND, "account_not_found");
    assert_eq!(app.balance_of(1).await, "5");
    assert_eq!(app.ledger_entry_count().await, 0);
}

#[sqlx::test]
async fn a_transfer_to_the_same_account_is_rejected(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "5").await;

    app.transfer(1, 1, "1")
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "same_account");
    assert_eq!(app.ledger_entry_count().await, 0);
}

#[sqlx::test]
async fn zero_negative_and_malformed_amounts_are_rejected(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "5").await;
    app.given_account(2, "5").await;

    for amount in ["0", "0.000", "-1", "1e2", "abc", ""] {
        let response = app.transfer(1, 2, amount).await;
        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "amount {amount:?}"
        );
        assert_eq!(
            response.body["error"]["code"], "invalid_amount",
            "amount {amount:?}"
        );
    }
    assert_eq!(app.ledger_entry_count().await, 0);
}

#[sqlx::test]
async fn an_amount_with_too_many_decimal_places_is_rejected_not_rounded(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "5").await;
    app.given_account(2, "5").await;

    let nineteen_decimals = "0.0000000000000000001";
    app.transfer(1, 2, nineteen_decimals)
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_amount");
    assert_eq!(app.balance_of(1).await, "5");
}

#[sqlx::test]
async fn an_amount_sent_as_a_json_number_is_rejected(pool: PgPool) {
    let app = TestApp::new(pool);
    let body = json!({ "source_account_id": 1, "destination_account_id": 2, "amount": 1.5 });
    app.post_json("/transactions", body)
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request");
}

#[sqlx::test]
async fn the_database_refuses_a_negative_balance_even_if_the_application_check_is_bypassed(
    pool: PgPool,
) {
    let app = TestApp::new(pool);
    app.given_account(1, "5").await;

    let result = app
        .execute_sql_directly("UPDATE accounts SET balance = balance - 6 WHERE account_id = 1")
        .await;
    assert!(
        result.is_err(),
        "CHECK (balance >= 0) must reject the update"
    );
    assert_eq!(app.balance_of(1).await, "5");
}

#[sqlx::test]
async fn a_transfer_with_a_non_positive_account_id_is_rejected(pool: PgPool) {
    let app = TestApp::new(pool);
    app.transfer(0, 2, "1")
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_account_id");
    app.transfer(1, -2, "1")
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_account_id");
}

#[sqlx::test]
async fn the_largest_supported_amount_survives_a_round_trip_through_the_database(pool: PgPool) {
    let app = TestApp::new(pool);
    let largest = "99999999999999999999.999999999999999999";
    app.given_account(1, largest).await;
    app.given_account(2, "0").await;

    app.transfer(1, 2, largest)
        .await
        .assert_status(StatusCode::CREATED);

    assert_eq!(app.balance_of(1).await, "0");
    assert_eq!(app.balance_of(2).await, largest);
}

#[sqlx::test]
async fn a_transfer_that_would_push_a_balance_past_the_limit_is_rejected_and_changes_nothing(
    pool: PgPool,
) {
    let app = TestApp::new(pool);
    app.given_account(1, "10").await;
    app.given_account(2, "99999999999999999999.5").await;

    let response = app.transfer(1, 2, "0.5").await;

    response.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "balance_limit_exceeded");
    assert_eq!(app.balance_of(1).await, "10");
    assert_eq!(app.balance_of(2).await, "99999999999999999999.5");
    assert_eq!(app.ledger_entry_count().await, 0);
}

#[sqlx::test]
async fn a_balance_may_reach_just_below_the_limit(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "10").await;
    app.given_account(2, "99999999999999999999").await;

    app.transfer(1, 2, "0.999999999999999999")
        .await
        .assert_status(StatusCode::CREATED);

    assert_eq!(
        app.balance_of(2).await,
        "99999999999999999999.999999999999999999"
    );
}

#[sqlx::test]
async fn the_database_refuses_a_balance_past_the_limit_even_if_the_application_check_is_bypassed(
    pool: PgPool,
) {
    let app = TestApp::new(pool);
    app.given_account(1, "99999999999999999999").await;

    let result = app
        .execute_sql_directly("UPDATE accounts SET balance = balance + 1 WHERE account_id = 1")
        .await;

    assert!(
        result.is_err(),
        "the balance limit constraint must reject it"
    );
    assert_eq!(app.balance_of(1).await, "99999999999999999999");
}
