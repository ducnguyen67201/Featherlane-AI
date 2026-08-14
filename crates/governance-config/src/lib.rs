//! Typed environment configuration shared by first-party binaries.

use std::{collections::BTreeMap, env, fmt, net::SocketAddr, str::FromStr};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

#[derive(Clone)]
pub struct AppConfig {
    pub api_addr: SocketAddr,
    pub gateway_addr: SocketAddr,
    pub sandbox_addr: SocketAddr,
    pub database_url: String,
    pub web_origin: String,
    pub telemetry: TelemetryConfig,
    pub policy_import: PolicyImportConfig,
    pub source_connectors: SourceConnectorConfig,
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

#[derive(Clone)]
pub struct SourceConnectorConfig {
    pub console_api_key: SecretString,
    pub encryption_keys: BTreeMap<u32, SecretString>,
    pub active_key_version: Option<u32>,
    pub callback_base_url: Url,
    pub max_items_per_batch: usize,
    pub max_batch_bytes: usize,
    pub max_redirects: usize,
    pub oauth_state_ttl_seconds: usize,
    pub connect_timeout_seconds: usize,
    pub response_timeout_seconds: usize,
    pub google: Option<ProviderOAuthConfig>,
    pub microsoft: Option<ProviderOAuthConfig>,
    pub notion: Option<ProviderOAuthConfig>,
}

#[derive(Clone)]
pub struct ProviderOAuthConfig {
    pub client_id: String,
    pub client_secret: SecretString,
    pub callback_url: Url,
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
            .field("source_connectors", &self.source_connectors)
            .finish()
    }
}

impl fmt::Debug for SourceConnectorConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SourceConnectorConfig")
            .field("console_api_key", &"<redacted>")
            .field(
                "encryption_keys",
                &format_args!("<{} configured>", self.encryption_keys.len()),
            )
            .field("active_key_version", &self.active_key_version)
            .field("callback_base_url", &self.callback_base_url)
            .field("max_items_per_batch", &self.max_items_per_batch)
            .field("max_batch_bytes", &self.max_batch_bytes)
            .field("max_redirects", &self.max_redirects)
            .field("oauth_state_ttl_seconds", &self.oauth_state_ttl_seconds)
            .field("connect_timeout_seconds", &self.connect_timeout_seconds)
            .field("response_timeout_seconds", &self.response_timeout_seconds)
            .field("google", &self.google.as_ref().map(|_| "<configured>"))
            .field(
                "microsoft",
                &self.microsoft.as_ref().map(|_| "<configured>"),
            )
            .field("notion", &self.notion.as_ref().map(|_| "<configured>"))
            .finish()
    }
}

impl fmt::Debug for ProviderOAuthConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderOAuthConfig")
            .field("client_id", &"<configured>")
            .field("client_secret", &"<redacted>")
            .field("callback_url", &self.callback_url)
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
    #[error("invalid value in {key}: {value}")]
    InvalidValue { key: String, value: String },
    #[error("invalid connector configuration in {key}")]
    InvalidConnector { key: String },
}

impl AppConfig {
    /// Loads service addresses and infrastructure endpoints from the environment.
    ///
    /// # Errors
    ///
    /// Returns an error when a configured socket address is invalid.
    pub fn from_env() -> Result<Self, ConfigError> {
        let config = Self {
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
            source_connectors: SourceConnectorConfig::from_env()?,
        };
        Ok(config)
    }
}

impl SourceConnectorConfig {
    /// Loads bounded acquisition, credential-encryption, and OAuth settings.
    ///
    /// Providers are optional, but each provider's ID and secret must be configured together.
    ///
    /// # Errors
    ///
    /// Returns an error when a configured URL, limit, key, or provider credential is invalid.
    pub fn from_env() -> Result<Self, ConfigError> {
        let callback_base_url = Url::parse(&env_value(
            "SOURCE_CONNECTOR_CALLBACK_BASE_URL",
            "http://localhost:3000",
        ))
        .map_err(|_| ConfigError::InvalidConnector {
            key: "SOURCE_CONNECTOR_CALLBACK_BASE_URL".to_owned(),
        })?;
        let environment = env_value("FEATHERLANE_ENVIRONMENT", "development");
        if environment != "development" && callback_base_url.scheme() != "https" {
            return Err(ConfigError::InvalidConnector {
                key: "SOURCE_CONNECTOR_CALLBACK_BASE_URL".to_owned(),
            });
        }

        let active_key_version = optional_u32("SOURCE_CONNECTOR_ACTIVE_KEY_VERSION")?;
        let mut encryption_keys = BTreeMap::new();
        for (name, value) in env::vars() {
            let Some(version) = name
                .strip_prefix("SOURCE_CONNECTOR_ENCRYPTION_KEY_V")
                .and_then(|suffix| suffix.parse::<u32>().ok())
            else {
                continue;
            };
            let decoded = STANDARD
                .decode(value.as_bytes())
                .map_err(|_| ConfigError::InvalidConnector { key: name.clone() })?;
            if decoded.len() != 32 {
                return Err(ConfigError::InvalidConnector { key: name });
            }
            encryption_keys.insert(version, SecretString::from(value));
        }
        if active_key_version.is_some_and(|version| !encryption_keys.contains_key(&version)) {
            return Err(ConfigError::InvalidConnector {
                key: "SOURCE_CONNECTOR_ACTIVE_KEY_VERSION".to_owned(),
            });
        }

        let google = provider_config(
            "GOOGLE_DRIVE_CLIENT_ID",
            "GOOGLE_DRIVE_CLIENT_SECRET",
            callback_base_url.join("/api/source-connections/google_drive/callback"),
        )?;
        let microsoft = provider_config(
            "MICROSOFT_GRAPH_CLIENT_ID",
            "MICROSOFT_GRAPH_CLIENT_SECRET",
            callback_base_url.join("/api/source-connections/microsoft_graph/callback"),
        )?;
        let notion = provider_config(
            "NOTION_CLIENT_ID",
            "NOTION_CLIENT_SECRET",
            callback_base_url.join("/api/source-connections/notion/callback"),
        )?;

        Ok(Self {
            console_api_key: SecretString::from(env_value("GOVERNANCE_CONSOLE_API_KEY", "")),
            encryption_keys,
            active_key_version,
            callback_base_url,
            max_items_per_batch: positive_usize("SOURCE_INGESTION_MAX_ITEMS", 25)?,
            max_batch_bytes: positive_usize("SOURCE_INGESTION_MAX_BATCH_BYTES", 104_857_600)?,
            max_redirects: positive_usize("SOURCE_FETCH_MAX_REDIRECTS", 5)?,
            oauth_state_ttl_seconds: positive_usize("SOURCE_OAUTH_STATE_TTL_SECONDS", 600)?,
            connect_timeout_seconds: positive_usize("SOURCE_FETCH_CONNECT_TIMEOUT_SECONDS", 5)?,
            response_timeout_seconds: positive_usize("SOURCE_FETCH_RESPONSE_TIMEOUT_SECONDS", 30)?,
            google,
            microsoft,
            notion,
        })
    }

    #[must_use]
    pub fn encryption_configured(&self) -> bool {
        self.active_key_version.is_some() && !self.encryption_keys.is_empty()
    }
}

fn provider_config(
    id_key: &str,
    secret_key: &str,
    callback: Result<Url, url::ParseError>,
) -> Result<Option<ProviderOAuthConfig>, ConfigError> {
    let id = env::var(id_key).ok().filter(|value| !value.is_empty());
    let secret = env::var(secret_key).ok().filter(|value| !value.is_empty());
    match (id, secret) {
        (None, None) => Ok(None),
        (Some(client_id), Some(client_secret)) => Ok(Some(ProviderOAuthConfig {
            client_id,
            client_secret: SecretString::from(client_secret),
            callback_url: callback.map_err(|_| ConfigError::InvalidConnector {
                key: "SOURCE_CONNECTOR_CALLBACK_BASE_URL".to_owned(),
            })?,
        })),
        _ => Err(ConfigError::InvalidConnector {
            key: id_key.to_owned(),
        }),
    }
}

fn optional_u32(key: &str) -> Result<Option<u32>, ConfigError> {
    let Some(value) = env::var(key).ok().filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    value
        .parse::<u32>()
        .ok()
        .filter(|number| *number > 0)
        .map(Some)
        .ok_or(ConfigError::InvalidNumber {
            key: key.to_owned(),
            value,
        })
}

impl PolicyImportConfig {
    /// Loads bounded import, object-store, and model-provider settings.
    ///
    /// # Errors
    ///
    /// Returns an error when a configured numeric limit is invalid or zero.
    pub fn from_env() -> Result<Self, ConfigError> {
        let config = Self {
            max_bytes: positive_usize("POLICY_IMPORT_MAX_BYTES", 26_214_400)?,
            max_pages: positive_usize("POLICY_IMPORT_MAX_PAGES", 250)?,
            max_chunks: positive_usize("POLICY_IMPORT_MAX_CHUNKS", 200)?,
            max_candidates: positive_usize("POLICY_IMPORT_MAX_CANDIDATES", 500)?,
            object_store_url: env_value("OBJECT_STORE_URL", "http://localhost:9000"),
            object_store_bucket: env_value("OBJECT_STORE_BUCKET", "featherlane"),
            object_store_region: env_value("OBJECT_STORE_REGION", "us-east-1"),
            object_store_access_key_id: env_value("OBJECT_STORE_ACCESS_KEY_ID", ""),
            object_store_secret_access_key: env_value("OBJECT_STORE_SECRET_ACCESS_KEY", ""),
            llm_enabled: env_bool("POLICY_LLM_ENABLED", false)?,
            llm_provider: env_value("POLICY_LLM_PROVIDER", "openrouter"),
            llm_base_url: env_value("POLICY_LLM_BASE_URL", "https://openrouter.ai/api/v1"),
            llm_api_key: env_value("POLICY_LLM_API_KEY", ""),
            llm_model: env_value("POLICY_LLM_MODEL", ""),
            llm_prompt_version: env_value("POLICY_LLM_PROMPT_VERSION", "policy-extract-v1"),
            llm_require_zdr: env_bool("POLICY_LLM_REQUIRE_ZDR", true)?,
            llm_data_collection: env_value("POLICY_LLM_DATA_COLLECTION", "deny"),
            llm_allow_fallbacks: env_bool("POLICY_LLM_ALLOW_FALLBACKS", false)?,
        };
        if !matches!(config.llm_provider.as_str(), "openrouter" | "heuristic") {
            return Err(ConfigError::InvalidValue {
                key: "POLICY_LLM_PROVIDER".to_owned(),
                value: config.llm_provider,
            });
        }
        if !matches!(config.llm_data_collection.as_str(), "allow" | "deny") {
            return Err(ConfigError::InvalidValue {
                key: "POLICY_LLM_DATA_COLLECTION".to_owned(),
                value: config.llm_data_collection,
            });
        }
        Ok(config)
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

fn env_bool(key: &str, default: bool) -> Result<bool, ConfigError> {
    let Ok(value) = env::var(key) else {
        return Ok(default);
    };
    value.parse().map_err(|_| ConfigError::InvalidValue {
        key: key.to_owned(),
        value,
    })
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

    #[test]
    fn connector_debug_output_redacts_every_secret() {
        let mut encryption_keys = BTreeMap::new();
        encryption_keys.insert(1, SecretString::from("encryption-key-material".to_owned()));
        let config = SourceConnectorConfig {
            console_api_key: SecretString::from("console-secret".to_owned()),
            encryption_keys,
            active_key_version: Some(1),
            callback_base_url: Url::parse("https://console.example.test").expect("valid URL"),
            max_items_per_batch: 25,
            max_batch_bytes: 100,
            max_redirects: 5,
            oauth_state_ttl_seconds: 600,
            connect_timeout_seconds: 5,
            response_timeout_seconds: 30,
            google: Some(ProviderOAuthConfig {
                client_id: "visible-client-id".to_owned(),
                client_secret: SecretString::from("provider-secret".to_owned()),
                callback_url: Url::parse("https://console.example.test/callback")
                    .expect("valid URL"),
            }),
            microsoft: None,
            notion: None,
        };

        let rendered = format!("{config:?}");
        for secret in [
            "console-secret",
            "encryption-key-material",
            "visible-client-id",
            "provider-secret",
        ] {
            assert!(!rendered.contains(secret));
        }
    }
}
