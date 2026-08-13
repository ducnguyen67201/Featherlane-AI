mod docx;
mod pdf;
mod plain_text;

use async_trait::async_trait;
use governance_application::{ApplicationError, PolicyDocumentParser, detect_document_format};
use governance_config::PolicyImportConfig;
use governance_domain::{DocumentFormat, ParsedDocument};

#[derive(Clone, Debug)]
#[allow(clippy::struct_field_names)]
pub struct SafePolicyDocumentParser {
    max_bytes: usize,
    max_pages: usize,
    max_characters: usize,
}

impl SafePolicyDocumentParser {
    #[must_use]
    pub fn from_config(config: &PolicyImportConfig) -> Self {
        Self {
            max_bytes: config.max_bytes,
            max_pages: config.max_pages,
            max_characters: config.max_bytes.saturating_mul(4).min(100_000_000),
        }
    }

    #[must_use]
    pub fn with_limits(max_bytes: usize, max_pages: usize, max_characters: usize) -> Self {
        Self {
            max_bytes,
            max_pages,
            max_characters,
        }
    }
}

#[async_trait]
impl PolicyDocumentParser for SafePolicyDocumentParser {
    async fn parse(
        &self,
        detected_mime_type: &str,
        content: Vec<u8>,
    ) -> Result<ParsedDocument, ApplicationError> {
        if content.len() > self.max_bytes {
            return Err(ApplicationError::InvalidRequest(
                "source exceeds the configured byte limit".to_owned(),
            ));
        }
        let (format, detected) = detect_document_format(&content)?;
        if detected != detected_mime_type {
            return Err(ApplicationError::InvalidRequest(format!(
                "detected media type {detected} does not match persisted media type {detected_mime_type}"
            )));
        }
        let max_pages = self.max_pages;
        let max_characters = self.max_characters;
        tokio::task::spawn_blocking(move || match format {
            DocumentFormat::PlainText => plain_text::parse(&content, max_characters),
            DocumentFormat::Pdf => pdf::parse(&content, max_pages, max_characters),
            DocumentFormat::Docx => docx::parse(&content, max_characters),
        })
        .await
        .map_err(|error| ApplicationError::Repository(format!("parser task failed: {error}")))?
    }
}
