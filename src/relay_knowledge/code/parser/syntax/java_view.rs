//! A same-length lexical view bridges Java's identifier alphabet to the grammar.
use crate::code::java_identifiers::{is_ignorable, is_part, is_start};
use std::borrow::Cow;

// One source scan, at most one source-sized allocation. Original source remains
// the only input to fact text extraction and query predicates; byte offsets and
// line breaks are unchanged. This runs inside the bounded parser worker stage.
pub(super) fn identifier_view(source: &str) -> Cow<'_, [u8]> {
    let mut view = Cow::Borrowed(source.as_bytes());
    let mut offset = 0;
    while offset < source.len() {
        let tail = &source[offset..];
        if tail.starts_with("//") {
            offset += tail.find(['\r', '\n']).unwrap_or(tail.len());
            continue;
        }
        if let Some(comment) = tail.strip_prefix("/*") {
            offset += comment.find("*/").map_or(tail.len(), |end| end + 4);
            continue;
        }
        let value = tail.chars().next().expect("nonempty UTF-8 tail");
        if matches!(value, '\'' | '"') {
            offset = literal_end(source.as_bytes(), offset, value as u8);
            continue;
        }
        if !is_start(value) {
            offset += value.len_utf8();
            continue;
        }
        while offset < source.len() {
            let next = source[offset..]
                .chars()
                .next()
                .expect("nonempty UTF-8 tail");
            if !is_part(next) {
                break;
            }
            let end = offset + next.len_utf8();
            if !next.is_ascii() || is_ignorable(next) {
                view.to_mut()[offset..end].fill(b'$');
            }
            offset = end;
        }
    }
    view
}

fn literal_end(bytes: &[u8], start: usize, quote: u8) -> usize {
    let delimiter = if quote == b'"' && bytes[start..].starts_with(b"\"\"\"") {
        3
    } else {
        1
    };
    let mut offset = start + delimiter;
    while let Some(&value) = bytes.get(offset) {
        if value == b'\\' {
            offset = (offset + 2).min(bytes.len());
        } else {
            if value == quote && (delimiter == 1 || bytes[offset..].starts_with(b"\"\"\"")) {
                return offset + delimiter;
            }
            offset += 1;
        }
    }
    offset
}

#[cfg(test)]
#[path = "java_view_tests.rs"]
mod tests;
