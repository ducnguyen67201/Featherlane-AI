use governance_application::{ApplicationError, sha256_hex};
use governance_domain::{DocumentFormat, ParsedDocument, SourceSegment};

pub fn parse(
    content: &[u8],
    max_pages: usize,
    max_characters: usize,
) -> Result<ParsedDocument, ApplicationError> {
    let pages = pdf_extract::extract_text_from_mem_by_pages(content).map_err(|error| {
        let detail = error.to_string();
        if detail.to_ascii_lowercase().contains("encrypt")
            || detail.to_ascii_lowercase().contains("password")
        {
            ApplicationError::InvalidRequest("encrypted_pdf: password-protected PDF".to_owned())
        } else {
            ApplicationError::InvalidRequest("invalid_pdf: PDF could not be parsed".to_owned())
        }
    })?;
    if pages.len() > max_pages {
        return Err(ApplicationError::InvalidRequest(
            "PDF exceeds the configured page limit".to_owned(),
        ));
    }
    let useful_characters: usize = pages
        .iter()
        .map(|page| {
            page.chars()
                .filter(|character| !character.is_whitespace())
                .count()
        })
        .sum();
    if useful_characters < 40 {
        return Err(ApplicationError::InvalidRequest(
            "needs_ocr: PDF has no usable embedded text".to_owned(),
        ));
    }
    let character_count: usize = pages.iter().map(|page| page.chars().count()).sum();
    if character_count > max_characters {
        return Err(ApplicationError::InvalidRequest(
            "normalized source exceeds the character limit".to_owned(),
        ));
    }
    let segments = pages
        .into_iter()
        .enumerate()
        .filter_map(|(index, page)| {
            let text = page.trim().to_owned();
            (!text.is_empty()).then(|| SourceSegment {
                ordinal: u32::try_from(index + 1).unwrap_or(u32::MAX),
                page: Some(u32::try_from(index + 1).unwrap_or(u32::MAX)),
                section: None,
                paragraph_start: None,
                paragraph_end: None,
                text_sha256: sha256_hex(text.as_bytes()),
                text,
            })
        })
        .collect();
    Ok(ParsedDocument {
        format: DocumentFormat::Pdf,
        parser_version: "pdf-extract-0.12".to_owned(),
        title: None,
        segments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_corrupt_pdf() {
        assert!(parse(b"%PDF-not-valid", 10, 1_000).is_err());
    }
}
