use crate::domain::CodeParseStatus;

use super::super::{CodeIndexError, languages::LanguageSpec};

pub(super) const MAX_TEXT_FILE_BYTES: usize = 512 * 1024;

pub(super) fn validate_text_content(
    path: &str,
    bytes: &[u8],
    language: Option<LanguageSpec>,
) -> Result<(CodeParseStatus, Option<String>, Option<String>), CodeIndexError> {
    if bytes.contains(&0) {
        return Ok((
            CodeParseStatus::TextOnly,
            Some("binary file skipped; lexical code retrieval is unavailable".to_owned()),
            None,
        ));
    }

    let mut degraded_reasons = Vec::new();
    let content_bytes = if bytes.len() > MAX_TEXT_FILE_BYTES {
        degraded_reasons.push(format!(
            "file exceeds {MAX_TEXT_FILE_BYTES} byte code index budget"
        ));
        truncate_bytes_at_utf8_boundary(bytes, MAX_TEXT_FILE_BYTES)
    } else {
        bytes
    };
    let content = match String::from_utf8(content_bytes.to_vec()) {
        Ok(content) => content,
        Err(_) => {
            degraded_reasons.push(format!(
                "{path} is not valid UTF-8; using lossy text fallback"
            ));
            String::from_utf8_lossy(content_bytes).into_owned()
        }
    };
    if language.is_none() {
        degraded_reasons
            .push("tree-sitter grammar is not configured for this file extension".to_owned());
    }

    if !degraded_reasons.is_empty() {
        return Ok((
            CodeParseStatus::TextOnly,
            Some(degraded_reasons.join("; ")),
            Some(content),
        ));
    }

    Ok((CodeParseStatus::Parsed, None, Some(content)))
}

fn truncate_bytes_at_utf8_boundary(bytes: &[u8], max_bytes: usize) -> &[u8] {
    let end = bytes.len().min(max_bytes);
    match std::str::from_utf8(&bytes[..end]) {
        Ok(_) => &bytes[..end],
        Err(error) if error.error_len().is_none() => &bytes[..error.valid_up_to()],
        Err(_) => &bytes[..end],
    }
}

pub(super) fn count_lines(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }

    bytes.iter().filter(|byte| **byte == b'\n').count() + 1
}
