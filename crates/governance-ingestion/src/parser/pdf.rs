use std::io::{self, Write};

use governance_application::{ApplicationError, sha256_hex};
use governance_domain::{DocumentFormat, ParsedDocument, SourceSegment};

pub fn parse(
    content: &[u8],
    max_pages: usize,
    max_characters: usize,
) -> Result<ParsedDocument, ApplicationError> {
    let document =
        pdf_extract::Document::load_mem(content).map_err(|error| map_document_error(&error))?;
    if document.is_encrypted() {
        return Err(ApplicationError::InvalidRequest(
            "encrypted_pdf: password-protected PDF".to_owned(),
        ));
    }
    let page_numbers = document.get_pages().into_keys().collect::<Vec<_>>();
    if page_numbers.len() > max_pages {
        return Err(ApplicationError::InvalidRequest(
            "PDF exceeds the configured page limit".to_owned(),
        ));
    }
    let mut pages = Vec::with_capacity(page_numbers.len());
    let mut remaining_characters = max_characters;
    for page_number in page_numbers {
        let mut writer = BoundedTextWriter::new(remaining_characters);
        let extraction = {
            let writer: &mut dyn Write = &mut writer;
            let mut output = pdf_extract::PlainTextOutput::new(writer);
            pdf_extract::output_doc_page(&document, &mut output, page_number)
        };
        if writer.exceeded {
            return Err(ApplicationError::InvalidRequest(
                "normalized source exceeds the character limit".to_owned(),
            ));
        }
        extraction.map_err(|error| map_output_error(&error))?;
        remaining_characters = remaining_characters.saturating_sub(writer.characters);
        pages.push(writer.text);
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
        parser_version: concat!("pdf-extract-", env!("PDF_EXTRACT_VERSION"), "-bounded").to_owned(),
        title: None,
        segments,
    })
}

fn map_document_error(error: &pdf_extract::Error) -> ApplicationError {
    if matches!(
        error,
        pdf_extract::Error::Decryption(_) | pdf_extract::Error::InvalidPassword
    ) {
        ApplicationError::InvalidRequest("encrypted_pdf: password-protected PDF".to_owned())
    } else {
        ApplicationError::InvalidRequest("invalid_pdf: PDF could not be parsed".to_owned())
    }
}

fn map_output_error(error: &pdf_extract::OutputError) -> ApplicationError {
    if matches!(
        error,
        pdf_extract::OutputError::PdfError(
            pdf_extract::Error::Decryption(_) | pdf_extract::Error::InvalidPassword
        )
    ) {
        ApplicationError::InvalidRequest("encrypted_pdf: password-protected PDF".to_owned())
    } else {
        ApplicationError::InvalidRequest("invalid_pdf: PDF could not be parsed".to_owned())
    }
}

#[derive(Debug)]
struct BoundedTextWriter {
    text: String,
    max_characters: usize,
    characters: usize,
    exceeded: bool,
}

impl BoundedTextWriter {
    fn new(max_characters: usize) -> Self {
        Self {
            text: String::new(),
            max_characters,
            characters: 0,
            exceeded: false,
        }
    }
}

impl Write for BoundedTextWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let text = std::str::from_utf8(buffer)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let characters = text.chars().count();
        if self.characters.saturating_add(characters) > self.max_characters {
            self.exceeded = true;
            return Err(io::Error::other("PDF text exceeds configured limit"));
        }
        self.text.push_str(text);
        self.characters += characters;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_corrupt_pdf() {
        assert!(parse(b"%PDF-not-valid", 10, 1_000).is_err());
    }

    #[test]
    fn bounded_writer_stops_before_exceeding_character_limit() {
        let mut writer = BoundedTextWriter::new(5);
        writer
            .write_all("éabc".as_bytes())
            .expect("four characters should fit");
        assert!(writer.write_all("de".as_bytes()).is_err());
        assert_eq!(writer.text, "éabc");
        assert_eq!(writer.characters, 4);
        assert!(writer.exceeded);
    }
}
