//! Typed environment configuration shared by first-party binaries.

use std::{env, net::SocketAddr, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub api_addr: SocketAddr,
    pub gateway_addr: SocketAddr,
    pub sandbox_addr: SocketAddr,
    pub database_url: String,
    pub web_origin: String,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid socket address in {key}: {value}")]
    InvalidAddress { key: String, value: String },
}

impl AppConfig {
    /// Loads service addresses and infrastructure endpoints from the environment.
    ///
    /// # Errors
    ///
    /// Returns an error when a configured socket address is invalid.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            api_addr: address("GOVERNANCE_API_ADDR", "0.0.0.0:8080")?,
            gateway_addr: address("GOVERNANCE_GATEWAY_ADDR", "0.0.0.0:4318")?,
            sandbox_addr: address("GOVERNANCE_SANDBOX_ADDR", "0.0.0.0:8090")?,
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgres://featherlane:featherlane@localhost:5432/featherlane".to_owned()
            }),
            web_origin: env::var("GOVERNANCE_WEB_ORIGIN")
                .unwrap_or_else(|_| "http://localhost:3000".to_owned()),
        })
    }
}

fn address(key: &str, default: &str) -> Result<SocketAddr, ConfigError> {
    let value = env::var(key).unwrap_or_else(|_| default.to_owned());
    SocketAddr::from_str(&value).map_err(|_| ConfigError::InvalidAddress {
        key: key.to_owned(),
        value,
    })
}

pub fn init_tracing(service_name: &str) {
    let _ = service_name;
    // Binaries initialize their subscriber so libraries stay logging-backend agnostic.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert!(address("FEATHERLANE_TEST_MISSING_ADDR", "127.0.0.1:8080").is_ok());
    }
}
