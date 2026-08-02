use std::{fs, path::Path};

use crate::{
    code::{CodeIndexError, generated_detection, languages::language_id},
    domain::RepositoryCodeRange,
};

use super::{
    SourceGrepKind, SourceGrepMatch, SourceGrepRequest,
    query::{find_query_bytes, source_grep_queries},
};

const MAX_GREP_LINE_BYTES: usize = 4096;

pub(super) fn internal_source_grep_matches(
    root: &Path,
    paths: &[String],
    request: &SourceGrepRequest,
    accepts: impl Fn(&SourceGrepMatch) -> bool,
) -> Result<Vec<SourceGrepMatch>, CodeIndexError> {
    let queries = source_grep_queries(request);
    if queries.is_empty() {
        return Ok(Vec::new());
    }

    let mut handwritten_matches = Vec::new();
    let mut generated_matches = Vec::new();
    for path in paths {
        if handwritten_matches.len() >= request.limit
            && (request.exclude_generated || generated_matches.len() >= request.limit)
        {
            break;
        }
        let Ok(bytes) = fs::read(root.join(path)) else {
            continue;
        };
        if source_bytes_are_binary(&bytes) {
            continue;
        }
        let is_generated = generated_detection::is_generated_file(path, &bytes);
        if request.exclude_generated && is_generated {
            continue;
        }
        let matches = if is_generated {
            &mut generated_matches
        } else {
            &mut handwritten_matches
        };
        if matches.len() >= request.limit {
            continue;
        }
        push_internal_file_matches(
            InternalFileScan {
                path,
                bytes: &bytes,
                is_generated,
            },
            &queries,
            request.kind,
            request.limit,
            &accepts,
            matches,
        )?;
    }

    let mut matches = handwritten_matches;
    if matches.len() < request.limit {
        matches.extend(
            generated_matches
                .into_iter()
                .take(request.limit - matches.len()),
        );
    }
    Ok(matches)
}

struct InternalFileScan<'a> {
    path: &'a str,
    bytes: &'a [u8],
    is_generated: bool,
}

fn push_internal_file_matches(
    input: InternalFileScan<'_>,
    queries: &[Vec<u8>],
    kind: SourceGrepKind,
    limit: usize,
    accepts: &impl Fn(&SourceGrepMatch) -> bool,
    matches: &mut Vec<SourceGrepMatch>,
) -> Result<(), CodeIndexError> {
    let path = input.path;
    let bytes = input.bytes;
    let mut line_start = 0usize;
    let mut line_number = 1usize;
    let mut previous_line = None;
    while line_start < bytes.len() && matches.len() < limit {
        let line_end = bytes[line_start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |offset| line_start + offset);
        let line = &bytes[line_start..line_end];
        let mut carried_line = SourceLineContext {
            byte_start: line_start,
            byte_end: line_end,
            line_start: line_number,
        };
        if let Some((match_start, match_end)) = find_query_bytes(line, queries) {
            if line.len() > MAX_GREP_LINE_BYTES && kind == SourceGrepKind::Definition {
                line_start = if line_end < bytes.len() {
                    line_end + 1
                } else {
                    bytes.len()
                };
                line_number += 1;
                continue;
            }
            let context = source_grep_line_context(bytes, line_start, line_end, previous_line);
            if let Some(context) = context {
                carried_line = context;
            }
            let byte_range = RepositoryCodeRange::new(
                "byte_range",
                context
                    .as_ref()
                    .map_or(line_start + match_start, |context| context.byte_start),
                context
                    .as_ref()
                    .map_or(line_start + match_end, |context| context.byte_end),
            )
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?;
            let line_range = RepositoryCodeRange::new(
                "line_range",
                context
                    .as_ref()
                    .map_or(line_number, |context| context.line_start),
                line_number,
            )
            .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?;
            let excerpt = context.map_or_else(
                || {
                    String::from_utf8_lossy(source_line_excerpt(line, match_start, match_end))
                        .trim_end_matches('\r')
                        .trim()
                        .to_owned()
                },
                |context| {
                    String::from_utf8_lossy(&bytes[context.byte_start..context.byte_end])
                        .trim_end_matches('\r')
                        .trim()
                        .to_owned()
                },
            );
            let matched = SourceGrepMatch {
                path: path.to_owned(),
                language_id: language_id(path).unwrap_or("unknown").to_owned(),
                excerpt,
                byte_range,
                line_range,
                is_generated: input.is_generated,
            };
            if accepts(&matched) {
                matches.push(matched);
            }
        }
        previous_line = Some(carried_line);
        line_start = if line_end < bytes.len() {
            line_end + 1
        } else {
            bytes.len()
        };
        line_number += 1;
    }

    Ok(())
}

#[derive(Clone, Copy)]
struct SourceLineContext {
    byte_start: usize,
    byte_end: usize,
    line_start: usize,
}

fn source_grep_line_context(
    bytes: &[u8],
    line_start: usize,
    line_end: usize,
    previous_line: Option<SourceLineContext>,
) -> Option<SourceLineContext> {
    let previous = previous_line?;
    let previous_line = std::str::from_utf8(&bytes[previous.byte_start..previous.byte_end])
        .ok()?
        .trim();
    let current_line = std::str::from_utf8(&bytes[line_start..line_end])
        .ok()?
        .trim_start();
    if previous_line.starts_with("template ")
        || (current_line.starts_with('.')
            && (previous_line.ends_with('{')
                || previous_line
                    .lines()
                    .next()
                    .is_some_and(|line| line.trim_end().ends_with('{'))))
    {
        Some(SourceLineContext {
            byte_start: previous.byte_start,
            byte_end: line_end,
            line_start: previous.line_start,
        })
    } else {
        None
    }
}

fn source_bytes_are_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0)
}

fn source_line_excerpt(line: &[u8], match_start: usize, match_end: usize) -> &[u8] {
    if line.len() <= MAX_GREP_LINE_BYTES {
        return line;
    }

    let match_len = match_end.saturating_sub(match_start);
    let budget = MAX_GREP_LINE_BYTES.max(match_len);
    let ideal_start = match_start.saturating_sub((budget.saturating_sub(match_len)) / 2);
    let max_start = line.len().saturating_sub(budget);
    let start = ideal_start.min(max_start);
    let end = start.saturating_add(budget).min(line.len());
    &line[start..end]
}

#[cfg(test)]
#[path = "scanner_tests.rs"]
mod tests;
