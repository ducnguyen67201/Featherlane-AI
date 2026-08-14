use std::collections::BTreeSet;

use governance_domain::{OrganizationId, PolicyPackId, RunBoundaryKind, TargetId};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;
use url::{Host, Url};

pub const TARGET_SCHEMA_VERSION: &str = "1.0";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverType {
    HttpText,
    Webhook,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceMode {
    #[default]
    Inline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetEnvironment {
    Staging,
    Preview,
    Sandbox,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TargetManifest {
    pub schema_version: String,
    pub target_id: String,
    pub target_version: String,
    pub driver_type: DriverType,
    pub endpoint: String,
    pub reset_endpoint: Option<String>,
    #[serde(default)]
    pub status_endpoint: Option<String>,
    #[serde(default)]
    pub terminal_response_key: Option<String>,
    pub auth_secret_ref: Option<String>,
    pub timeout_seconds: u64,
    #[serde(default)]
    pub evidence_mode: EvidenceMode,
    #[serde(default)]
    pub otlp_required: bool,
    pub production_credentials_allowed: bool,
    #[serde(default)]
    pub telemetry_boundary: TelemetryBoundaryConfig,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TelemetryBoundaryConfig {
    pub boundary_kind: RunBoundaryKind,
    #[serde(default)]
    pub external_id_attributes: Vec<String>,
    #[serde(default)]
    pub terminal_attribute: Option<String>,
    #[serde(default)]
    pub default_policy_pack_id: Option<PolicyPackId>,
    #[serde(default = "default_settle_seconds")]
    pub settle_seconds: u64,
    #[serde(default)]
    pub idle_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub max_duration_seconds: Option<u64>,
    #[serde(default)]
    pub conversation_id_is_task_boundary: bool,
}

const fn default_settle_seconds() -> u64 {
    10
}

impl Default for TelemetryBoundaryConfig {
    fn default() -> Self {
        Self {
            boundary_kind: RunBoundaryKind::ExplicitCi,
            external_id_attributes: vec!["featherlane.external_run.id".to_owned()],
            terminal_attribute: Some("featherlane.run.terminal".to_owned()),
            default_policy_pack_id: None,
            settle_seconds: default_settle_seconds(),
            idle_timeout_seconds: None,
            max_duration_seconds: None,
            conversation_id_is_task_boundary: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapabilityReport {
    pub target_id: String,
    pub reachable: bool,
    pub reset_supported: bool,
    pub trace_context_supported: bool,
    pub issues: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub checked_at: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RegisteredTarget {
    pub id: TargetId,
    pub organization_id: OrganizationId,
    pub name: String,
    pub environment: TargetEnvironment,
    pub manifest: TargetManifest,
    pub capability: CapabilityReport,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ManifestError {
    #[error("target manifest schema_version must be 1.0")]
    UnsupportedSchema,
    #[error("target name must contain between 1 and 80 characters")]
    InvalidName,
    #[error("target_id must be a lowercase slug of at most 63 characters")]
    InvalidTargetId,
    #[error("target_version must contain between 1 and 120 characters")]
    InvalidVersion,
    #[error("{field} must be an absolute HTTP or HTTPS URL without credentials")]
    InvalidUrl { field: &'static str },
    #[error("{field} points at a blocked metadata or link-local host")]
    BlockedHost { field: &'static str },
    #[error("timeout_seconds must be between 1 and 120")]
    InvalidTimeout,
    #[error("auth_secret_ref must be an uppercase environment-variable name")]
    InvalidSecretReference,
    #[error("production credentials must be disabled")]
    ProductionCredentials,
    #[error("an authenticated reset_endpoint must use the same origin as endpoint")]
    CrossOriginAuthenticatedReset,
    #[error("an authenticated status_endpoint must use the same origin as endpoint")]
    CrossOriginAuthenticatedStatus,
    #[error("terminal_response_key must contain between 1 and 120 characters")]
    InvalidTerminalResponseKey,
    #[error("automatic telemetry evaluation requires a non-CI session boundary")]
    InvalidTelemetryBoundaryKind,
    #[error("automatic telemetry evaluation requires between 1 and 8 unique attribute names")]
    InvalidExternalIdAttributes,
    #[error("telemetry attribute names must contain between 1 and 255 non-whitespace characters")]
    InvalidTelemetryAttribute,
    #[error("automatic telemetry evaluation requires a terminal boolean attribute")]
    MissingTerminalAttribute,
    #[error("telemetry settle_seconds must be at most 300")]
    InvalidSettleSeconds,
    #[error(
        "telemetry timeouts must be between 1 and 86400 seconds, with idle not exceeding max duration"
    )]
    InvalidTelemetryTimeout,
}

/// Validates the user-facing target name and its stored manifest.
///
/// # Errors
///
/// Returns a contract-specific validation error for an unsafe or malformed
/// registration.
pub fn validate_registration(name: &str, manifest: &TargetManifest) -> Result<(), ManifestError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 80 {
        return Err(ManifestError::InvalidName);
    }
    validate_manifest(manifest)
}

/// Validates a manifest without performing any network request.
///
/// # Errors
///
/// Returns a contract-specific validation error for unsupported schemas,
/// unsafe URLs, invalid bounds, secret references, or production credentials.
pub fn validate_manifest(manifest: &TargetManifest) -> Result<(), ManifestError> {
    if manifest.schema_version != TARGET_SCHEMA_VERSION {
        return Err(ManifestError::UnsupportedSchema);
    }
    if !valid_slug(&manifest.target_id) {
        return Err(ManifestError::InvalidTargetId);
    }
    let version = manifest.target_version.trim();
    if version.is_empty() || version.chars().count() > 120 {
        return Err(ManifestError::InvalidVersion);
    }
    let target_endpoint = validate_url("endpoint", &manifest.endpoint)?;
    if let Some(reset_endpoint) = &manifest.reset_endpoint {
        let reset_endpoint = validate_url("reset_endpoint", reset_endpoint)?;
        if manifest.auth_secret_ref.is_some() && reset_endpoint.origin() != target_endpoint.origin()
        {
            return Err(ManifestError::CrossOriginAuthenticatedReset);
        }
    }
    if let Some(status_endpoint) = &manifest.status_endpoint {
        let status_endpoint = validate_url("status_endpoint", status_endpoint)?;
        if manifest.auth_secret_ref.is_some()
            && status_endpoint.origin() != target_endpoint.origin()
        {
            return Err(ManifestError::CrossOriginAuthenticatedStatus);
        }
    }
    if manifest
        .terminal_response_key
        .as_ref()
        .is_some_and(|key| key.trim().is_empty() || key.len() > 120)
    {
        return Err(ManifestError::InvalidTerminalResponseKey);
    }
    if !(1..=120).contains(&manifest.timeout_seconds) {
        return Err(ManifestError::InvalidTimeout);
    }
    if let Some(reference) = &manifest.auth_secret_ref
        && !valid_secret_reference(reference)
    {
        return Err(ManifestError::InvalidSecretReference);
    }
    if manifest.production_credentials_allowed {
        return Err(ManifestError::ProductionCredentials);
    }
    validate_telemetry_boundary(&manifest.telemetry_boundary)?;
    Ok(())
}

/// Validates the passive telemetry boundary when automatic evaluation is enabled.
///
/// # Errors
///
/// Returns a contract-specific error for ambiguous boundaries, unsafe attribute
/// names, or durations outside the supported lifecycle bounds.
pub fn validate_telemetry_boundary(config: &TelemetryBoundaryConfig) -> Result<(), ManifestError> {
    if config.default_policy_pack_id.is_none() {
        return Ok(());
    }
    if config.boundary_kind == RunBoundaryKind::ExplicitCi {
        return Err(ManifestError::InvalidTelemetryBoundaryKind);
    }
    if config.external_id_attributes.is_empty() || config.external_id_attributes.len() > 8 {
        return Err(ManifestError::InvalidExternalIdAttributes);
    }
    let mut attributes = BTreeSet::new();
    for attribute in &config.external_id_attributes {
        if !valid_telemetry_attribute(attribute) {
            return Err(ManifestError::InvalidTelemetryAttribute);
        }
        if !attributes.insert(attribute.as_str()) {
            return Err(ManifestError::InvalidExternalIdAttributes);
        }
    }
    let terminal_attribute = config
        .terminal_attribute
        .as_deref()
        .ok_or(ManifestError::MissingTerminalAttribute)?;
    if !valid_telemetry_attribute(terminal_attribute) {
        return Err(ManifestError::InvalidTelemetryAttribute);
    }
    if config.settle_seconds > 300 {
        return Err(ManifestError::InvalidSettleSeconds);
    }
    let timeout_in_bounds = |value: u64| (1..=86_400).contains(&value);
    if config
        .idle_timeout_seconds
        .is_some_and(|value| !timeout_in_bounds(value))
        || config
            .max_duration_seconds
            .is_some_and(|value| !timeout_in_bounds(value))
        || matches!(
            (config.idle_timeout_seconds, config.max_duration_seconds),
            (Some(idle), Some(maximum)) if idle > maximum
        )
    {
        return Err(ManifestError::InvalidTelemetryTimeout);
    }
    Ok(())
}

fn valid_telemetry_attribute(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .chars()
            .all(|character| !character.is_whitespace() && !character.is_control())
}

fn valid_slug(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn valid_secret_reference(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes[0].is_ascii_uppercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn validate_url(field: &'static str, value: &str) -> Result<Url, ManifestError> {
    let parsed = Url::parse(value).map_err(|_| ManifestError::InvalidUrl { field })?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.host().is_none()
    {
        return Err(ManifestError::InvalidUrl { field });
    }
    let blocked = match parsed.host() {
        Some(Host::Ipv4(address)) => address.is_link_local() || address.is_broadcast(),
        Some(Host::Ipv6(address)) => address.is_unicast_link_local(),
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("metadata.google.internal"),
        None => false,
    };
    if blocked {
        return Err(ManifestError::BlockedHost { field });
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> TargetManifest {
        TargetManifest {
            schema_version: "1.0".to_owned(),
            target_id: "refund-agent".to_owned(),
            target_version: "git:test".to_owned(),
            driver_type: DriverType::HttpText,
            endpoint: "http://refund-agent:8091/v1/messages".to_owned(),
            reset_endpoint: None,
            status_endpoint: None,
            terminal_response_key: None,
            auth_secret_ref: None,
            timeout_seconds: 30,
            evidence_mode: EvidenceMode::Inline,
            otlp_required: false,
            production_credentials_allowed: false,
            telemetry_boundary: TelemetryBoundaryConfig::default(),
        }
    }

    #[test]
    fn safe_manifest_is_valid() {
        assert!(validate_registration("Refund Agent", &manifest()).is_ok());
    }

    #[test]
    fn production_credentials_are_rejected() {
        let mut value = manifest();
        value.production_credentials_allowed = true;
        assert_eq!(
            validate_manifest(&value),
            Err(ManifestError::ProductionCredentials)
        );
    }

    #[test]
    fn url_credentials_and_metadata_hosts_are_rejected() {
        let mut value = manifest();
        value.endpoint = "https://user:secret@example.com/run".to_owned();
        assert!(matches!(
            validate_manifest(&value),
            Err(ManifestError::InvalidUrl { .. })
        ));
        value.endpoint = "http://169.254.169.254/latest/meta-data".to_owned();
        assert!(matches!(
            validate_manifest(&value),
            Err(ManifestError::BlockedHost { .. })
        ));
    }

    #[test]
    fn bearer_secret_cannot_be_forwarded_to_another_reset_origin() {
        let mut value = manifest();
        value.endpoint = "https://agent.example.com/run".to_owned();
        value.reset_endpoint = Some("https://sandbox.example.com/reset".to_owned());
        value.auth_secret_ref = Some("TARGET_TOKEN".to_owned());

        assert_eq!(
            validate_manifest(&value),
            Err(ManifestError::CrossOriginAuthenticatedReset)
        );
    }

    fn automatic_boundary() -> TelemetryBoundaryConfig {
        TelemetryBoundaryConfig {
            boundary_kind: RunBoundaryKind::WorkflowExecution,
            default_policy_pack_id: Some(PolicyPackId::new()),
            idle_timeout_seconds: Some(300),
            max_duration_seconds: Some(3_600),
            ..TelemetryBoundaryConfig::default()
        }
    }

    #[test]
    fn canonical_automatic_boundary_is_valid() {
        assert_eq!(validate_telemetry_boundary(&automatic_boundary()), Ok(()));
    }

    #[test]
    fn disabled_automatic_boundary_keeps_backward_compatible_defaults() {
        let mut config = TelemetryBoundaryConfig::default();
        config.external_id_attributes.clear();
        config.terminal_attribute = None;
        assert_eq!(validate_telemetry_boundary(&config), Ok(()));
    }

    #[test]
    fn automatic_boundary_requires_session_and_terminal_attributes() {
        let mut config = automatic_boundary();
        config.external_id_attributes.clear();
        assert_eq!(
            validate_telemetry_boundary(&config),
            Err(ManifestError::InvalidExternalIdAttributes)
        );
        config.external_id_attributes = vec!["workflow.run.id".to_owned()];
        config.terminal_attribute = None;
        assert_eq!(
            validate_telemetry_boundary(&config),
            Err(ManifestError::MissingTerminalAttribute)
        );
    }

    #[test]
    fn automatic_boundary_rejects_duplicate_or_unsafe_attributes() {
        let mut config = automatic_boundary();
        config.external_id_attributes = vec!["workflow.run.id".to_owned(); 2];
        assert_eq!(
            validate_telemetry_boundary(&config),
            Err(ManifestError::InvalidExternalIdAttributes)
        );
        config.external_id_attributes = vec!["workflow run id".to_owned()];
        assert_eq!(
            validate_telemetry_boundary(&config),
            Err(ManifestError::InvalidTelemetryAttribute)
        );
    }

    #[test]
    fn automatic_boundary_rejects_ci_and_invalid_timing() {
        let mut config = automatic_boundary();
        config.boundary_kind = RunBoundaryKind::ExplicitCi;
        assert_eq!(
            validate_telemetry_boundary(&config),
            Err(ManifestError::InvalidTelemetryBoundaryKind)
        );
        config.boundary_kind = RunBoundaryKind::AgentTask;
        config.settle_seconds = 301;
        assert_eq!(
            validate_telemetry_boundary(&config),
            Err(ManifestError::InvalidSettleSeconds)
        );
        config.settle_seconds = 10;
        config.idle_timeout_seconds = Some(600);
        config.max_duration_seconds = Some(300);
        assert_eq!(
            validate_telemetry_boundary(&config),
            Err(ManifestError::InvalidTelemetryTimeout)
        );
    }
}
