//! Operational endpoints and conventions: probes for the orchestrator and
//! request ids for following one request through the logs.

use crate::helpers::TestApp;
use axum::http::StatusCode;
use sqlx::PgPool;

#[sqlx::test]
async fn the_service_is_ready_when_the_database_answers(pool: PgPool) {
    let app = TestApp::new(pool);

    let response = app.check_readiness().await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.body["status"], "ready");
}

#[sqlx::test]
async fn the_service_is_alive_but_not_ready_when_the_database_is_unreachable(pool: PgPool) {
    let app = TestApp::with_unreachable_database(&pool).await;

    app.check_liveness().await.assert_status(StatusCode::OK);
    app.check_readiness()
        .await
        .assert_error(StatusCode::SERVICE_UNAVAILABLE, "not_ready");
}

#[sqlx::test]
async fn every_response_carries_a_request_id(pool: PgPool) {
    let app = TestApp::new(pool);

    let response = app.request_with_id(None).await;

    let generated = response.headers["x-request-id"]
        .to_str()
        .unwrap_or_default();
    assert_eq!(generated.len(), 36, "a UUID is generated: {generated:?}");
}

#[sqlx::test]
async fn a_request_id_sent_by_the_caller_is_echoed_back(pool: PgPool) {
    let app = TestApp::new(pool);

    let response = app.request_with_id(Some("trace-abc-123")).await;

    assert_eq!(response.headers["x-request-id"], "trace-abc-123");
}
