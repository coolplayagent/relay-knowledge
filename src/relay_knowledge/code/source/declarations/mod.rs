//! Bounded source-declaration fallback and safe blob-path validation.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use crate::{
    code::generated_detection,
    domain::{CodeRepositoryRegistration, RepositoryCodeRange},
};

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
    pub(crate) is_generated: bool,
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
    exclude_generated: bool,
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
    source_declarations_for_identity_with_hashes(
        &root,
        commit,
        paths,
        identity,
        exclude_generated,
        filesystem_hashes.as_ref(),
    )
}

pub(crate) fn source_declarations_for_identity_from_worktree_overlay(
    registration: &CodeRepositoryRegistration,
    expected_hashes: BTreeMap<String, String>,
    paths: Vec<String>,
    identity: &str,
    exclude_generated: bool,
) -> Result<Vec<SourceDeclarationMatch>, CodeIndexError> {
    if !simple_source_identifier(identity) {
        return Ok(Vec::new());
    }

    let root = PathBuf::from(&registration.root_path);
    source_declarations_for_identity_with_hashes(
        &root,
        "filesystem:worktree-overlay-fallback",
        paths,
        identity,
        exclude_generated,
        Some(&expected_hashes),
    )
}

fn source_declarations_for_identity_with_hashes(
    root: &Path,
    commit: &str,
    paths: Vec<String>,
    identity: &str,
    exclude_generated: bool,
    expected_hashes: Option<&BTreeMap<String, String>>,
) -> Result<Vec<SourceDeclarationMatch>, CodeIndexError> {
    let mut seen = BTreeSet::new();
    let mut files_inspected = 0usize;
    let mut files_considered = 0usize;
    let mut matches = Vec::new();
    for path in paths {
        if files_inspected >= MAX_SOURCE_DECLARATION_FILES
            || files_considered >= MAX_SOURCE_DECLARATION_FILES
        {
            break;
        }
        if !safe_git_blob_path(&path) || !seen.insert(path.clone()) {
            continue;
        }
        if exclude_generated && generated_detection::path_has_generated_signal(&path) {
            continue;
        }
        files_inspected += 1;
        let Ok(bytes) =
            source_bytes_after_content_verification(root, commit, &path, expected_hashes)
        else {
            continue;
        };
        if bytes.len() > MAX_SOURCE_DECLARATION_BYTES {
            continue;
        }
        let is_generated = generated_detection::is_generated_file(&path, &bytes);
        if exclude_generated && is_generated {
            continue;
        }
        let Ok(content) = std::str::from_utf8(&bytes) else {
            continue;
        };
        files_considered += 1;
        if let Some(declaration) =
            first_source_declaration_match(&path, content, identity, is_generated)?
        {
            matches.push(declaration);
        }
    }

    Ok(matches)
}

fn first_source_declaration_match(
    path: &str,
    content: &str,
    identity: &str,
    is_generated: bool,
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
                is_generated,
            }));
        }
        byte_start += line.len();
    }

    Ok(None)
}

pub(crate) fn source_line_defines_identity(line: &str, identity: &str) -> bool {
    let line = line.trim();
    if line.is_empty() || !line_contains_identifier(line, identity) {
        return false;
    }
    if line.starts_with("#define ") {
        return line
            .strip_prefix("#define ")
            .is_some_and(|suffix| line_starts_with_identifier(suffix, identity));
    }
    if source_comment_line(line) {
        return false;
    }
    if line.starts_with("typedef ") || line.contains(" typedef ") {
        return true;
    }
    let declaration = strip_declaration_modifiers(line);
    if declaration
        .strip_prefix("using ")
        .or_else(|| declaration.strip_prefix("typealias "))
        .is_some_and(|suffix| line_starts_with_identifier(suffix, identity))
    {
        return true;
    }
    if [
        "enum class ",
        "enum struct ",
        "record class ",
        "record struct ",
        "@interface ",
        "struct ",
        "class ",
        "enum ",
        "union ",
        "interface ",
        "trait ",
        "protocol ",
        "record ",
        "actor ",
    ]
    .into_iter()
    .filter_map(|prefix| declaration.strip_prefix(prefix))
    .any(|suffix| line_starts_with_identifier(suffix, identity))
    {
        return true;
    }
    if declaration
        .strip_prefix(identity)
        .is_some_and(|suffix| suffix.trim_start().starts_with('('))
    {
        return false;
    }

    declaration.contains('(') && line_looks_like_function_definition(declaration, identity)
}

fn source_comment_line(line: &str) -> bool {
    line.starts_with("//")
        || line.starts_with("/*")
        || line.starts_with('*')
        || (line.starts_with('#') && !line.starts_with("#["))
        || line.starts_with("--")
        || line.starts_with("<!--")
}

fn strip_declaration_modifiers(mut line: &str) -> &str {
    loop {
        let trimmed = line.trim_start();
        if let Some(suffix) = strip_declaration_attribute(trimmed) {
            line = suffix;
            continue;
        }
        let Some((head, tail)) = trimmed.split_once(char::is_whitespace) else {
            return trimmed;
        };
        if !declaration_modifier(head) {
            return trimmed;
        }
        line = tail;
    }
}

fn strip_declaration_attribute(line: &str) -> Option<&str> {
    if line.starts_with("@interface") {
        return None;
    }
    if line.starts_with("#[") {
        return balanced_prefix_suffix(line, 1, '[', ']');
    }
    if line.starts_with('[') {
        return balanced_prefix_suffix(line, 0, '[', ']');
    }
    let annotation = line.strip_prefix('@')?;
    let name_end = annotation
        .char_indices()
        .find(|(_, character)| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '$'))
        })
        .map_or(annotation.len(), |(index, _)| index);
    if name_end == 0 {
        return None;
    }
    let after_name = &annotation[name_end..];
    let whitespace_len = after_name.len() - after_name.trim_start().len();
    let after_whitespace = &after_name[whitespace_len..];
    if after_whitespace.starts_with('(') {
        let open_index = line.len() - after_whitespace.len();
        balanced_prefix_suffix(line, open_index, '(', ')')
    } else {
        Some(after_name)
    }
}

fn balanced_prefix_suffix(value: &str, open_index: usize, open: char, close: char) -> Option<&str> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (offset, character) in value[open_index..].char_indices() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
        } else if character == open {
            depth += 1;
        } else if character == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                let end = open_index + offset + character.len_utf8();
                return value.get(end..);
            }
        }
    }
    None
}

fn declaration_modifier(token: &str) -> bool {
    matches!(
        token,
        "abstract"
            | "async"
            | "case"
            | "consteval"
            | "constexpr"
            | "convenience"
            | "data"
            | "default"
            | "distributed"
            | "explicit"
            | "export"
            | "extern"
            | "file"
            | "final"
            | "friend"
            | "indirect"
            | "inline"
            | "internal"
            | "mutating"
            | "native"
            | "non-sealed"
            | "nonmutating"
            | "open"
            | "override"
            | "partial"
            | "private"
            | "protected"
            | "pub"
            | "public"
            | "readonly"
            | "ref"
            | "required"
            | "sealed"
            | "static"
            | "strictfp"
            | "synchronized"
            | "unsafe"
            | "value"
            | "virtual"
    ) || (token.starts_with("pub(") && token.ends_with(')'))
        || (token.starts_with("private[") && token.ends_with(']'))
        || (token.starts_with("protected[") && token.ends_with(']'))
        || (token.starts_with('@') && token != "@interface")
}

fn line_looks_like_function_definition(line: &str, identity: &str) -> bool {
    line.match_indices(identity).any(|(identity_start, _)| {
        if !identifier_match_has_boundaries(line, identity, identity_start) {
            return false;
        }
        let prefix = line[..identity_start].trim_start();
        let suffix = line[identity_start + identity.len()..].trim_start();
        if !suffix.starts_with('(')
            || prefix.contains('=')
            || prefix.ends_with('~')
            || (prefix.ends_with("::") && !prefix.contains(char::is_whitespace))
        {
            return false;
        }
        if prefix.chars().next_back().is_some_and(|character| {
            matches!(character, '(' | '.' | '>') || (character == ':' && !prefix.ends_with("::"))
        }) {
            return false;
        }
        !matches!(
            prefix.split_whitespace().next(),
            Some(
                "await"
                    | "co_await"
                    | "delete"
                    | "for"
                    | "if"
                    | "new"
                    | "return"
                    | "switch"
                    | "throw"
                    | "try"
                    | "while"
                    | "yield"
            )
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
