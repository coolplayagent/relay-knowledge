use std::ops::Range;

use crate::{
    code::{SnapshotBuild, stable_id},
    domain::{
        CodeFrameworkEdgeRecord, CodeFrameworkNodeRecord, FrameworkEdgeKind, FrameworkKind,
        FrameworkNodeKind, RepositoryCodeRange,
    },
};

use super::FrameworkFileInput;

pub(super) fn framework_node(
    build: &SnapshotBuild,
    input: &FrameworkFileInput<'_>,
    framework: FrameworkKind,
    kind: FrameworkNodeKind,
    name: &str,
    detail: Option<String>,
    byte_range: Range<usize>,
) -> CodeFrameworkNodeRecord {
    let byte_start = byte_range.start;
    let byte_end = byte_range.end;
    let byte_end = byte_end.max(byte_start).min(input.content.len());
    let line_range = line_range(input.content, byte_start, byte_end);
    let symbol_snapshot_id = input
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .map(|symbol| symbol.symbol_snapshot_id.clone());
    let node_id = stable_id(
        "framework-node",
        [
            build.repository_id.as_str(),
            build.source_scope.as_str(),
            input.path,
            framework.as_str(),
            kind.as_str(),
            name,
            &byte_start.to_string(),
        ],
    );
    CodeFrameworkNodeRecord {
        repository_id: build.repository_id.clone(),
        source_scope: build.source_scope.clone(),
        node_id,
        file_id: input.file_id.to_owned(),
        path: input.path.to_owned(),
        framework,
        kind,
        name: name.to_owned(),
        detail,
        symbol_snapshot_id,
        byte_range: RepositoryCodeRange {
            start: bounded_u32(byte_start),
            end: bounded_u32(byte_end),
        },
        line_range,
    }
}

pub(super) fn framework_edge(
    build: &SnapshotBuild,
    input: &FrameworkFileInput<'_>,
    framework: FrameworkKind,
    kind: FrameworkEdgeKind,
    source_node_id: &str,
    target: (Option<String>, Option<String>),
    byte_range: Range<usize>,
) -> CodeFrameworkEdgeRecord {
    let (target_node_id, target_hint) = target;
    let byte_start = byte_range.start;
    let byte_end = byte_range.end;
    let byte_end = byte_end.max(byte_start).min(input.content.len());
    let resolution_state = if target_node_id.is_some() {
        "resolved"
    } else {
        "unresolved"
    };
    let confidence_basis_points = if target_node_id.is_some() {
        10_000
    } else {
        6_000
    };
    let edge_id = stable_id(
        "framework-edge",
        [
            build.repository_id.as_str(),
            build.source_scope.as_str(),
            input.path,
            framework.as_str(),
            kind.as_str(),
            source_node_id,
            target_hint.as_deref().unwrap_or_default(),
            &byte_start.to_string(),
        ],
    );
    CodeFrameworkEdgeRecord {
        repository_id: build.repository_id.clone(),
        source_scope: build.source_scope.clone(),
        edge_id,
        file_id: input.file_id.to_owned(),
        path: input.path.to_owned(),
        framework,
        kind,
        source_node_id: source_node_id.to_owned(),
        target_node_id,
        target_hint,
        resolution_state: resolution_state.to_owned(),
        confidence_basis_points,
        confidence_tier: if confidence_basis_points == 10_000 {
            "exact"
        } else {
            "extracted"
        }
        .to_owned(),
        byte_range: RepositoryCodeRange {
            start: bounded_u32(byte_start),
            end: bounded_u32(byte_end),
        },
        line_range: line_range(input.content, byte_start, byte_end),
    }
}

pub(super) fn line_range(content: &str, byte_start: usize, byte_end: usize) -> RepositoryCodeRange {
    let start = content
        .as_bytes()
        .get(..byte_start.min(content.len()))
        .unwrap_or_default()
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1;
    let end = start
        + content
            .as_bytes()
            .get(byte_start.min(content.len())..byte_end.min(content.len()))
            .unwrap_or_default()
            .iter()
            .filter(|byte| **byte == b'\n')
            .count();
    RepositoryCodeRange {
        start: bounded_u32(start),
        end: bounded_u32(end),
    }
}

fn bounded_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

pub(super) fn balanced_region(
    content: &str,
    open: usize,
    opening: u8,
    closing: u8,
) -> Option<usize> {
    let bytes = content.as_bytes();
    if bytes.get(open) != Some(&opening) {
        return None;
    }
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (offset, byte) in bytes.iter().copied().enumerate().skip(open) {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(byte, b'\'' | b'"' | b'`') {
            quote = Some(byte);
        } else if byte == opening {
            depth += 1;
        } else if byte == closing {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(offset + 1);
            }
        }
    }
    None
}

pub(super) fn quoted_property(content: &str, property: &str) -> Option<(String, usize, usize)> {
    let mut search_start = 0usize;
    while let Some(relative_start) = content.get(search_start..)?.find(property) {
        let property_start = search_start + relative_start;
        let property_end = property_start + property.len();
        let boundary_before = property_start == 0
            || content
                .as_bytes()
                .get(property_start - 1)
                .is_some_and(|byte| {
                    !byte.is_ascii_alphanumeric() && *byte != b'_' && *byte != b'$'
                });
        let mut cursor = property_end;
        while content
            .as_bytes()
            .get(cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            cursor += 1;
        }
        if boundary_before && content.as_bytes().get(cursor) == Some(&b':') {
            cursor += 1;
            while content
                .as_bytes()
                .get(cursor)
                .is_some_and(u8::is_ascii_whitespace)
            {
                cursor += 1;
            }
            let quote = *content.as_bytes().get(cursor)?;
            if matches!(quote, b'\'' | b'"' | b'`') {
                let value_start = cursor;
                cursor += 1;
                let mut escaped = false;
                while let Some(byte) = content.as_bytes().get(cursor).copied() {
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == quote {
                        return Some((
                            content.get(value_start + 1..cursor)?.to_owned(),
                            value_start,
                            cursor + 1,
                        ));
                    }
                    cursor += 1;
                }
                return None;
            }
        }
        search_start = property_end;
    }
    None
}

pub(super) fn identifiers(expression: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut cursor = 0usize;
    std::iter::from_fn(move || {
        while cursor < expression.len()
            && !expression.as_bytes()[cursor].is_ascii_alphabetic()
            && expression.as_bytes()[cursor] != b'_'
            && expression.as_bytes()[cursor] != b'$'
        {
            cursor += 1;
        }
        let start = cursor;
        while cursor < expression.len()
            && (expression.as_bytes()[cursor].is_ascii_alphanumeric()
                || matches!(expression.as_bytes()[cursor], b'_' | b'$'))
        {
            cursor += 1;
        }
        (start < cursor).then(|| (start, &expression[start..cursor]))
    })
}

pub(super) fn expression_identifier(name: &str) -> bool {
    !matches!(
        name,
        "as" | "const"
            | "else"
            | "false"
            | "for"
            | "if"
            | "in"
            | "let"
            | "null"
            | "of"
            | "return"
            | "this"
            | "track"
            | "true"
            | "undefined"
    ) && !name.starts_with('$')
}

pub(super) fn relative_module_path(source_path: &str, target: &str) -> String {
    let mut parts = source_path.split('/').collect::<Vec<_>>();
    parts.pop();
    for segment in target.split('/') {
        match segment {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            segment => parts.push(segment),
        }
    }
    parts.join("/")
}
