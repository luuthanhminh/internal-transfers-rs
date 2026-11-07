use super::extractors::Json;
use crate::{
    db::{
        self,
        transfers::{TransactionRecord, TransferOutcome, TransferRequest},
    },
    domain::{AccountId, Money, format_amount},
    error::{AppError, ErrorResponse},
};
use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;

const IDEMPOTENCY_KEY_HEADER: &str = "idempotency-key";
const REPLAYED_HEADER: &str = "idempotent-replayed";
const MAX_IDEMPOTENCY_KEY_LEN: usize = 255;

#[derive(Deserialize, ToSchema)]
pub struct CreateTransactionRequest {
    #[schema(example = 123, minimum = 1)]
    source_account_id: i64,
    #[schema(example = 456, minimum = 1)]
    destination_account_id: i64,
    /// A decimal string > 0 (at most 20 integer digits and 18 decimals); JSON
    /// numbers are rejected to avoid float precision loss.
    #[schema(example = "100.12345", pattern = r"^(0|[1-9][0-9]*)(\.[0-9]+)?$")]
    amount: String,
}

/// The recorded transfer.
#[derive(Serialize, ToSchema)]
pub struct TransactionResponse {
    #[schema(example = 1)]
    transaction_id: i64,
    #[schema(example = 123)]
    source_account_id: i64,
    #[schema(example = 456)]
    destination_account_id: i64,
    #[schema(example = "100.12345")]
    amount: String,
    created_at: DateTime<Utc>,
}

impl From<TransactionRecord> for TransactionResponse {
    fn from(record: TransactionRecord) -> Self {
        Self {
            transaction_id: record.transaction_id,
            source_account_id: record.source_account_id,
            destination_account_id: record.destination_account_id,
            amount: format_amount(&record.amount),
            created_at: record.created_at,
        }
    }
}

/// Transfer money between two accounts.
///
/// Both balances and the ledger entry are committed together or not at all.
/// An optional `Idempotency-Key` header makes retries safe: repeating a request
/// with the same key and body returns the original transfer (with
/// `Idempotent-Replayed: true`) instead of moving money again.
#[utoipa::path(post, path = "/transactions", tag = "transactions",
    request_body = CreateTransactionRequest,
    params(("Idempotency-Key" = Option<String>, Header,
        description = "Optional, 1–255 visible ASCII characters. Resending the same request with the same key returns the original transfer instead of moving money again.")),
    responses(
        (status = 201, description = "Transfer completed. A replay of an earlier request with the same `Idempotency-Key` also returns 201, with header `Idempotent-Replayed: true`", body = TransactionResponse,
            headers(("Idempotent-Replayed" = String, description = "`true` when this is a replay"))),
        (status = 400, description = "Malformed JSON or `invalid_idempotency_key`", body = ErrorResponse),
        (status = 404, description = "`account_not_found` (source or destination)", body = ErrorResponse),
        (status = 415, description = "Missing `Content-Type: application/json`", body = ErrorResponse),
        (status = 422, description = "`invalid_amount`, `invalid_account_id`, `same_account`, `insufficient_funds`, `balance_limit_exceeded` or `idempotency_key_reused`", body = ErrorResponse),
        (status = 503, description = "`temporarily_unavailable`: lock contention, timeout or no free connection; retry after `Retry-After` seconds", body = ErrorResponse,
            headers(("Retry-After" = u32, description = "Seconds to wait before retrying"))),
    ))]
pub async fn create_transaction(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(request): Json<CreateTransactionRequest>,
) -> Result<Response, AppError> {
    let source = AccountId::new(request.source_account_id)?;
    let destination = AccountId::new(request.destination_account_id)?;
    // Checked before touching the database: `account_id IN (a, a)` would return a
    // single row and be misreported as a missing account.
    if source == destination {
        return Err(AppError::SameAccount);
    }
    let transfer = TransferRequest {
        source,
        destination,
        amount: Money::parse_positive(&request.amount)?,
        idempotency_key: idempotency_key(&headers)?,
    };

    // Run the transfer on its own task: if the client disconnects, axum drops
    // the handler future, and dropping it mid-COMMIT would leave the outcome
    // unknown. The spawned task always runs to completion.
    let outcome = tokio::spawn(async move { db::transfers::transfer(&pool, &transfer).await })
        .await
        .map_err(|err| AppError::Internal(format!("transfer task failed: {err}")))??;

    let response = match outcome {
        TransferOutcome::Created(record) => (
            StatusCode::CREATED,
            axum::Json(TransactionResponse::from(record)),
        )
            .into_response(),
        TransferOutcome::Replayed(record) => (
            StatusCode::CREATED,
            [(REPLAYED_HEADER, HeaderValue::from_static("true"))],
            axum::Json(TransactionResponse::from(record)),
        )
            .into_response(),
    };
    Ok(response)
}

/// Reads the optional `Idempotency-Key` header: 1–255 visible ASCII characters.
fn idempotency_key(headers: &HeaderMap) -> Result<Option<String>, AppError> {
    let Some(value) = headers.get(IDEMPOTENCY_KEY_HEADER) else {
        return Ok(None);
    };
    let key = value
        .to_str()
        .map_err(|_| AppError::InvalidIdempotencyKey)?;
    let valid = !key.is_empty()
        && key.len() <= MAX_IDEMPOTENCY_KEY_LEN
        && key.bytes().all(|b| b.is_ascii_graphic());
    if valid {
        Ok(Some(key.to_owned()))
    } else {
        Err(AppError::InvalidIdempotencyKey)
    }
}
