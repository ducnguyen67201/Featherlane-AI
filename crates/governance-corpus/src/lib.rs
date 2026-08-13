//! Pinned Open US Law manifest acquisition and provenance controls.

use governance_domain::SourceConfidence;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const OPEN_US_LAW_MANIFEST_URL: &str = "https://oss-data-us.vaquill.ai/index.json";
pub const PINNED_SNAPSHOT: &str = "v2026.07";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CorpusManifest {
    pub dataset: String,
    pub version: String,
    pub snapshot_date: String,
    pub total_bytes: u64,
    pub parquet_files: u32,
    pub license_data: String,
    pub files: Vec<CorpusFile>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CorpusFile {
    pub file: String,
    pub key: String,
    pub bytes: u64,
    pub sha256: String,
    pub url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedCorpusFile {
    pub jurisdiction: String,
    pub corpus_type: String,
    pub file: CorpusFile,
    pub confidence: SourceConfidence,
}

#[derive(Debug, Error)]
pub enum CorpusError {
    #[error("manifest request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("manifest is not the pinned snapshot: expected {expected}, got {actual}")]
    SnapshotMismatch { expected: String, actual: String },
    #[error("requested corpus file was not found: {0}")]
    FileNotFound(String),
    #[error("corpus file exceeded declared size")]
    SizeExceeded,
    #[error("corpus file size mismatch: expected {expected}, got {actual}")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("corpus checksum mismatch")]
    ChecksumMismatch,
    #[error("download is not a Parquet file")]
    InvalidParquet,
}

/// Fetches the public Open US Law manifest.
///
/// # Errors
///
/// Returns an error when the request fails or the response is not a valid manifest.
pub async fn fetch_manifest(client: &reqwest::Client) -> Result<CorpusManifest, CorpusError> {
    Ok(client
        .get(OPEN_US_LAW_MANIFEST_URL)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// Selects one exact corpus artifact from the pinned snapshot.
///
/// # Errors
///
/// Returns an error when the snapshot differs or the requested file is absent.
pub fn select_file(
    manifest: &CorpusManifest,
    snapshot: &str,
    jurisdiction: &str,
    corpus_type: &str,
) -> Result<SelectedCorpusFile, CorpusError> {
    if manifest.version != snapshot {
        return Err(CorpusError::SnapshotMismatch {
            expected: snapshot.to_owned(),
            actual: manifest.version.clone(),
        });
    }
    let normalized = jurisdiction.trim_start_matches("us_").to_ascii_lowercase();
    let filename = format!("us_{normalized}_{corpus_type}.parquet");
    let file = manifest
        .files
        .iter()
        .find(|file| file.file == filename)
        .cloned()
        .ok_or_else(|| CorpusError::FileNotFound(filename.clone()))?;
    let confidence = classify_jurisdiction(&normalized, corpus_type, true);
    Ok(SelectedCorpusFile {
        jurisdiction: normalized,
        corpus_type: corpus_type.to_owned(),
        file,
        confidence,
    })
}

/// Downloads an artifact and applies size, checksum, and Parquet checks.
///
/// # Errors
///
/// Returns an error for transport failures or any verification mismatch.
pub async fn download_verified(
    client: &reqwest::Client,
    file: &CorpusFile,
) -> Result<Vec<u8>, CorpusError> {
    let response = client.get(&file.url).send().await?.error_for_status()?;
    if response
        .content_length()
        .is_some_and(|content_length| content_length != file.bytes)
    {
        return Err(CorpusError::SizeMismatch {
            expected: file.bytes,
            actual: response.content_length().unwrap_or_default(),
        });
    }
    let bytes = response.bytes().await?;
    verify_bytes(file, &bytes)?;
    Ok(bytes.to_vec())
}

/// Verifies a fully acquired corpus artifact against its manifest entry.
///
/// # Errors
///
/// Returns an error when size, checksum, or file markers do not match.
pub fn verify_bytes(file: &CorpusFile, bytes: &[u8]) -> Result<(), CorpusError> {
    let actual_size = u64::try_from(bytes.len()).map_err(|_| CorpusError::SizeExceeded)?;
    if actual_size != file.bytes {
        return Err(CorpusError::SizeMismatch {
            expected: file.bytes,
            actual: actual_size,
        });
    }
    if format!("{:x}", Sha256::digest(bytes)) != file.sha256 {
        return Err(CorpusError::ChecksumMismatch);
    }
    if bytes.len() < 8 || &bytes[..4] != b"PAR1" || &bytes[bytes.len() - 4..] != b"PAR1" {
        return Err(CorpusError::InvalidParquet);
    }
    Ok(())
}

pub fn classify_jurisdiction(
    jurisdiction: &str,
    corpus_type: &str,
    has_government_source_url: bool,
) -> SourceConfidence {
    let normalized = jurisdiction.trim_start_matches("us_").to_ascii_lowercase();
    if corpus_type == "statutes" && matches!(normalized.as_str(), "ga" | "nc") {
        return SourceConfidence::Quarantined;
    }
    if !has_government_source_url
        || matches!(
            normalized.as_str(),
            "ar" | "co" | "ms" | "nm" | "nv" | "or" | "tn" | "wy"
        )
    {
        return SourceConfidence::SnapshotUnverifiedProvenance;
    }
    SourceConfidence::SnapshotOfficialProvenance
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> CorpusManifest {
        CorpusManifest {
            dataset: "open-us-law".to_owned(),
            version: PINNED_SNAPSHOT.to_owned(),
            snapshot_date: "2026-07-21".to_owned(),
            total_bytes: 8,
            parquet_files: 1,
            license_data: "CC-BY-4.0".to_owned(),
            files: vec![CorpusFile {
                file: "us_ga_statutes.parquet".to_owned(),
                key: "v2026.07/us_ga_statutes.parquet".to_owned(),
                bytes: 8,
                sha256: format!("{:x}", Sha256::digest(b"PAR1PAR1")),
                url: "https://example.test/file".to_owned(),
            }],
        }
    }

    #[test]
    fn known_noisy_jurisdictions_are_quarantined() {
        let selected = select_file(&manifest(), PINNED_SNAPSHOT, "ga", "statutes")
            .expect("fixture file should resolve");
        assert_eq!(selected.confidence, SourceConfidence::Quarantined);
        assert_eq!(
            classify_jurisdiction("nc", "statutes", true),
            SourceConfidence::Quarantined
        );
    }

    #[test]
    fn checksum_and_parquet_markers_are_required() {
        let file = &manifest().files[0];
        verify_bytes(file, b"PAR1PAR1").expect("fixture should verify");
        assert!(matches!(
            verify_bytes(file, b"NOTPARQT"),
            Err(CorpusError::ChecksumMismatch)
        ));
    }

    #[test]
    fn manifest_version_must_be_pinned() {
        assert!(matches!(
            select_file(&manifest(), "latest", "ga", "statutes"),
            Err(CorpusError::SnapshotMismatch { .. })
        ));
    }
}
