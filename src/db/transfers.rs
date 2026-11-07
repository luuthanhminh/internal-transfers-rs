//! The transfer transaction.
//!
//! Correctness under concurrency rests on four rules:
//! 1. Both account rows are locked with `SELECT ... FOR UPDATE` in ascending
//!    `account_id` order. Every transfer takes locks in the same global order,
//!    so A→B and B→A running together cannot deadlock.
//! 2. The balance is checked only after the lock is held, so it cannot change
//!    between the check and the update.
//! 3. Balances are updated relatively (`balance = balance - $amount`), and the
//!    `CHECK (balance >= 0)` constraint backs up the application check.
//! 4. The ledger row and both balance updates commit in one transaction.
//!
//! The destination balance must also stay below `balance_limit()` (10^20),
//! checked under the same lock and backed by a database CHECK.
//!
//! Every statement goes through `&mut PgConnection` taken from the transaction;
//! functions here never accept the pool, so nothing can run outside it by accident.

use super::is_retryable;
use crate::{
    domain::{AccountId, Money, balance_limit},
    error::AppError,
};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_ATTEMPTS: u32 = 3;

pub struct TransferRequest {
    pub source: AccountId,
    pub destination: AccountId,
    pub amount: Money,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TransactionRecord {
    pub transaction_id: i64,
    pub source_account_id: i64,
    pub destination_account_id: i64,
    pub amount: BigDecimal,
    pub created_at: DateTime<Utc>,
}

pub enum TransferOutcome {
    /// The transfer was executed by this request.
    Created(TransactionRecord),
    /// The Idempotency-Key was already used for this exact transfer; nothing was moved again.
    Replayed(TransactionRecord),
}

/// Executes a transfer, re-running the whole transaction on errors after which
/// PostgreSQL guarantees a rollback (deadlock, serialization failure, lock
/// timeout). With ordered locking these should not happen; the retry is a
/// safety net and is logged so a regression is visible.
pub async fn transfer(
    pool: &PgPool,
    request: &TransferRequest,
) -> Result<TransferOutcome, AppError> {
    // Fast path for retries: a key that is already recorded is answered without
    // taking any account lock, so a replay never waits behind (or 503s on) busy
    // accounts. Two requests racing with a fresh key are settled inside the
    // transaction by `ON CONFLICT`.
    if request.idempotency_key.is_some()
        && let Some(existing) = find_by_idempotency_key(pool, request).await?
    {
        return replay_of(existing, request);
    }

    let mut attempt = 1;
    loop {
        match try_transfer(pool, request).await {
            Err(AppError::Database(err)) if is_retryable(&err) && attempt < MAX_ATTEMPTS => {
                tracing::warn!(attempt, error = %err, "transient database error, retrying transfer");
                tokio::time::sleep(backoff(attempt)).await;
                attempt += 1;
            }
            outcome => return outcome,
        }
    }
}

async fn try_transfer(
    pool: &PgPool,
    request: &TransferRequest,
) -> Result<TransferOutcome, AppError> {
    let mut tx = pool.begin().await?;
    // Bound waits on contended rows; SET LOCAL ends with the transaction.
    // lock_timeout applies to each individual wait, and locking two rows typically
    // waits up to four times (row lock, then the holder's transaction, per row), so
    // statement_timeout is set above 4 × lock_timeout to let lock waits fail first.
    // Either way the client gets 503.
    sqlx::query("SET LOCAL lock_timeout = '2s'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL statement_timeout = '10s'")
        .execute(&mut *tx)
        .await?;

    let locked = lock_accounts(&mut tx, request.source, request.destination).await?;
    let Some(source_balance) = balance_of(&locked, request.source) else {
        rollback(tx).await;
        return Err(AppError::AccountNotFound(request.source));
    };
    let Some(destination_balance) = balance_of(&locked, request.destination) else {
        rollback(tx).await;
        return Err(AppError::AccountNotFound(request.destination));
    };

    // Record the transfer (claiming the idempotency key) before moving money, so a
    // duplicate request is detected without touching balances.
    let Some(record) = insert_ledger_entry(&mut tx, request).await? else {
        rollback(tx).await;
        return replay(pool, request).await;
    };

    if source_balance < request.amount.as_decimal() {
        rollback(tx).await;
        return Err(AppError::InsufficientFunds(request.source));
    }
    // Checked under the lock like the source balance; the database CHECK backs it up.
    if destination_balance + request.amount.as_decimal() >= balance_limit() {
        rollback(tx).await;
        return Err(AppError::BalanceLimitExceeded(request.destination));
    }

    apply_balance_delta(&mut tx, request.source, &-request.amount.as_decimal()).await?;
    apply_balance_delta(&mut tx, request.destination, request.amount.as_decimal()).await?;
    tx.commit().await?;

    Ok(TransferOutcome::Created(record))
}

/// Ends a transaction that will not be committed. The explicit rollback hands a
/// clean connection back to the pool at once; if it fails (e.g. the connection
/// dropped) the business error being returned must not be replaced by a 500 —
/// PostgreSQL discards the uncommitted work either way.
async fn rollback(tx: Transaction<'_, Postgres>) {
    if let Err(err) = tx.rollback().await {
        tracing::warn!(error = %err, "rollback failed; uncommitted work is discarded");
    }
}

/// Locks both accounts in ascending id order and returns `(account_id, balance)`
/// for those that exist.
async fn lock_accounts(
    conn: &mut PgConnection,
    source: AccountId,
    destination: AccountId,
) -> Result<Vec<(i64, BigDecimal)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT account_id, balance FROM accounts \
         WHERE account_id = ANY($1) ORDER BY account_id FOR UPDATE",
    )
    .bind(vec![source.get(), destination.get()])
    .fetch_all(conn)
    .await
}

fn balance_of(locked: &[(i64, BigDecimal)], id: AccountId) -> Option<&BigDecimal> {
    locked
        .iter()
        .find(|(account_id, _)| *account_id == id.get())
        .map(|(_, balance)| balance)
}

/// Inserts the ledger row. Returns `None` when the idempotency key is already
/// taken. `ON CONFLICT DO NOTHING` keeps the transaction usable, whereas a plain
/// INSERT would abort it with a unique violation. If another transaction holding
/// the same key is still in flight, PostgreSQL waits for it to finish first.
async fn insert_ledger_entry(
    conn: &mut PgConnection,
    request: &TransferRequest,
) -> Result<Option<TransactionRecord>, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO transactions \
             (source_account_id, destination_account_id, amount, idempotency_key) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (idempotency_key) DO NOTHING \
         RETURNING transaction_id, source_account_id, destination_account_id, amount, created_at",
    )
    .bind(request.source.get())
    .bind(request.destination.get())
    .bind(request.amount.as_decimal())
    .bind(request.idempotency_key.as_deref())
    .fetch_optional(conn)
    .await
}

async fn apply_balance_delta(
    conn: &mut PgConnection,
    id: AccountId,
    delta: &BigDecimal,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE accounts SET balance = balance + $1 WHERE account_id = $2")
        .bind(delta)
        .bind(id.get())
        .execute(conn)
        .await?;
    Ok(())
}

/// The transfer previously recorded under this request's idempotency key, if any.
async fn find_by_idempotency_key(
    pool: &PgPool,
    request: &TransferRequest,
) -> Result<Option<TransactionRecord>, sqlx::Error> {
    sqlx::query_as(
        "SELECT transaction_id, source_account_id, destination_account_id, amount, created_at \
         FROM transactions WHERE idempotency_key = $1",
    )
    .bind(request.idempotency_key.as_deref())
    .fetch_optional(pool)
    .await
}

/// Answers a request whose idempotency key is already taken: the original
/// transfer if it matches this request, an error if the key is being reused for
/// a different transfer.
fn replay_of(
    existing: TransactionRecord,
    request: &TransferRequest,
) -> Result<TransferOutcome, AppError> {
    let same_transfer = existing.source_account_id == request.source.get()
        && existing.destination_account_id == request.destination.get()
        && &existing.amount == request.amount.as_decimal();

    if same_transfer {
        Ok(TransferOutcome::Replayed(existing))
    } else {
        Err(AppError::IdempotencyKeyReused)
    }
}

/// Replay after losing the race for the key inside the transaction: the winner
/// has committed by now, so its row is visible.
async fn replay(pool: &PgPool, request: &TransferRequest) -> Result<TransferOutcome, AppError> {
    match find_by_idempotency_key(pool, request).await? {
        Some(existing) => replay_of(existing, request),
        None => Err(AppError::Internal(
            "idempotency key conflict without a stored transfer".into(),
        )),
    }
}

/// Quadratic backoff (20ms, 80ms) plus up to 20ms of jitter so retries of
/// colliding transactions do not line up again.
fn backoff(attempt: u32) -> Duration {
    let jitter_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::from(d.subsec_nanos() % 20))
        .unwrap_or(0);
    Duration::from_millis(20 * u64::from(attempt * attempt) + jitter_ms)
}
