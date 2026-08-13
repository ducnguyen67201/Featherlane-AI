use governance_domain::{OrganizationId, TargetId};
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
    pub auth_secret_ref: Option<String>,
    pub timeout_seconds: u64,
    #[serde(default)]
    pub evidence_mode: EvidenceMode,
    pub production_credentials_allowed: bool,
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
    #[error("only inline evidence is supported for active CI targets")]
    UnsupportedEvidenceMode,
    #[error("production credentials must be disabled")]
    ProductionCredentials,
    #[error("an authenticated reset_endpoint must use the same origin as endpoint")]
    CrossOriginAuthenticatedReset,
}

/// Validates the user-facing target name and its stored manifest.
///
/// # Errors
///
/// Returns a contract-specific validation error for an unsafe or malformed
/// registration.
pub fn validate_registration(
    name: &str,
    _environment: TargetEnvironment,
    manifest: &TargetManifest,
) -> Result<(), ManifestError> {
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
    if !(1..=120).contains(&manifest.timeout_seconds) {
        return Err(ManifestError::InvalidTimeout);
    }
    if let Some(reference) = &manifest.auth_secret_ref
        && !valid_secret_reference(reference)
    {
        return Err(ManifestError::InvalidSecretReference);
    }
    if manifest.evidence_mode != EvidenceMode::Inline {
        return Err(ManifestError::UnsupportedEvidenceMode);
    }
    if manifest.production_credentials_allowed {
        return Err(ManifestError::ProductionCredentials);
    }
    Ok(())
}

fn valid_slug(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase_or_digit()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase_or_digit() || *byte == b'-')
}

trait AsciiSlugByte {
    fn is_ascii_lowercase_or_digit(&self) -> bool;
}

impl AsciiSlugByte for u8 {
    fn is_ascii_lowercase_or_digit(&self) -> bool {
        self.is_ascii_lowercase() || self.is_ascii_digit()
    }
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
            auth_secret_ref: None,
            timeout_seconds: 30,
            evidence_mode: EvidenceMode::Inline,
            production_credentials_allowed: false,
        }
    }

    #[test]
    fn safe_manifest_is_valid() {
        assert!(
            validate_registration("Refund Agent", TargetEnvironment::Staging, &manifest()).is_ok()
        );
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
}
