use super::{sqlstate, sqlstate_of};
use crate::{
    domain::{AccountId, Money},
    error::AppError,
};
use bigdecimal::BigDecimal;
use sqlx::PgPool;

/// Creates an account whose balance starts at `initial_balance`.
///
/// Inserts directly and relies on the primary key to detect duplicates: a
/// "check, then insert" sequence would race when two requests create the
/// same account concurrently.
pub async fn insert(pool: &PgPool, id: AccountId, initial_balance: &Money) -> Result<(), AppError> {
    let result = sqlx::query(
        "INSERT INTO accounts (account_id, initial_balance, balance) VALUES ($1, $2, $2)",
    )
    .bind(id.get())
    .bind(initial_balance.as_decimal())
    .execute(pool)
    .await;

    match result {
        Ok(_) => Ok(()),
        Err(err) if sqlstate_of(&err).as_deref() == Some(sqlstate::UNIQUE_VIOLATION) => {
            Err(AppError::AccountAlreadyExists(id))
        }
        Err(err) => Err(err.into()),
    }
}

/// Current balance of an account, or `None` if it does not exist.
pub async fn find_balance(pool: &PgPool, id: AccountId) -> Result<Option<BigDecimal>, sqlx::Error> {
    sqlx::query_scalar("SELECT balance FROM accounts WHERE account_id = $1")
        .bind(id.get())
        .fetch_optional(pool)
        .await
}
