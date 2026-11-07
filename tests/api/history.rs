use crate::helpers::TestApp;
use axum::http::StatusCode;
use sqlx::PgPool;

#[sqlx::test]
async fn transaction_history_cannot_be_updated_deleted_or_truncated(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "10").await;
    app.given_account(2, "0").await;
    app.transfer(1, 2, "4")
        .await
        .assert_status(StatusCode::CREATED);

    for sql in [
        "UPDATE transactions SET amount = 1",
        "DELETE FROM transactions",
        "TRUNCATE transactions CASCADE",
    ] {
        assert!(
            app.execute_sql_directly(sql).await.is_err(),
            "{sql} must be refused"
        );
    }
    assert_eq!(app.ledger_entry_count().await, 1);
}

#[sqlx::test]
async fn an_initial_balance_cannot_be_changed_after_creation(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "10").await;

    let result = app
        .execute_sql_directly("UPDATE accounts SET initial_balance = 1000 WHERE account_id = 1")
        .await;
    assert!(result.is_err(), "initial_balance must be immutable");
}

#[sqlx::test]
async fn every_balance_reconciles_with_its_initial_balance_and_history(pool: PgPool) {
    let app = TestApp::new(pool);
    app.given_account(1, "100.5").await;
    app.given_account(2, "20").await;
    app.given_account(3, "0").await;

    app.transfer(1, 2, "10.25")
        .await
        .assert_status(StatusCode::CREATED);
    app.transfer(2, 3, "30.25")
        .await
        .assert_status(StatusCode::CREATED);
    app.transfer(3, 1, "0.5")
        .await
        .assert_status(StatusCode::CREATED);
    app.transfer(3, 2, "1000")
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);

    app.assert_all_accounts_reconcile().await;
}
