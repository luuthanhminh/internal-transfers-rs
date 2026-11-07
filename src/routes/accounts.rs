use super::extractors::{Json, Path};
use crate::{
    db,
    domain::{AccountId, Money, format_amount},
    error::AppError,
};
use axum::{extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;

use crate::error::ErrorResponse;

#[derive(Deserialize, ToSchema)]
pub struct CreateAccountRequest {
    /// Client-chosen positive id.
    #[schema(example = 123, minimum = 1)]
    account_id: i64,
    /// A decimal string (≥ 0, at most 20 integer digits and 18 decimals); JSON
    /// numbers are rejected to avoid float precision loss.
    #[schema(example = "100.23344", pattern = r"^(0|[1-9][0-9]*)(\.[0-9]+)?$")]
    initial_balance: String,
}

#[derive(Serialize, ToSchema)]
pub struct AccountResponse {
    #[schema(value_type = i64, example = 123)]
    account_id: AccountId,
    /// Decimal string without trailing zeros.
    #[schema(example = "100.23344")]
    balance: String,
}

/// Create an account with an opening balance.
#[utoipa::path(post, path = "/accounts", tag = "accounts",
    request_body = CreateAccountRequest,
    responses(
        (status = 201, description = "Account created (empty body)"),
        (status = 400, description = "Malformed JSON", body = ErrorResponse),
        (status = 409, description = "`account_already_exists`", body = ErrorResponse),
        (status = 415, description = "Missing `Content-Type: application/json`", body = ErrorResponse),
        (status = 422, description = "`invalid_amount`, `invalid_account_id` or a missing/mistyped field", body = ErrorResponse),
        (status = 503, description = "`temporarily_unavailable`; retry after `Retry-After` seconds", body = ErrorResponse),
    ))]
pub async fn create_account(
    State(pool): State<PgPool>,
    Json(request): Json<CreateAccountRequest>,
) -> Result<StatusCode, AppError> {
    let id = AccountId::new(request.account_id)?;
    let initial_balance = Money::parse(&request.initial_balance)?;
    db::accounts::insert(&pool, id, &initial_balance).await?;
    Ok(StatusCode::CREATED)
}

/// Get an account and its current balance.
#[utoipa::path(get, path = "/accounts/{account_id}", tag = "accounts",
    params(("account_id" = i64, Path, description = "Account id", example = 123)),
    responses(
        (status = 200, description = "The account", body = AccountResponse),
        (status = 400, description = "Non-numeric id", body = ErrorResponse),
        (status = 404, description = "`account_not_found`", body = ErrorResponse),
        (status = 422, description = "`invalid_account_id` (id ≤ 0)", body = ErrorResponse),
        (status = 503, description = "`temporarily_unavailable`", body = ErrorResponse),
    ))]
pub async fn get_account(
    State(pool): State<PgPool>,
    Path(account_id): Path<i64>,
) -> Result<axum::Json<AccountResponse>, AppError> {
    let id = AccountId::new(account_id)?;
    let balance = db::accounts::find_balance(&pool, id)
        .await?
        .ok_or(AppError::AccountNotFound(id))?;
    Ok(axum::Json(AccountResponse {
        account_id: id,
        balance: format_amount(&balance),
    }))
}
