//! HTTP layer: routing, request/response shapes and extractors. Handlers stay
//! thin: validate input into domain types, call `db`, shape the response.

mod accounts;
mod extractors;
mod health;
pub mod openapi;
mod transactions;

use axum::{
    Router,
    http::{HeaderName, Request},
    routing::{get, post},
};
use sqlx::PgPool;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};

/// Correlates a request across services and log lines. A caller-supplied value
/// is kept; otherwise a UUID is generated. Echoed back on the response.
const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

pub fn router(pool: PgPool) -> Router {
    // Layers run outermost-last: the id is set first, then the trace span
    // (which records it) is opened, and finally the id is copied to the response.
    let trace = TraceLayer::new_for_http().make_span_with(|request: &Request<_>| {
        let request_id = request
            .headers()
            .get(&REQUEST_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        tracing::info_span!(
            "request",
            method = %request.method(),
            uri = %request.uri(),
            request_id,
        )
    });

    Router::new()
        .route("/health", get(health::health))
        .route("/ready", get(health::ready))
        .route("/accounts", post(accounts::create_account))
        .route("/accounts/{account_id}", get(accounts::get_account))
        .route("/transactions", post(transactions::create_transaction))
        .merge(openapi::routes())
        .layer(PropagateRequestIdLayer::new(REQUEST_ID_HEADER))
        .layer(trace)
        .layer(SetRequestIdLayer::new(REQUEST_ID_HEADER, MakeRequestUuid))
        .with_state(pool)
}
