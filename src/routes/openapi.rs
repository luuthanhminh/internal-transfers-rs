//! OpenAPI description of the HTTP API, generated from the handler annotations
//! and request/response types, plus an interactive reference page.
//!
//! - `GET /api-docs/openapi.json` — the OpenAPI 3.1 document
//! - `GET /docs` — Scalar API reference rendered from that document

use super::{accounts, transactions};
use crate::error::{ErrorBody, ErrorResponse};
use axum::{Json, Router, routing::get};
use utoipa::OpenApi;
use utoipa_scalar::{Scalar, Servable};

pub const SPEC_PATH: &str = "/api-docs/openapi.json";
pub const DOCS_PATH: &str = "/docs";

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Internal Transfers API",
        description = "Create accounts, query balances and transfer money between accounts. \
                       Amounts are decimal strings; every error has the shape \
                       `{\"error\": {\"code\", \"message\"}}`."
    ),
    paths(
        super::health::health,
        super::health::ready,
        accounts::create_account,
        accounts::get_account,
        transactions::create_transaction,
    ),
    components(schemas(ErrorResponse, ErrorBody)),
    tags(
        (name = "accounts", description = "Account creation and balance queries"),
        (name = "transactions", description = "Transfers between accounts"),
        (name = "health", description = "Liveness and readiness probes"),
    )
)]
pub struct ApiDoc;

/// Scalar's default template loads its script unpinned from a CDN; pin a
/// version so the page cannot change underneath us.
const SCALAR_HTML: &str = r#"<!doctype html>
<html>
<head>
  <title>$title</title>
  <meta charset="utf-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1"/>
</head>
<body>
  <script id="api-reference" type="application/json">$spec</script>
  <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference@1.74.0"></script>
</body>
</html>"#;

/// Routes serving the OpenAPI document and the reference page.
pub fn routes<S: Clone + Send + Sync + 'static>() -> Router<S> {
    let spec = ApiDoc::openapi();
    let docs_page = Scalar::with_url(DOCS_PATH, spec.clone())
        .custom_html(SCALAR_HTML)
        .title("Internal Transfers API");
    Router::new()
        .route(SPEC_PATH, get(move || async move { Json(spec) }))
        .merge(docs_page)
}
