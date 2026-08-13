use std::io::{self, Cursor, Read};

use docx_rs::{DocumentChild, Paragraph, Table, TableCellContent, TableChild, TableRowChild};
use governance_application::{ApplicationError, sha256_hex};
use governance_domain::{DocumentFormat, ParsedDocument, SourceSegment};
use serde_json::Value;

pub fn parse(content: &[u8], max_characters: usize) -> Result<ParsedDocument, ApplicationError> {
    validate_package_limits(content, max_characters)?;
    let document = docx_rs::read_docx(content).map_err(|_| {
        ApplicationError::InvalidRequest("invalid_docx: DOCX could not be parsed".to_owned())
    })?;
    let mut paragraphs = Vec::new();
    for paragraph in document_paragraphs(&document.document.children) {
        let value = serde_json::to_value(paragraph).map_err(|_| {
            ApplicationError::InvalidRequest(
                "invalid_docx: DOCX text could not be decoded".to_owned(),
            )
        })?;
        let mut fragments = Vec::new();
        collect_text(&value, &mut fragments);
        let text = fragments.concat().trim().to_owned();
        if !text.is_empty() {
            paragraphs.push(text);
        }
    }
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
        parser_version: concat!("docx-rs-", env!("DOCX_RS_VERSION")).to_owned(),
        title: None,
        segments,
    })
}

enum DocumentBlock<'a> {
    Child(&'a DocumentChild),
    Table(&'a Table),
    Cell(&'a TableCellContent),
}

fn document_paragraphs(children: &[DocumentChild]) -> Vec<&Paragraph> {
    let mut paragraphs = Vec::new();
    let mut pending = children
        .iter()
        .rev()
        .map(DocumentBlock::Child)
        .collect::<Vec<_>>();

    while let Some(block) = pending.pop() {
        match block {
            DocumentBlock::Child(DocumentChild::Paragraph(paragraph))
            | DocumentBlock::Cell(TableCellContent::Paragraph(paragraph)) => {
                paragraphs.push(paragraph.as_ref());
            }
            DocumentBlock::Child(DocumentChild::Table(table))
            | DocumentBlock::Cell(TableCellContent::Table(table)) => {
                pending.push(DocumentBlock::Table(table));
            }
            DocumentBlock::Table(table) => {
                for TableChild::TableRow(row) in table.rows.iter().rev() {
                    for TableRowChild::TableCell(cell) in row.cells.iter().rev() {
                        pending.extend(cell.children.iter().rev().map(DocumentBlock::Cell));
                    }
                }
            }
            DocumentBlock::Cell(
                TableCellContent::StructuredDataTag(_) | TableCellContent::TableOfContents(_),
            )
            | DocumentBlock::Child(_) => {}
        }
    }

    paragraphs
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
    let mut declared_expanded = 0_u64;
    let mut actual_expanded = 0_u64;
    let max_expanded =
        u64::try_from(max_characters.min(50_000_000).saturating_mul(2)).unwrap_or(100_000_000);
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|_| {
            ApplicationError::InvalidRequest("invalid_docx: invalid ZIP entry".to_owned())
        })?;
        // ZIP metadata is uploader-controlled, so this is only a cheap pre-filter.
        declared_expanded = declared_expanded.saturating_add(file.size());
        let compression_ratio = file.size() / file.compressed_size().max(1);
        if declared_expanded > max_expanded || compression_ratio >= 100 {
            return Err(ApplicationError::InvalidRequest(
                "zip_limit_exceeded: expanded DOCX is too large".to_owned(),
            ));
        }
        // Decompress each entry through a bounded reader before docx-rs sees the
        // package. This enforces the real expansion limit rather than trusting
        // the declared uncompressed sizes above.
        let remaining = max_expanded.saturating_sub(actual_expanded);
        let expanded = io::copy(
            &mut file.by_ref().take(remaining.saturating_add(1)),
            &mut io::sink(),
        )
        .map_err(|_| {
            ApplicationError::InvalidRequest(
                "invalid_docx: DOCX entry could not be decompressed".to_owned(),
            )
        })?;
        actual_expanded = actual_expanded.saturating_add(expanded);
        if actual_expanded > max_expanded {
            return Err(ApplicationError::InvalidRequest(
                "zip_limit_exceeded: expanded DOCX is too large".to_owned(),
            ));
        }
    }
    Ok(())
}

fn collect_text(value: &Value, output: &mut Vec<String>) {
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::Object(object) => {
                if let Some(Value::String(text)) = object.get("text") {
                    output.push(text.to_owned());
                }
                for (key, child) in object.iter().rev() {
                    if key != "text" {
                        pending.push(child);
                    }
                }
            }
            Value::Array(items) => {
                pending.extend(items.iter().rev());
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use docx_rs::{Docx, Paragraph, Run, Table, TableCell, TableRow};

    use super::*;

    #[test]
    fn rejects_non_zip_docx() {
        assert!(parse(b"PK-not-a-zip", 1_000).is_err());
    }

    #[test]
    fn parses_generated_policy_docx() {
        let mut bytes = Cursor::new(Vec::new());
        Docx::new()
            .add_paragraph(
                Paragraph::new()
                    .add_run(Run::new().add_text("Refunds above USD 500 "))
                    .add_run(Run::new().add_text("must receive recorded human approval.")),
            )
            .build()
            .pack(&mut bytes)
            .expect("DOCX fixture should serialize");
        let document = parse(bytes.get_ref(), 10_000).expect("DOCX fixture should parse");
        assert_eq!(document.format, DocumentFormat::Docx);
        assert!(document.segments.iter().any(|segment| {
            segment
                .text
                .contains("Refunds above USD 500 must receive recorded human approval.")
        }));
    }

    #[test]
    fn preserves_paragraphs_inside_tables_in_document_order() {
        let mut bytes = Cursor::new(Vec::new());
        let table = Table::new(vec![TableRow::new(vec![
            TableCell::new()
                .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Approval threshold"))),
            TableCell::new()
                .add_paragraph(Paragraph::new().add_run(Run::new().add_text("USD 500"))),
        ])]);
        Docx::new()
            .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Refund policy")))
            .add_table(table)
            .add_paragraph(Paragraph::new().add_run(Run::new().add_text("End of policy")))
            .build()
            .pack(&mut bytes)
            .expect("DOCX fixture should serialize");

        let document = parse(bytes.get_ref(), 10_000).expect("DOCX fixture should parse");
        let text = document
            .segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            text,
            vec![
                "Refund policy",
                "Approval threshold",
                "USD 500",
                "End of policy"
            ]
        );
    }
}
