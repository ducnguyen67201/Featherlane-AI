use std::io::Cursor;

use governance_application::{ApplicationError, sha256_hex};
use governance_domain::{DocumentFormat, ParsedDocument, SourceSegment};
use serde_json::Value;

pub fn parse(content: &[u8], max_characters: usize) -> Result<ParsedDocument, ApplicationError> {
    validate_package_limits(content, max_characters)?;
    let document = docx_rs::read_docx(content).map_err(|_| {
        ApplicationError::InvalidRequest("invalid_docx: DOCX could not be parsed".to_owned())
    })?;
    let value: Value = serde_json::from_str(&document.json()).map_err(|_| {
        ApplicationError::InvalidRequest("invalid_docx: DOCX text could not be decoded".to_owned())
    })?;
    let mut paragraphs = Vec::new();
    collect_text(&value, &mut paragraphs);
    paragraphs.retain(|text| !text.trim().is_empty());
    let total: usize = paragraphs.iter().map(|text| text.chars().count()).sum();
    if total == 0 {
        return Err(ApplicationError::InvalidRequest(
            "empty_document: DOCX contains no supported text".to_owned(),
        ));
    }
    if total > max_characters {
        return Err(ApplicationError::InvalidRequest(
            "normalized source exceeds the character limit".to_owned(),
        ));
    }
    let segments = paragraphs
        .into_iter()
        .enumerate()
        .map(|(index, text)| SourceSegment {
            ordinal: u32::try_from(index + 1).unwrap_or(u32::MAX),
            page: None,
            section: None,
            paragraph_start: Some(u32::try_from(index + 1).unwrap_or(u32::MAX)),
            paragraph_end: Some(u32::try_from(index + 1).unwrap_or(u32::MAX)),
            text_sha256: sha256_hex(text.as_bytes()),
            text,
        })
        .collect();
    Ok(ParsedDocument {
        format: DocumentFormat::Docx,
        parser_version: "docx-rs-0.4".to_owned(),
        title: None,
        segments,
    })
}

fn validate_package_limits(content: &[u8], max_characters: usize) -> Result<(), ApplicationError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(content)).map_err(|_| {
        ApplicationError::InvalidRequest("invalid_docx: invalid ZIP package".to_owned())
    })?;
    if archive.len() > 1_000 {
        return Err(ApplicationError::InvalidRequest(
            "zip_limit_exceeded: too many DOCX entries".to_owned(),
        ));
    }
    let mut expanded = 0_u64;
    let max_expanded =
        u64::try_from(max_characters.min(50_000_000).saturating_mul(2)).unwrap_or(100_000_000);
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|_| {
            ApplicationError::InvalidRequest("invalid_docx: invalid ZIP entry".to_owned())
        })?;
        expanded = expanded.saturating_add(file.size());
        let compression_ratio = file.size() / file.compressed_size().max(1);
        if expanded > max_expanded || compression_ratio > 100 {
            return Err(ApplicationError::InvalidRequest(
                "zip_limit_exceeded: expanded DOCX is too large".to_owned(),
            ));
        }
    }
    Ok(())
}

fn collect_text(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            if let Some(Value::String(text)) = object.get("text") {
                output.push(text.trim().to_owned());
            }
            for (key, child) in object {
                if key != "text" {
                    collect_text(child, output);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_text(item, output);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use docx_rs::{Docx, Paragraph, Run};

    use super::*;

    #[test]
    fn rejects_non_zip_docx() {
        assert!(parse(b"PK-not-a-zip", 1_000).is_err());
    }

    #[test]
    fn parses_generated_policy_docx() {
        let mut bytes = Cursor::new(Vec::new());
        Docx::new()
            .add_paragraph(Paragraph::new().add_run(
                Run::new().add_text("Refunds above USD 500 must receive recorded human approval."),
            ))
            .build()
            .pack(&mut bytes)
            .expect("DOCX fixture should serialize");
        let document = parse(bytes.get_ref(), 10_000).expect("DOCX fixture should parse");
        assert_eq!(document.format, DocumentFormat::Docx);
        assert!(
            document
                .segments
                .iter()
                .any(|segment| segment.text.contains("human approval"))
        );
    }
}
