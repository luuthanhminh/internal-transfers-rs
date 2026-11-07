#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

//! `internal-transfers`          apply migrations (unless RUN_MIGRATIONS=false), then serve
//! `internal-transfers migrate`  apply migrations and exit (one-off release step)

use internal_transfers::{
    config::{Config, LogFormat},
    startup,
};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;
    init_logging(config.log_format);

    match std::env::args().nth(1).as_deref() {
        None => startup::run(config).await,
        Some("migrate") => startup::migrate_only(&config).await,
        Some(other) => Err(format!("unknown command {other:?}; expected `migrate` or none").into()),
    }
}

/// Logs go to stdout (12-factor); `LOG_FORMAT=json` emits one JSON object per line.
fn init_logging(format: LogFormat) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    match format {
        LogFormat::Text => builder.init(),
        LogFormat::Json => builder.json().flatten_event(true).init(),
    }
}
