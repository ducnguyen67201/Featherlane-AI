use governance_application::{ApplicationError, sha256_hex};
use governance_domain::{DocumentFormat, ParsedDocument, SourceSegment};

pub fn parse(content: &[u8], max_characters: usize) -> Result<ParsedDocument, ApplicationError> {
    let text = std::str::from_utf8(content)
        .map_err(|_| ApplicationError::InvalidRequest("plain text must be UTF-8".to_owned()))?;
    let normalized = text
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\0', "");
    if normalized.trim().is_empty() {
        return Err(ApplicationError::InvalidRequest(
            "empty_document: policy source contains no text".to_owned(),
        ));
    }
    if normalized.chars().count() > max_characters {
        return Err(ApplicationError::InvalidRequest(
            "normalized source exceeds the character limit".to_owned(),
        ));
    }
    let mut segments = Vec::new();
    let mut paragraph = 0_u32;
    let mut section: Option<String> = None;
    for block in normalized.split("\n\n") {
        let text = block.trim();
        if text.is_empty() {
            continue;
        }
        paragraph = paragraph.saturating_add(1);
        if text.lines().count() == 1 && text.len() <= 100 && text.ends_with(':') {
            section = Some(text.trim_end_matches(':').to_owned());
        }
        segments.push(SourceSegment {
            ordinal: u32::try_from(segments.len() + 1).unwrap_or(u32::MAX),
            page: None,
            section: section.clone(),
            paragraph_start: Some(paragraph),
            paragraph_end: Some(paragraph),
            text: text.to_owned(),
            text_sha256: sha256_hex(text.as_bytes()),
        });
    }
    Ok(ParsedDocument {
        format: DocumentFormat::PlainText,
        parser_version: "plain-text-v1".to_owned(),
        title: None,
        segments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_bom_and_preserves_paragraphs() {
        let parsed = parse(
            "\u{feff}Refunds:\r\n\r\nApproval is required.".as_bytes(),
            1_000,
        )
        .expect("text should parse");
        assert_eq!(parsed.segments.len(), 2);
        assert_eq!(parsed.segments[1].section.as_deref(), Some("Refunds"));
    }

    #[test]
    fn rejects_empty_text() {
        assert!(parse(b" \n ", 100).is_err());
    }
}
