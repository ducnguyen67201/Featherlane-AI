use std::fmt::Write as _;

use governance_domain::ParsedDocument;
use serde::{Deserialize, Serialize};

pub(crate) const EXTRACTION_CHUNK_CHARACTER_BUDGET: usize = 12_000;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicyChunk {
    pub id: String,
    pub segment_ordinals: Vec<u32>,
    pub content: String,
    pub completes_document: bool,
}

#[must_use]
pub fn chunk_document(
    document: &ParsedDocument,
    max_chars: usize,
    max_chunks: usize,
) -> Vec<PolicyChunk> {
    let mut chunks = Vec::new();
    let mut ordinals = Vec::new();
    let mut content = String::new();
    let mut fully_processed = true;

    for segment in &document.segments {
        if chunks.len() >= max_chunks {
            fully_processed = false;
            break;
        }
        let labeled = format!(
            "<source-segment ordinal=\"{}\">\n{}\n</source-segment>\n",
            segment.ordinal, segment.text
        );
        if labeled.chars().count() > max_chars {
            if !content.is_empty() {
                push_chunk(&mut chunks, &mut ordinals, &mut content);
            }
            let wrapper_characters = 64_usize;
            let piece_characters = max_chars.saturating_sub(wrapper_characters).max(1);
            let characters: Vec<char> = segment.text.chars().collect();
            for piece in characters.chunks(piece_characters) {
                if chunks.len() >= max_chunks {
                    fully_processed = false;
                    break;
                }
                ordinals.push(segment.ordinal);
                write!(
                    content,
                    "<source-segment ordinal=\"{}\">\n{}\n</source-segment>\n",
                    segment.ordinal,
                    piece.iter().collect::<String>()
                )
                .expect("writing to String should not fail");
                push_chunk(&mut chunks, &mut ordinals, &mut content);
            }
            continue;
        }
        if !content.is_empty()
            && content
                .chars()
                .count()
                .saturating_add(labeled.chars().count())
                > max_chars
        {
            push_chunk(&mut chunks, &mut ordinals, &mut content);
            if chunks.len() >= max_chunks {
                fully_processed = false;
                break;
            }
        }
        ordinals.push(segment.ordinal);
        content.push_str(&labeled);
    }
    if !content.is_empty() && chunks.len() < max_chunks {
        push_chunk(&mut chunks, &mut ordinals, &mut content);
    } else if !content.is_empty() {
        fully_processed = false;
    }
    if fully_processed
        && chunks
            .last()
            .and_then(|chunk| chunk.segment_ordinals.last())
            .copied()
            == document.segments.last().map(|segment| segment.ordinal)
        && let Some(last) = chunks.last_mut()
    {
        last.completes_document = true;
    }
    chunks
}

fn push_chunk(chunks: &mut Vec<PolicyChunk>, ordinals: &mut Vec<u32>, content: &mut String) {
    let index = chunks.len() + 1;
    chunks.push(PolicyChunk {
        id: format!("chunk-{index:04}"),
        segment_ordinals: std::mem::take(ordinals),
        content: std::mem::take(content),
        completes_document: false,
    });
}

#[cfg(test)]
mod tests {
    use governance_domain::{DocumentFormat, SourceSegment};

    use super::*;

    #[test]
    fn chunking_is_stable_and_bounded() {
        let document = ParsedDocument {
            format: DocumentFormat::PlainText,
            parser_version: "1".to_owned(),
            title: None,
            segments: vec![
                SourceSegment {
                    ordinal: 1,
                    page: None,
                    section: None,
                    paragraph_start: Some(1),
                    paragraph_end: Some(1),
                    text: "A".repeat(20),
                    text_sha256: "a".repeat(64),
                },
                SourceSegment {
                    ordinal: 2,
                    page: None,
                    section: None,
                    paragraph_start: Some(2),
                    paragraph_end: Some(2),
                    text: "B".repeat(20),
                    text_sha256: "b".repeat(64),
                },
            ],
        };
        let chunks = chunk_document(&document, 70, 10);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].id, "chunk-0001");
    }

    #[test]
    fn oversized_paragraphs_are_split_without_exceeding_chunk_limit() {
        let document = ParsedDocument {
            format: DocumentFormat::PlainText,
            parser_version: "1".to_owned(),
            title: None,
            segments: vec![SourceSegment {
                ordinal: 1,
                page: None,
                section: None,
                paragraph_start: Some(1),
                paragraph_end: Some(1),
                text: "policy ".repeat(1_000),
                text_sha256: "a".repeat(64),
            }],
        };
        let chunks = chunk_document(&document, 500, 100);
        assert!(chunks.len() > 1);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.content.chars().count() <= 500)
        );
        assert!(chunks.iter().all(|chunk| chunk.segment_ordinals == [1]));
    }
}
