use std::{collections::BTreeSet, path::PathBuf};

use crate::domain::{CodeRepositoryRegistration, RepositoryCodeRange};

use super::{
    CodeIndexError, scope,
    source::{source_bytes_after_content_verification, source_commit_is_filesystem},
};

/// Exact source declaration recovered from an indexed Git snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceDeclarationMatch {
    pub(crate) path: String,
    pub(crate) excerpt: String,
    pub(crate) byte_range: RepositoryCodeRange,
    pub(crate) line_range: RepositoryCodeRange,
}

const MAX_SOURCE_DECLARATION_FILES: usize = 8;
const MAX_SOURCE_DECLARATION_BYTES: usize = 512 * 1024;

/// Reads a bounded set of indexed Git blobs and returns exact declaration lines.
pub(crate) fn source_declarations_for_identity(
    registration: &CodeRepositoryRegistration,
    commit: &str,
    paths: Vec<String>,
    path_filters: &[String],
    language_filters: &[String],
    identity: &str,
) -> Result<Vec<SourceDeclarationMatch>, CodeIndexError> {
    if !simple_source_identifier(identity) {
        return Ok(Vec::new());
    }

    let root = PathBuf::from(&registration.root_path);
    let filesystem_hashes = if source_commit_is_filesystem(commit) {
        match scope::scoped_source_snapshot_for_registration(registration, commit).or_else(|_| {
            scope::scoped_source_snapshot_for_registration_filters(
                registration,
                commit,
                path_filters,
                language_filters,
            )
        }) {
            Ok(snapshot) => Some(snapshot.content_hashes),
            Err(_) => return Ok(Vec::new()),
        }
    } else {
        None
    };
    let mut seen = BTreeSet::new();
    let mut files_considered = 0usize;
    let mut matches = Vec::new();
    for path in paths {
        if files_considered >= MAX_SOURCE_DECLARATION_FILES {
            break;
        }
        if !safe_git_blob_path(&path) || !seen.insert(path.clone()) {
            continue;
        }
        files_considered += 1;
        let Ok(bytes) = source_bytes_after_content_verification(
            &root,
            commit,
            &path,
            filesystem_hashes.as_ref(),
        ) else {
            continue;
        };
        if bytes.len() > MAX_SOURCE_DECLARATION_BYTES {
            continue;
        }
        let Ok(content) = std::str::from_utf8(&bytes) else {
            continue;
        };
        if let Some(declaration) = first_source_declaration_match(&path, content, identity)? {
            matches.push(declaration);
        }
    }

    Ok(matches)
}

fn first_source_declaration_match(
    path: &str,
    content: &str,
    identity: &str,
) -> Result<Option<SourceDeclarationMatch>, CodeIndexError> {
    let mut byte_start = 0usize;
    for (line_index, line) in content.split_inclusive('\n').enumerate() {
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        let byte_end = byte_start + line_without_newline.len();
        if source_line_defines_identity(line_without_newline.trim(), identity) {
            let line_number = line_index + 1;
            return Ok(Some(SourceDeclarationMatch {
                path: path.to_owned(),
                excerpt: line_without_newline.trim().to_owned(),
                byte_range: RepositoryCodeRange::new("byte_range", byte_start, byte_end)
                    .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
                line_range: RepositoryCodeRange::new("line_range", line_number, line_number)
                    .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
            }));
        }
        byte_start += line.len();
    }

    Ok(None)
}

pub(crate) fn source_line_defines_identity(line: &str, identity: &str) -> bool {
    if line.is_empty() || !line_contains_identifier(line, identity) {
        return false;
    }
    if line.starts_with("typedef ") || line.contains(" typedef ") {
        return true;
    }
    if line.starts_with("#define ") {
        return line
            .strip_prefix("#define ")
            .is_some_and(|suffix| line_starts_with_identifier(suffix, identity));
    }
    if line
        .strip_prefix("using ")
        .or_else(|| line.strip_prefix("typealias "))
        .is_some_and(|suffix| line_starts_with_identifier(suffix, identity))
    {
        return true;
    }
    if ["struct ", "class ", "enum ", "union ", "interface "]
        .into_iter()
        .filter_map(|prefix| line.strip_prefix(prefix))
        .any(|suffix| line_starts_with_identifier(suffix, identity))
    {
        return true;
    }

    line.contains('(') && line_looks_like_function_definition(line, identity)
}

fn line_looks_like_function_definition(line: &str, identity: &str) -> bool {
    line.match_indices(identity).any(|(identity_start, _)| {
        if !identifier_match_has_boundaries(line, identity, identity_start) {
            return false;
        }
        let prefix = line[..identity_start].trim_start();
        let suffix = line[identity_start + identity.len()..].trim_start();
        if !suffix.starts_with('(') || prefix.contains('=') {
            return false;
        }
        if prefix.chars().next_back().is_some_and(|character| {
            matches!(character, '(' | '.' | '>') || (character == ':' && !prefix.ends_with("::"))
        }) {
            return false;
        }
        !matches!(
            prefix.split_whitespace().next(),
            Some("if" | "for" | "while" | "switch" | "return")
        )
    })
}

fn line_starts_with_identifier(line: &str, identifier: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with(identifier)
        && trimmed
            .get(identifier.len()..)
            .is_some_and(|suffix| suffix.chars().next().is_none_or(|c| !is_identifier_char(c)))
}

fn line_contains_identifier(line: &str, identifier: &str) -> bool {
    line.match_indices(identifier)
        .any(|(start, _)| identifier_match_has_boundaries(line, identifier, start))
}

fn identifier_match_has_boundaries(line: &str, identifier: &str, start: usize) -> bool {
    let end = start + identifier.len();
    line.get(..start).is_some_and(|prefix| {
        prefix
            .chars()
            .next_back()
            .is_none_or(|c| !is_identifier_char(c))
    }) && line
        .get(end..)
        .is_some_and(|suffix| suffix.chars().next().is_none_or(|c| !is_identifier_char(c)))
}

pub(crate) fn simple_source_identifier(value: &str) -> bool {
    !value.is_empty() && value.chars().all(is_identifier_char)
}

fn is_identifier_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

pub(crate) fn safe_git_blob_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && !path.contains('\n')
        && !path.contains('\r')
        && path.split('/').all(|part| !part.is_empty() && part != "..")
}
