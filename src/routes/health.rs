//! Probes for orchestrators: liveness (`/health`, process is up) and readiness
//! (`/ready`, the database answers, so traffic can be routed here).

use crate::{
    db,
    error::{AppError, ErrorResponse},
};
use axum::{Json, extract::State};
use serde::Serialize;
use sqlx::PgPool;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct StatusResponse {
    #[schema(example = "ok")]
    status: &'static str,
}

/// Liveness: the process is running. Does not touch the database, so a
/// database outage does not get every instance restarted.
#[utoipa::path(get, path = "/health", tag = "health",
    responses((status = 200, description = "The service is running", body = StatusResponse)))]
pub async fn health() -> Json<StatusResponse> {
    Json(StatusResponse { status: "ok" })
}

/// Readiness: the database is reachable.
#[utoipa::path(get, path = "/ready", tag = "health",
    responses(
        (status = 200, description = "Ready to serve traffic", body = StatusResponse),
        (status = 503, description = "`not_ready`: the database cannot be reached", body = ErrorResponse),
    ))]
pub async fn ready(State(pool): State<PgPool>) -> Result<Json<StatusResponse>, AppError> {
    db::ping(&pool).await.map_err(AppError::NotReady)?;
    Ok(Json(StatusResponse { status: "ready" }))
}
