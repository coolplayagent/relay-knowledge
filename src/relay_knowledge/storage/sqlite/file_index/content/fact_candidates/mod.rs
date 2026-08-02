//! Bounded line-span fact candidates projected from local-file content chunks.

use crate::{domain::EvidenceSpan, storage::FileKnowledgeFactCandidate};

use super::identity::stable_hash64;

pub(super) fn for_chunk(
    source_scope: &str,
    source_path: &str,
    chunk_id: &str,
    content: &str,
    span: EvidenceSpan,
    fingerprint: &str,
    freshness_cursor: &str,
) -> Vec<FileKnowledgeFactCandidate> {
    content_line_spans(content, span)
        .into_iter()
        .filter_map(|(line, line_span)| {
            candidate_for_line(source_scope, source_path, chunk_id, line)
                .map(|candidate| (candidate, line_span))
        })
        .take(8)
        .map(
            |((kind, subject, predicate, object), line_span)| FileKnowledgeFactCandidate {
                candidate_id: format!(
                    "file-fact:{:016x}",
                    stable_hash64(
                        format!("{chunk_id}:{subject}:{predicate}:{object:?}").as_bytes()
                    )
                ),
                kind,
                subject,
                predicate,
                object,
                confidence_basis_points: 6500,
                status: "candidate".to_owned(),
                source_scope: source_scope.to_owned(),
                source_path: source_path.to_owned(),
                span: line_span,
                fingerprint: fingerprint.to_owned(),
                freshness_cursor: freshness_cursor.to_owned(),
            },
        )
        .collect()
}

fn content_line_spans(content: &str, chunk_span: EvidenceSpan) -> Vec<(&str, EvidenceSpan)> {
    let mut lines = Vec::new();
    let mut byte_offset = 0usize;
    let mut line_number = chunk_span.start_line;

    for segment in content.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        let start_byte = chunk_span
            .start_byte
            .saturating_add(u32::try_from(byte_offset).unwrap_or(u32::MAX));
        let end_byte = start_byte.saturating_add(u32::try_from(line.len()).unwrap_or(u32::MAX));
        lines.push((
            line,
            EvidenceSpan {
                start_byte,
                end_byte,
                start_line: line_number,
                end_line: line_number,
            },
        ));
        byte_offset = byte_offset.saturating_add(segment.len());
        if segment.ends_with('\n') {
            line_number = line_number.saturating_add(1);
        }
    }

    if !content.is_empty() && !content.ends_with('\n') {
        return lines;
    }
    if content.is_empty() {
        lines.push(("", chunk_span));
    }

    lines
}

fn candidate_for_line(
    source_scope: &str,
    source_path: &str,
    chunk_id: &str,
    line: &str,
) -> Option<(String, String, String, Option<String>)> {
    let line = line.trim().trim_matches('-').trim();
    if let Some(heading) = line.strip_prefix('#') {
        let heading = heading.trim_matches('#').trim();
        if !heading.is_empty() {
            return Some((
                "claim".to_owned(),
                source_path.to_owned(),
                "has_heading".to_owned(),
                Some(heading.to_owned()),
            ));
        }
    }
    for delimiter in [":", "="] {
        if let Some((key, value)) = line.split_once(delimiter) {
            let key = key.trim();
            let value = value.trim();
            if key.len() >= 2 && !value.is_empty() {
                return Some((
                    "claim".to_owned(),
                    source_path.to_owned(),
                    key.to_ascii_lowercase().replace(' ', "_"),
                    Some(value.to_owned()),
                ));
            }
        }
    }
    for phrase in [" depends on ", " uses ", " references "] {
        if let Some((left, right)) = line.split_once(phrase) {
            let left = left.trim();
            let right = right.trim().trim_end_matches('.');
            if !left.is_empty() && !right.is_empty() {
                return Some((
                    "relation".to_owned(),
                    left.to_owned(),
                    phrase.trim().replace(' ', "_"),
                    Some(right.to_owned()),
                ));
            }
        }
    }
    if line.contains("ignore previous") || line.contains("system prompt") {
        return Some((
            "claim".to_owned(),
            source_scope.to_owned(),
            "contains_untrusted_instruction_text".to_owned(),
            Some(chunk_id.to_owned()),
        ));
    }

    None
}

#[cfg(test)]
mod mod_tests;
