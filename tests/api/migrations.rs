//! Schema upgrades on a database that already holds data. Every other test
//! starts from an empty, fully migrated database; these start from an older
//! schema version, as a deployed release would (ADR 0006).

use crate::helpers::{
    TestApp, given_a_database_migrated_up_to, given_an_account_stored_directly,
    upgrade_to_the_current_schema,
};
use sqlx::PgPool;

/// Version 1 is the original schema; version 2 adds the balance limit.
const SCHEMA_BEFORE_THE_BALANCE_LIMIT: i64 = 1;

#[sqlx::test(migrations = false)]
async fn upgrading_a_database_that_holds_a_balance_past_the_limit_succeeds_and_keeps_it(
    pool: PgPool,
) {
    given_a_database_migrated_up_to(&pool, SCHEMA_BEFORE_THE_BALANCE_LIMIT).await;
    given_an_account_stored_directly(&pool, 1, "199999999999999999999.5").await;

    let upgrade = upgrade_to_the_current_schema(&pool).await;

    assert!(
        upgrade.is_ok(),
        "the upgrade must not fail on old data: {upgrade:?}"
    );
    let app = TestApp::new(pool);
    assert_eq!(app.balance_of(1).await, "199999999999999999999.5");
}

#[sqlx::test(migrations = false)]
async fn after_the_upgrade_new_balances_past_the_limit_are_refused(pool: PgPool) {
    given_a_database_migrated_up_to(&pool, SCHEMA_BEFORE_THE_BALANCE_LIMIT).await;
    given_an_account_stored_directly(&pool, 1, "5").await;

    upgrade_to_the_current_schema(&pool)
        .await
        .expect("upgrade to the current schema");

    let app = TestApp::new(pool);
    let result = app
        .execute_sql_directly("UPDATE accounts SET balance = 1e20 WHERE account_id = 1")
        .await;
    assert!(result.is_err(), "the limit applies to every new write");
    assert_eq!(app.balance_of(1).await, "5");
}
