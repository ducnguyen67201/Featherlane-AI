//! Bounded, credential-safe acquisition adapters for policy sources.

use async_trait::async_trait;
use governance_domain::PolicyImportTransformationKind;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;

mod crypto;
mod google_drive;
mod microsoft_graph;
mod notion;
mod oauth;
mod safe_fetch;

pub use crypto::*;
pub use google_drive::*;
pub use microsoft_graph::*;
pub use notion::*;
pub use oauth::*;
pub use safe_fetch::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PreparedPolicyArtifact {
    pub kind: PolicyImportTransformationKind,
    pub processor: String,
    pub processor_version: String,
    pub mime_type: String,
    pub content: Vec<u8>,
    pub metadata: serde_json::Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AcquiredPolicyArtifact {
    pub external_revision: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub external_modified_at: Option<OffsetDateTime>,
    pub canonical_url: Option<String>,
    pub title: String,
    pub original_filename: Option<String>,
    pub declared_mime_type: Option<String>,
    pub raw_content: Vec<u8>,
    pub prepared: Option<PreparedPolicyArtifact>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectorRetry {
    Never,
    Retryable,
    Reauthorize,
}

#[derive(Debug, Error)]
#[error("source acquisition failed ({code})")]
pub struct ConnectorError {
    pub code: &'static str,
    pub retry: ConnectorRetry,
}

impl ConnectorError {
    #[must_use]
    pub const fn terminal(code: &'static str) -> Self {
        Self {
            code,
            retry: ConnectorRetry::Never,
        }
    }

    #[must_use]
    pub const fn retryable(code: &'static str) -> Self {
        Self {
            code,
            retry: ConnectorRetry::Retryable,
        }
    }

    #[must_use]
    pub const fn reauthorize() -> Self {
        Self {
            code: "reauthorization_required",
            retry: ConnectorRetry::Reauthorize,
        }
    }
}

#[async_trait]
pub trait PolicySourceConnector: Send + Sync {
    async fn acquire(
        &self,
        external_item_id: &str,
    ) -> Result<AcquiredPolicyArtifact, ConnectorError>;
}
