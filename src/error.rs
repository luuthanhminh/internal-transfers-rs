//! Application error type and its mapping to consistent JSON HTTP responses:
//! `{"error": {"code": "...", "message": "..."}}`.

use crate::{
    db,
    domain::{AccountId, InvalidAccountId, MoneyError},
};
use axum::{
    Json,
    extract::rejection::{JsonRejection, PathRejection},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

/// Body of every error response.
#[derive(Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    /// Stable, machine-readable code such as `insufficient_funds`.
    #[schema(example = "insufficient_funds")]
    pub code: &'static str,
    /// Human-readable explanation; never contains internal details.
    #[schema(example = "account 123 has insufficient funds")]
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// The request could not be parsed (malformed JSON, wrong types, bad path).
    #[error("{message}")]
    BadRequest { status: StatusCode, message: String },
    #[error(transparent)]
    InvalidAmount(#[from] MoneyError),
    #[error(transparent)]
    InvalidAccountId(#[from] InvalidAccountId),
    #[error("Idempotency-Key must be 1 to 255 visible ASCII characters")]
    InvalidIdempotencyKey,
    #[error("source and destination accounts must be different")]
    SameAccount,
    #[error("account {0} does not exist")]
    AccountNotFound(AccountId),
    #[error("account {0} already exists")]
    AccountAlreadyExists(AccountId),
    #[error("account {0} has insufficient funds")]
    InsufficientFunds(AccountId),
    #[error("account {0} would exceed the maximum balance")]
    BalanceLimitExceeded(AccountId),
    #[error("Idempotency-Key was already used for a different transfer")]
    IdempotencyKeyReused,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    /// The readiness probe could not reach the database.
    #[error("not ready: {0}")]
    NotReady(sqlx::Error),
    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            Self::BadRequest { status, .. } => (*status, "invalid_request"),
            Self::InvalidIdempotencyKey => (StatusCode::BAD_REQUEST, "invalid_idempotency_key"),
            Self::InvalidAmount(_) => (StatusCode::UNPROCESSABLE_ENTITY, "invalid_amount"),
            Self::InvalidAccountId(_) => (StatusCode::UNPROCESSABLE_ENTITY, "invalid_account_id"),
            Self::SameAccount => (StatusCode::UNPROCESSABLE_ENTITY, "same_account"),
            Self::InsufficientFunds(_) => (StatusCode::UNPROCESSABLE_ENTITY, "insufficient_funds"),
            Self::BalanceLimitExceeded(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, "balance_limit_exceeded")
            }
            Self::IdempotencyKeyReused => {
                (StatusCode::UNPROCESSABLE_ENTITY, "idempotency_key_reused")
            }
            Self::AccountNotFound(_) => (StatusCode::NOT_FOUND, "account_not_found"),
            Self::AccountAlreadyExists(_) => (StatusCode::CONFLICT, "account_already_exists"),
            Self::NotReady(_) => (StatusCode::SERVICE_UNAVAILABLE, "not_ready"),
            Self::Database(err) if db::is_temporarily_unavailable(err) => {
                (StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable")
            }
            Self::Database(_) | Self::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
            }
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();

        // Never leak database or internal details to the client; log them instead.
        let message = if status.is_server_error() {
            tracing::error!(error = %self, "request failed");
            match status {
                StatusCode::SERVICE_UNAVAILABLE => "the service is busy, please retry".to_owned(),
                _ => "internal server error".to_owned(),
            }
        } else {
            self.to_string()
        };

        let mut response = (
            status,
            Json(ErrorResponse {
                error: ErrorBody { code, message },
            }),
        )
            .into_response();
        if status == StatusCode::SERVICE_UNAVAILABLE {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        }
        response
    }
}

// Extractor rejections become JSON errors too, instead of axum's plain-text default.
impl From<JsonRejection> for AppError {
    fn from(rejection: JsonRejection) -> Self {
        Self::BadRequest {
            status: rejection.status(),
            message: rejection.body_text(),
        }
    }
}

impl From<PathRejection> for AppError {
    fn from(rejection: PathRejection) -> Self {
        Self::BadRequest {
            status: StatusCode::BAD_REQUEST,
            message: rejection.body_text(),
        }
    }
}
