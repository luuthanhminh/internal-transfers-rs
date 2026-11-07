use crate::helpers::TestApp;
use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;

#[sqlx::test]
async fn creating_an_account_returns_created_with_an_empty_body(pool: PgPool) {
    let app = TestApp::new(pool);
    let response = app.create_account(123, "100.23344").await;
    response.assert_status(StatusCode::CREATED);
    assert_eq!(response.body, Value::Null);
}

#[sqlx::test]
async fn a_created_account_reports_its_initial_balance(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(123, "100.23344").await;

    let response = app.get_account("123").await;
    response.assert_status(StatusCode::OK);
    assert_eq!(
        response.body,
        json!({ "account_id": 123, "balance": "100.23344" })
    );
}

#[sqlx::test]
async fn an_account_may_start_with_a_zero_balance(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "0").await;
    assert_eq!(app.balance_of(1).await, "0");
}

#[sqlx::test]
async fn creating_a_duplicate_account_is_rejected_and_keeps_the_original(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(123, "100").await;

    app.create_account(123, "5")
        .await
        .assert_error(StatusCode::CONFLICT, "account_already_exists");
    assert_eq!(app.balance_of(123).await, "100");
}

#[sqlx::test]
async fn creating_the_same_account_concurrently_succeeds_exactly_once(pool: PgPool) {
    let app = TestApp::new(pool);
    let attempts = (0..10).map(|_| {
        let app = app.clone();
        tokio::spawn(async move { app.create_account(42, "10").await.status })
    });
    let statuses: Vec<StatusCode> = futures::future::join_all(attempts)
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
    assert!(
        statuses
            .iter()
            .all(|s| *s == StatusCode::CREATED || *s == StatusCode::CONFLICT)
    );
}

#[sqlx::test]
async fn a_negative_initial_balance_is_rejected(pool: PgPool) {
    let app = TestApp::new(pool);
    let response = app.create_account(1, "-5").await;

    response.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_amount");
    assert_eq!(
        response.body["error"]["message"],
        "amount must not be negative"
    );
}

#[sqlx::test]
async fn malformed_amount_strings_are_rejected(pool: PgPool) {
    let app = TestApp::new(pool);
    for amount in [
        "", "abc", "1e5", "+5", ".5", "5.", "1_000", "00012", " 1", "NaN",
    ] {
        let response = app.create_account(1, amount).await;
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
}

#[sqlx::test]
async fn an_initial_balance_sent_as_a_json_number_is_rejected(pool: PgPool) {
    let app = TestApp::new(pool);
    let response = app
        .post_json(
            "/accounts",
            json!({ "account_id": 1, "initial_balance": 100.5 }),
        )
        .await;
    response.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request");
}

#[sqlx::test]
async fn a_non_positive_account_id_is_rejected(pool: PgPool) {
    let app = TestApp::new(pool);
    app.create_account(0, "1")
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_account_id");
    app.create_account(-7, "1")
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_account_id");
}

#[sqlx::test]
async fn an_account_id_beyond_64_bits_is_rejected(pool: PgPool) {
    let app = TestApp::new(pool);
    let body = r#"{"account_id": 9223372036854775808, "initial_balance": "1"}"#;
    let response = app
        .post_raw("/accounts", body, Some("application/json"))
        .await;
    response.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request");
}

#[sqlx::test]
async fn querying_a_missing_account_returns_not_found(pool: PgPool) {
    let app = TestApp::new(pool);
    app.get_account("999")
        .await
        .assert_error(StatusCode::NOT_FOUND, "account_not_found");
}

#[sqlx::test]
async fn querying_with_a_non_numeric_id_returns_a_json_error(pool: PgPool) {
    let app = TestApp::new(pool);
    app.get_account("abc")
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid_request");
}

#[sqlx::test]
async fn a_malformed_json_body_returns_a_json_error(pool: PgPool) {
    let app = TestApp::new(pool);
    let response = app
        .post_raw("/accounts", "{not json", Some("application/json"))
        .await;
    response.assert_error(StatusCode::BAD_REQUEST, "invalid_request");
}

#[sqlx::test]
async fn a_missing_field_returns_a_json_error(pool: PgPool) {
    let app = TestApp::new(pool);
    let response = app.post_json("/accounts", json!({ "account_id": 1 })).await;
    response.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request");
}

#[sqlx::test]
async fn a_missing_content_type_returns_a_json_error(pool: PgPool) {
    let app = TestApp::new(pool);
    let response = app
        .post_raw(
            "/accounts",
            r#"{"account_id":1,"initial_balance":"1"}"#,
            None,
        )
        .await;
    response.assert_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "invalid_request");
}
