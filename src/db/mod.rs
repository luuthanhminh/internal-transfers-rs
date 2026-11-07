//! Database access. All SQL lives in this module.

pub mod accounts;
pub mod transfers;

use std::borrow::Cow;

/// PostgreSQL error codes (SQLSTATE) the service reacts to.
pub mod sqlstate {
    pub const UNIQUE_VIOLATION: &str = "23505";
    pub const SERIALIZATION_FAILURE: &str = "40001";
    pub const DEADLOCK_DETECTED: &str = "40P01";
    pub const LOCK_NOT_AVAILABLE: &str = "55P03";
    pub const QUERY_CANCELED: &str = "57014";
}

/// Round-trip to the database, used by the readiness probe.
pub async fn ping(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(pool).await.map(|_| ())
}

/// The SQLSTATE of a database error, if the error came from PostgreSQL.
pub fn sqlstate_of(err: &sqlx::Error) -> Option<Cow<'_, str>> {
    match err {
        sqlx::Error::Database(db_err) => db_err.code(),
        _ => None,
    }
}

/// Errors after which PostgreSQL guarantees the transaction was rolled back, so
/// re-running the whole transaction is safe. Connection errors are deliberately
/// excluded: a connection lost during COMMIT leaves the outcome unknown.
pub fn is_retryable(err: &sqlx::Error) -> bool {
    matches!(
        sqlstate_of(err).as_deref(),
        Some(
            sqlstate::SERIALIZATION_FAILURE
                | sqlstate::DEADLOCK_DETECTED
                | sqlstate::LOCK_NOT_AVAILABLE
        )
    )
}

/// Errors that mean "busy, try again later" (answered with 503 + Retry-After):
/// the retryable ones above once retries are exhausted, a statement cancelled by
/// `statement_timeout`, and no pooled connection becoming free in time. The last
/// two are not retried in-process: retrying would only lengthen the queue.
pub fn is_temporarily_unavailable(err: &sqlx::Error) -> bool {
    is_retryable(err)
        || matches!(err, sqlx::Error::PoolTimedOut)
        || sqlstate_of(err).as_deref() == Some(sqlstate::QUERY_CANCELED)
}
