//! Typed environment configuration shared by first-party binaries.

use std::{env, fmt, net::SocketAddr, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone)]
pub struct AppConfig {
    pub api_addr: SocketAddr,
    pub gateway_addr: SocketAddr,
    pub sandbox_addr: SocketAddr,
    pub database_url: String,
    pub web_origin: String,
    pub telemetry: TelemetryConfig,
    pub policy_import: PolicyImportConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TelemetryConfig {
    pub default_settle_seconds: usize,
    pub default_idle_timeout_seconds: usize,
    pub max_run_duration_seconds: usize,
    pub max_compressed_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_spans_per_request: usize,
    pub max_spans_per_run: usize,
    pub max_attributes_per_span: usize,
    pub max_string_bytes: usize,
    pub job_poll_milliseconds: usize,
    pub job_lease_seconds: usize,
    pub job_max_attempts: usize,
    pub late_span_retention_days: usize,
}

#[derive(Clone)]
pub struct PolicyImportConfig {
    pub max_bytes: usize,
    pub max_pages: usize,
    pub max_chunks: usize,
    pub max_candidates: usize,
    pub object_store_url: String,
    pub object_store_bucket: String,
    pub object_store_region: String,
    pub object_store_access_key_id: String,
    pub object_store_secret_access_key: String,
    pub llm_enabled: bool,
    pub llm_provider: String,
    pub llm_base_url: String,
    pub llm_api_key: String,
    pub llm_model: String,
    pub llm_prompt_version: String,
    pub llm_require_zdr: bool,
    pub llm_data_collection: String,
    pub llm_allow_fallbacks: bool,
}

impl fmt::Debug for AppConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppConfig")
            .field("api_addr", &self.api_addr)
            .field("gateway_addr", &self.gateway_addr)
            .field("sandbox_addr", &self.sandbox_addr)
            .field("database_url", &"<redacted>")
            .field("web_origin", &self.web_origin)
            .field("telemetry", &self.telemetry)
            .field("policy_import", &self.policy_import)
            .finish()
    }
}

impl fmt::Debug for PolicyImportConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PolicyImportConfig")
            .field("max_bytes", &self.max_bytes)
            .field("max_pages", &self.max_pages)
            .field("max_chunks", &self.max_chunks)
            .field("max_candidates", &self.max_candidates)
            .field("object_store_url", &"<configured>")
            .field("object_store_bucket", &self.object_store_bucket)
            .field("object_store_region", &self.object_store_region)
            .field("object_store_access_key_id", &"<redacted>")
            .field("object_store_secret_access_key", &"<redacted>")
            .field("llm_enabled", &self.llm_enabled)
            .field("llm_provider", &self.llm_provider)
            .field("llm_base_url", &"<configured>")
            .field("llm_api_key", &"<redacted>")
            .field("llm_model", &self.llm_model)
            .field("llm_prompt_version", &self.llm_prompt_version)
            .field("llm_require_zdr", &self.llm_require_zdr)
            .field("llm_data_collection", &self.llm_data_collection)
            .field("llm_allow_fallbacks", &self.llm_allow_fallbacks)
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid socket address in {key}: {value}")]
    InvalidAddress { key: String, value: String },
    #[error("invalid positive integer in {key}: {value}")]
    InvalidNumber { key: String, value: String },
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
            telemetry: TelemetryConfig {
                default_settle_seconds: positive_usize("EVAL_DEFAULT_SETTLE_SECONDS", 10)?,
                default_idle_timeout_seconds: positive_usize(
                    "EVAL_DEFAULT_IDLE_TIMEOUT_SECONDS",
                    300,
                )?,
                max_run_duration_seconds: positive_usize("EVAL_MAX_RUN_DURATION_SECONDS", 86_400)?,
                max_compressed_bytes: positive_usize("OTLP_MAX_COMPRESSED_BYTES", 8_388_608)?,
                max_decoded_bytes: positive_usize("OTLP_MAX_DECODED_BYTES", 33_554_432)?,
                max_spans_per_request: positive_usize("OTLP_MAX_SPANS_PER_REQUEST", 10_000)?,
                max_spans_per_run: positive_usize("OTLP_MAX_SPANS_PER_RUN", 100_000)?,
                max_attributes_per_span: positive_usize("OTLP_MAX_ATTRIBUTES_PER_SPAN", 128)?,
                max_string_bytes: positive_usize("OTLP_MAX_STRING_BYTES", 16_384)?,
                job_poll_milliseconds: positive_usize("EVAL_JOB_POLL_MILLISECONDS", 1_000)?,
                job_lease_seconds: positive_usize("EVAL_JOB_LEASE_SECONDS", 120)?,
                job_max_attempts: positive_usize("EVAL_JOB_MAX_ATTEMPTS", 8)?,
                late_span_retention_days: positive_usize("OTLP_LATE_SPAN_RETENTION_DAYS", 7)?,
            },
            policy_import: PolicyImportConfig::from_env()?,
        })
    }
}

impl PolicyImportConfig {
    /// Loads bounded import, object-store, and model-provider settings.
    ///
    /// # Errors
    ///
    /// Returns an error when a configured numeric limit is invalid or zero.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            max_bytes: positive_usize("POLICY_IMPORT_MAX_BYTES", 26_214_400)?,
            max_pages: positive_usize("POLICY_IMPORT_MAX_PAGES", 250)?,
            max_chunks: positive_usize("POLICY_IMPORT_MAX_CHUNKS", 200)?,
            max_candidates: positive_usize("POLICY_IMPORT_MAX_CANDIDATES", 500)?,
            object_store_url: env_value("OBJECT_STORE_URL", "http://localhost:9000"),
            object_store_bucket: env_value("OBJECT_STORE_BUCKET", "featherlane"),
            object_store_region: env_value("OBJECT_STORE_REGION", "us-east-1"),
            object_store_access_key_id: env_value("OBJECT_STORE_ACCESS_KEY_ID", ""),
            object_store_secret_access_key: env_value("OBJECT_STORE_SECRET_ACCESS_KEY", ""),
            llm_enabled: env_bool("POLICY_LLM_ENABLED", false),
            llm_provider: env_value("POLICY_LLM_PROVIDER", "openrouter"),
            llm_base_url: env_value("POLICY_LLM_BASE_URL", "https://openrouter.ai/api/v1"),
            llm_api_key: env_value("POLICY_LLM_API_KEY", ""),
            llm_model: env_value("POLICY_LLM_MODEL", ""),
            llm_prompt_version: env_value("POLICY_LLM_PROMPT_VERSION", "policy-extract-v1"),
            llm_require_zdr: env_bool("POLICY_LLM_REQUIRE_ZDR", true),
            llm_data_collection: env_value("POLICY_LLM_DATA_COLLECTION", "deny"),
            llm_allow_fallbacks: env_bool("POLICY_LLM_ALLOW_FALLBACKS", false),
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

fn env_value(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_owned())
}

fn env_bool(key: &str, default: bool) -> bool {
    env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn positive_usize(key: &str, default: usize) -> Result<usize, ConfigError> {
    let value = env::var(key).unwrap_or_else(|_| default.to_string());
    value
        .parse::<usize>()
        .ok()
        .filter(|number| *number > 0)
        .ok_or_else(|| ConfigError::InvalidNumber {
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
