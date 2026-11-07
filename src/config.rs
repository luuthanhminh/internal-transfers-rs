//! Configuration read from environment variables (12-factor style).
//!
//! Read once at startup into typed values; an invalid value stops the process
//! with a clear message instead of running with a guess.

use std::{env, fmt, net::SocketAddr, str::FromStr, time::Duration};

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    pub db_max_connections: u32,
    pub db_acquire_timeout: Duration,
    /// Apply pending migrations on startup. Turn off when migrations run as a
    /// separate release step (`internal-transfers migrate`).
    pub run_migrations: bool,
    pub log_format: LogFormat,
}

/// `text` for humans, `json` for log aggregators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
}

impl FromStr for LogFormat {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            _ => Err(()),
        }
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("invalid value for {name}: {value:?}")]
    Invalid { name: &'static str, value: String },
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| env::var(name).ok())
    }

    /// Builds the configuration from any variable source (the environment in
    /// production, a map in tests).
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let vars = Vars(lookup);
        let db_max_connections = vars.parse_or("DB_MAX_CONNECTIONS", 20)?;
        if db_max_connections == 0 {
            return Err(ConfigError::Invalid {
                name: "DB_MAX_CONNECTIONS",
                value: "0".to_owned(),
            });
        }
        Ok(Self {
            database_url: vars
                .get("DATABASE_URL")
                .ok_or(ConfigError::Missing("DATABASE_URL"))?,
            bind_addr: vars.parse_or("BIND_ADDR", SocketAddr::from(([0, 0, 0, 0], 8080)))?,
            db_max_connections,
            db_acquire_timeout: Duration::from_secs(vars.parse_or("DB_ACQUIRE_TIMEOUT_SECS", 5)?),
            run_migrations: vars.parse_or("RUN_MIGRATIONS", true)?,
            log_format: vars.parse_or("LOG_FORMAT", LogFormat::Text)?,
        })
    }
}

/// Never print the database password, even if someone logs `{config:?}`.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &redact_password(&self.database_url))
            .field("bind_addr", &self.bind_addr)
            .field("db_max_connections", &self.db_max_connections)
            .field("db_acquire_timeout", &self.db_acquire_timeout)
            .field("run_migrations", &self.run_migrations)
            .field("log_format", &self.log_format)
            .finish()
    }
}

/// `postgres://user:secret@host/db` → `postgres://user:***@host/db`.
fn redact_password(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "***".to_owned();
    };
    match rest.rsplit_once('@') {
        Some((credentials, host)) => {
            let user = credentials.split_once(':').map_or(credentials, |(u, _)| u);
            format!("{scheme}://{user}:***@{host}")
        }
        None => url.to_owned(),
    }
}

struct Vars<F>(F);

impl<F: Fn(&str) -> Option<String>> Vars<F> {
    /// Unset and empty both count as "not provided".
    fn get(&self, name: &str) -> Option<String> {
        (self.0)(name).filter(|value| !value.is_empty())
    }

    /// Parses an optional variable, falling back to `default` when it is unset.
    fn parse_or<T: FromStr>(&self, name: &'static str, default: T) -> Result<T, ConfigError> {
        match self.get(name) {
            Some(value) => value
                .parse()
                .map_err(|_| ConfigError::Invalid { name, value }),
            None => Ok(default),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const URL: &str = "postgres://transfers:s3cret@db:5432/transfers";

    fn config_from(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let vars: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Config::from_lookup(|name| vars.get(name).cloned())
    }

    #[test]
    fn only_the_database_url_is_required() {
        let config = config_from(&[("DATABASE_URL", URL)]).unwrap();
        assert_eq!(config.bind_addr.port(), 8080);
        assert_eq!(config.db_max_connections, 20);
        assert!(config.run_migrations);
        assert_eq!(config.log_format, LogFormat::Text);

        assert_eq!(
            config_from(&[]).unwrap_err(),
            ConfigError::Missing("DATABASE_URL")
        );
    }

    #[test]
    fn invalid_values_are_rejected_instead_of_guessed() {
        for (name, value) in [
            ("DB_MAX_CONNECTIONS", "0"),
            ("DB_MAX_CONNECTIONS", "many"),
            ("RUN_MIGRATIONS", "yes"),
            ("LOG_FORMAT", "xml"),
            ("BIND_ADDR", "8080"),
        ] {
            let result = config_from(&[("DATABASE_URL", URL), (name, value)]);
            assert!(
                matches!(result, Err(ConfigError::Invalid { name: n, .. }) if n == name),
                "{name}={value}"
            );
        }
    }

    #[test]
    fn debug_output_never_contains_the_database_password() {
        let config = config_from(&[("DATABASE_URL", URL)]).unwrap();
        let printed = format!("{config:?}");
        assert!(!printed.contains("s3cret"), "{printed}");
        assert!(printed.contains("postgres://transfers:***@db:5432/transfers"));
    }
}
