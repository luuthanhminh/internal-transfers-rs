//! Test plumbing. Test bodies only call these helpers, so they read as
//! "given / when / then" without HTTP, JSON or SQL noise.

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode, header},
};
use bigdecimal::BigDecimal;
use http_body_util::BodyExt;
use internal_transfers::{routes, startup::MIGRATOR};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction, pool::PoolConnection, postgres::PgPoolOptions};
use std::time::Duration;
use tower::ServiceExt;

/// The real router wired to a fresh, migrated database (one per test).
#[derive(Clone)]
pub struct TestApp {
    router: Router,
    pool: PgPool,
}

/// A response with its status, headers and parsed JSON body (`Null` when empty).
pub struct TestResponse {
    pub status: StatusCode,
    pub headers: axum::http::HeaderMap,
    pub body: Value,
}

impl TestApp {
    pub fn new(pool: PgPool) -> Self {
        Self {
            router: routes::router(pool.clone()),
            pool,
        }
    }

    /// Same database, but the service gets a deliberately tiny connection pool,
    /// like a production pool under a burst of traffic.
    pub fn with_connection_pool_of(pool: &PgPool, size: u32, acquire_timeout: Duration) -> Self {
        let small_pool = PgPoolOptions::new()
            .max_connections(size)
            .acquire_timeout(acquire_timeout)
            .connect_lazy_with((*pool.connect_options()).clone());
        Self::new(small_pool)
    }

    /// A service whose database cannot be reached (its pool is already closed).
    pub async fn with_unreachable_database(pool: &PgPool) -> Self {
        let app = Self::with_connection_pool_of(pool, 1, Duration::from_millis(300));
        app.pool.close().await;
        app
    }

    // ---- Given ----------------------------------------------------------

    pub async fn given_account(&self, id: i64, initial_balance: &str) {
        self.create_account(id, initial_balance)
            .await
            .assert_status(StatusCode::CREATED);
    }

    /// Another database client locks an account row and keeps the lock until the
    /// returned transaction is dropped.
    pub async fn given_another_client_holds_a_lock_on(
        &self,
        id: i64,
    ) -> Transaction<'static, Postgres> {
        let mut tx = self.pool.begin().await.expect("begin");
        sqlx::query("SELECT 1 FROM accounts WHERE account_id = $1 FOR UPDATE")
            .bind(id)
            .execute(&mut *tx)
            .await
            .expect("lock row");
        tx
    }

    /// Another client is in the middle of a transfer that has claimed `key` but
    /// not committed yet. Commit or roll back the returned transaction to finish it.
    pub async fn given_another_request_is_mid_transfer_with_key(
        &self,
        from: i64,
        to: i64,
        amount: &str,
        key: &str,
    ) -> Transaction<'static, Postgres> {
        let mut tx = self.pool.begin().await.expect("begin");
        sqlx::query(
            "INSERT INTO transactions (source_account_id, destination_account_id, amount, idempotency_key) \
             VALUES ($1, $2, $3::numeric, $4)",
        )
        .bind(from)
        .bind(to)
        .bind(amount)
        .bind(key)
        .execute(&mut *tx)
        .await
        .expect("claim key");
        tx
    }

    /// A real database error raised when `statement_timeout` cancels a query.
    pub async fn error_from_a_statement_cancelled_by_timeout(&self) -> sqlx::Error {
        let mut tx = self.pool.begin().await.expect("begin");
        sqlx::query("SET LOCAL statement_timeout = '10ms'")
            .execute(&mut *tx)
            .await
            .expect("set timeout");
        sqlx::query("SELECT pg_sleep(1)")
            .execute(&mut *tx)
            .await
            .expect_err("the statement must time out")
    }

    /// Occupies every connection of this app's pool until the guards are dropped.
    pub async fn given_all_database_connections_are_busy(&self) -> Vec<PoolConnection<Postgres>> {
        let size = self.pool.options().get_max_connections();
        let mut held = Vec::with_capacity(size as usize);
        for _ in 0..size {
            held.push(self.pool.acquire().await.expect("acquire"));
        }
        held
    }

    // ---- When: API calls -------------------------------------------------

    pub async fn create_account(&self, id: i64, initial_balance: &str) -> TestResponse {
        self.post_json(
            "/accounts",
            json!({ "account_id": id, "initial_balance": initial_balance }),
        )
        .await
    }

    pub async fn get_account(&self, id: &str) -> TestResponse {
        self.send(Request::get(format!("/accounts/{id}")).body(Body::empty()))
            .await
    }

    pub async fn transfer(&self, from: i64, to: i64, amount: &str) -> TestResponse {
        self.post_json("/transactions", transfer_body(from, to, amount))
            .await
    }

    pub async fn transfer_with_key(
        &self,
        from: i64,
        to: i64,
        amount: &str,
        key: &str,
    ) -> TestResponse {
        let request = Request::post("/transactions")
            .header(header::CONTENT_TYPE, "application/json")
            .header("Idempotency-Key", key)
            .body(Body::from(transfer_body(from, to, amount).to_string()));
        self.send(request).await
    }

    /// Sends a transfer from another client task and returns its pending answer.
    pub fn start_transfer(
        &self,
        from: i64,
        to: i64,
        amount: &str,
    ) -> tokio::task::JoinHandle<TestResponse> {
        let app = self.clone();
        let amount = amount.to_owned();
        tokio::spawn(async move { app.transfer(from, to, &amount).await })
    }

    /// Sends a transfer and drops the request before the answer arrives, which is
    /// what axum does to the handler when the client disconnects. The transfer
    /// must be blocked on a row lock; the request is dropped only once it is.
    pub async fn transfer_then_disconnect(&self, from: i64, to: i64, amount: &str) {
        let request = self.start_transfer(from, to, amount);
        self.wait_until_a_transfer_waits_for_a_row_lock().await;
        assert!(
            !request.is_finished(),
            "the transfer must still be in progress when the client disconnects"
        );
        request.abort();
        assert!(
            request.await.is_err_and(|err| err.is_cancelled()),
            "the request was dropped"
        );
    }

    pub async fn post_json(&self, path: &str, body: Value) -> TestResponse {
        self.post_raw(path, &body.to_string(), Some("application/json"))
            .await
    }

    pub async fn post_raw(
        &self,
        path: &str,
        body: &str,
        content_type: Option<&str>,
    ) -> TestResponse {
        let mut request = Request::post(path);
        if let Some(content_type) = content_type {
            request = request.header(header::CONTENT_TYPE, content_type);
        }
        self.send(request.body(Body::from(body.to_owned()))).await
    }

    pub async fn check_liveness(&self) -> TestResponse {
        self.send(Request::get("/health").body(Body::empty())).await
    }

    pub async fn check_readiness(&self) -> TestResponse {
        self.send(Request::get("/ready").body(Body::empty())).await
    }

    /// Any request, optionally carrying the caller's `X-Request-Id`.
    pub async fn request_with_id(&self, request_id: Option<&str>) -> TestResponse {
        let mut request = Request::get("/health");
        if let Some(id) = request_id {
            request = request.header("x-request-id", id);
        }
        self.send(request.body(Body::empty())).await
    }

    pub async fn fetch_api_document(&self) -> TestResponse {
        self.send(Request::get("/api-docs/openapi.json").body(Body::empty()))
            .await
    }

    /// The interactive API reference page: status, content type and HTML.
    pub async fn open_api_reference_page(&self) -> (StatusCode, String, String) {
        let (status, headers, bytes) = self
            .send_raw(Request::get("/docs").body(Body::empty()))
            .await;
        let content_type = headers[header::CONTENT_TYPE]
            .to_str()
            .expect("ascii content type")
            .to_owned();
        let html = String::from_utf8(bytes.to_vec()).expect("utf-8 page");
        (status, content_type, html)
    }

    async fn send_raw(
        &self,
        request: Result<Request<Body>, axum::http::Error>,
    ) -> (StatusCode, HeaderMap, axum::body::Bytes) {
        let request = request.expect("valid test request");
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("router is infallible");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("readable body")
            .to_bytes();
        (status, headers, bytes)
    }

    async fn send(&self, request: Result<Request<Body>, axum::http::Error>) -> TestResponse {
        let (status, headers, bytes) = self.send_raw(request).await;
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("every response body is JSON")
        };
        TestResponse {
            status,
            headers,
            body,
        }
    }

    // ---- Then: state read straight from the database ---------------------

    pub async fn balance_of(&self, id: i64) -> String {
        let response = self.get_account(&id.to_string()).await;
        response.assert_status(StatusCode::OK);
        response.body["balance"]
            .as_str()
            .expect("balance is a string")
            .to_owned()
    }

    pub async fn ledger_entry_count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM transactions")
            .fetch_one(&self.pool)
            .await
            .expect("count ledger entries")
    }

    /// Waits up to 5 s for work that finishes in the background to reach the ledger.
    pub async fn wait_until_ledger_has(&self, expected: i64) {
        for _ in 0..50 {
            if self.ledger_entry_count().await == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(self.ledger_entry_count().await, expected, "ledger entries");
    }

    pub async fn total_balance(&self) -> BigDecimal {
        sqlx::query_scalar("SELECT coalesce(sum(balance), 0) FROM accounts")
            .fetch_one(&self.pool)
            .await
            .expect("sum balances")
    }

    pub async fn assert_no_negative_balances(&self) {
        let negative: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts WHERE balance < 0")
            .fetch_one(&self.pool)
            .await
            .expect("count negative balances");
        assert_eq!(negative, 0, "no account balance may be negative");
    }

    /// For every account: balance = initial_balance + incoming - outgoing.
    pub async fn assert_all_accounts_reconcile(&self) {
        let mismatched: Vec<i64> = sqlx::query_scalar(
            "SELECT a.account_id FROM accounts a
             WHERE a.balance <> a.initial_balance
                 + coalesce((SELECT sum(amount) FROM transactions WHERE destination_account_id = a.account_id), 0)
                 - coalesce((SELECT sum(amount) FROM transactions WHERE source_account_id = a.account_id), 0)",
        )
        .fetch_all(&self.pool)
        .await
        .expect("reconcile accounts");
        assert!(
            mismatched.is_empty(),
            "accounts not matching their history: {mismatched:?}"
        );
    }

    /// Runs raw SQL as a database client would, bypassing the application.
    /// Waits until a transaction in this database is queued on a row lock.
    pub async fn wait_until_a_transfer_waits_for_a_row_lock(&self) {
        self.first_lock_waiter_started_after(None).await;
    }

    /// Waits until the transfer queued on a row lock gave up (lock_timeout) and
    /// a NEW transaction, its retry, is queued on the lock again. Polling the
    /// database instead of sleeping keeps this deterministic on a slow machine.
    pub async fn wait_until_the_blocked_transfer_is_retried(&self) {
        let first_attempt = self.first_lock_waiter_started_after(None).await;
        self.first_lock_waiter_started_after(Some(first_attempt))
            .await;
    }

    /// Start time of a transaction waiting on a lock, newer than `after`.
    async fn first_lock_waiter_started_after(
        &self,
        after: Option<chrono::DateTime<chrono::Utc>>,
    ) -> chrono::DateTime<chrono::Utc> {
        for _ in 0..1000 {
            let started: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
                "SELECT max(xact_start) FROM pg_stat_activity
                  WHERE datname = current_database() AND wait_event_type = 'Lock'",
            )
            .fetch_one(&self.pool)
            .await
            .expect("read pg_stat_activity");
            if let Some(started) = started
                && after.is_none_or(|after| started > after)
            {
                return started;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no transaction started waiting for a row lock within 10 s");
    }

    pub async fn execute_sql_directly(&self, sql: &'static str) -> Result<(), sqlx::Error> {
        sqlx::raw_sql(sql).execute(&self.pool).await.map(|_| ())
    }
}

impl TestResponse {
    #[track_caller]
    pub fn assert_status(&self, expected: StatusCode) {
        assert_eq!(
            self.status, expected,
            "unexpected status, body: {}",
            self.body
        );
    }

    /// Asserts a JSON error response with the given status and error code.
    #[track_caller]
    pub fn assert_error(&self, expected_status: StatusCode, expected_code: &str) {
        self.assert_status(expected_status);
        assert_eq!(
            self.body["error"]["code"], expected_code,
            "body: {}",
            self.body
        );
        assert!(
            self.body["error"]["message"].is_string(),
            "body: {}",
            self.body
        );
    }

    pub fn transaction_id(&self) -> i64 {
        self.body["transaction_id"]
            .as_i64()
            .expect("transaction_id in body")
    }
}

fn transfer_body(from: i64, to: i64, amount: &str) -> Value {
    json!({ "source_account_id": from, "destination_account_id": to, "amount": amount })
}

/// Parses a decimal literal for comparisons in assertions.
pub fn decimal(value: &str) -> BigDecimal {
    value.parse().expect("valid decimal literal")
}

// ---- Schema upgrades (databases created with `#[sqlx::test(migrations = false)]`)

/// A database whose schema stops at `version`, like one deployed by an older release.
pub async fn given_a_database_migrated_up_to(pool: &PgPool, version: i64) {
    MIGRATOR
        .run_to(version, pool)
        .await
        .expect("apply migrations up to the given version");
}

/// Writes an account row with SQL, bypassing the API (data an older release allowed).
pub async fn given_an_account_stored_directly(pool: &PgPool, id: i64, balance: &str) {
    sqlx::query(
        "INSERT INTO accounts (account_id, initial_balance, balance)
         VALUES ($1, $2::numeric, $2::numeric)",
    )
    .bind(id)
    .bind(balance)
    .execute(pool)
    .await
    .expect("insert account");
}

/// Upgrades the database to the current schema, as the next release would.
pub async fn upgrade_to_the_current_schema(
    pool: &PgPool,
) -> Result<(), sqlx::migrate::MigrateError> {
    MIGRATOR.run(pool).await
}
