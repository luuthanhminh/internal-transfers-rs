//! The OpenAPI document must describe every endpoint the router serves, with
//! the error statuses clients have to handle, so the docs cannot silently drift.

use crate::helpers::TestApp;
use axum::http::StatusCode;
use sqlx::PgPool;

#[sqlx::test]
async fn the_api_document_lists_every_endpoint(pool: PgPool) {
    let app = TestApp::new(pool);

    let response = app.fetch_api_document().await;

    response.assert_status(StatusCode::OK);
    let paths = &response.body["paths"];
    for (path, method) in [
        ("/health", "get"),
        ("/ready", "get"),
        ("/accounts", "post"),
        ("/accounts/{account_id}", "get"),
        ("/transactions", "post"),
    ] {
        assert!(
            paths[path][method].is_object(),
            "{method} {path} is documented"
        );
    }
}

#[sqlx::test]
async fn the_api_document_describes_every_transfer_outcome(pool: PgPool) {
    let app = TestApp::new(pool);

    let document = app.fetch_api_document().await.body;

    let transfer = &document["paths"]["/transactions"]["post"];
    for status in ["201", "400", "404", "415", "422", "503"] {
        assert!(transfer["responses"][status].is_object(), "status {status}");
    }
    assert_eq!(transfer["parameters"][0]["name"], "Idempotency-Key");
    let amount =
        &document["components"]["schemas"]["CreateTransactionRequest"]["properties"]["amount"];
    assert_eq!(
        amount["type"], "string",
        "amounts are documented as strings"
    );
}

#[sqlx::test]
async fn the_api_reference_page_is_served(pool: PgPool) {
    let app = TestApp::new(pool);

    let (status, content_type, html) = app.open_api_reference_page().await;

    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/html"));
    assert!(
        html.contains("@scalar/api-reference@"),
        "script version is pinned"
    );
    assert!(html.contains("/transactions"), "the document is embedded");
}
