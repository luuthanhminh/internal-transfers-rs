//! Wiring: connection pool, migrations, router, server and graceful shutdown.

use crate::{config::Config, routes};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::net::TcpListener;

/// Connects to PostgreSQL, applies pending migrations, then serves HTTP until a
/// shutdown signal arrives. In-flight requests are allowed to finish before the
/// pool is closed.
pub async fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!(?config, "starting");
    let pool = connect(&config).await?;

    if config.run_migrations {
        migrate(&pool).await?;
        tracing::info!("database migrations applied");
    }

    let listener = TcpListener::bind(config.bind_addr).await?;
    tracing::info!(addr = %listener.local_addr()?, "listening");

    axum::serve(listener, routes::router(pool.clone()))
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Only close the pool once every in-flight request has completed.
    pool.close().await;
    tracing::info!("shutdown complete");
    Ok(())
}

/// One-off release step: apply migrations and exit.
pub async fn migrate_only(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let pool = connect(config).await?;
    migrate(&pool).await?;
    pool.close().await;
    tracing::info!("database migrations applied");
    Ok(())
}

/// TLS follows the URL: add `?sslmode=require` (or `verify-full`) for managed
/// PostgreSQL; the default `prefer` uses TLS when the server offers it.
async fn connect(config: &Config) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .acquire_timeout(config.db_acquire_timeout)
        .connect(&config.database_url)
        .await
}

/// Applies the embedded migrations. Safe to call concurrently from several
/// instances: sqlx serialises migrators with a PostgreSQL advisory lock.
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    MIGRATOR.run(pool).await
}

/// The embedded migrations (`migrations/*.sql`), shared with the upgrade tests.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Resolves on Ctrl-C or SIGTERM (what `docker stop` and Kubernetes send).
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!(%err, "failed to listen for Ctrl-C");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(err) => {
                tracing::error!(%err, "failed to listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown signal received, draining in-flight requests");
}
