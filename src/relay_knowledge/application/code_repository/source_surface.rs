use crate::{
    code::source_line_defines_identity,
    domain::{CodeRetrievalHit, CodeRetrievalLayer},
};

pub(super) fn hit_has_complete_source_surface(hit: &CodeRetrievalHit, identity: &str) -> bool {
    if !hit.retrieval_layers.iter().any(|layer| {
        matches!(
            layer,
            CodeRetrievalLayer::Symbol
                | CodeRetrievalLayer::Definition
                | CodeRetrievalLayer::CallGraph
        )
    }) {
        return false;
    }

    hit.excerpt
        .lines()
        .map(str::trim)
        .any(|line| source_surface_line_defines_identity(line, identity))
}

fn source_surface_line_defines_identity(line: &str, identity: &str) -> bool {
    if line.is_empty() || source_identifier_ranges(line, identity).next().is_none() {
        return false;
    }
    if synthetic_qualified_summary_line(line) {
        return false;
    }
    if source_line_defines_identity(line, identity) {
        return true;
    }
    let exported_type_surface = line
        .trim_start()
        .strip_prefix("export ")
        .or_else(|| line.trim_start().strip_prefix("pub "))
        .is_some_and(|stripped| stripped.trim_start().starts_with("type "));
    let stripped = strip_source_surface_modifiers(line);
    source_line_defines_identity(stripped, identity)
        || source_value_declaration_defines_identity(stripped, identity)
        || (exported_type_surface && source_type_alias_defines_identity(stripped, identity))
}

fn synthetic_qualified_summary_line(line: &str) -> bool {
    let Some((prefix, _)) = line.split_once(':') else {
        return false;
    };
    let prefix = prefix.trim();
    (prefix.contains('.') || prefix.contains("::"))
        && prefix.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | ':')
        })
}

fn strip_source_surface_modifiers(mut line: &str) -> &str {
    loop {
        let trimmed = line.trim_start();
        let Some(stripped) = trimmed
            .strip_prefix("export default ")
            .or_else(|| trimmed.strip_prefix("export "))
            .or_else(|| trimmed.strip_prefix("pub "))
            .or_else(|| trimmed.strip_prefix("public "))
            .or_else(|| trimmed.strip_prefix("private "))
            .or_else(|| trimmed.strip_prefix("protected "))
            .or_else(|| trimmed.strip_prefix("static "))
            .or_else(|| trimmed.strip_prefix("async "))
        else {
            return trimmed;
        };
        line = stripped;
    }
}

fn source_value_declaration_defines_identity(line: &str, identity: &str) -> bool {
    let Some(remainder) = line
        .strip_prefix("const ")
        .or_else(|| line.strip_prefix("let "))
        .or_else(|| line.strip_prefix("var "))
    else {
        return false;
    };
    source_identity_starts_declaration(remainder, identity, |after| {
        after.starts_with('=') || after.starts_with(':') || after.starts_with("satisfies ")
    })
}

fn source_type_alias_defines_identity(line: &str, identity: &str) -> bool {
    let Some(remainder) = line.strip_prefix("type ") else {
        return false;
    };
    source_identity_starts_declaration(remainder, identity, |after| {
        after.starts_with('=') || after.starts_with('<')
    })
}

fn source_identity_starts_declaration(
    remainder: &str,
    identity: &str,
    accepts_suffix: impl Fn(&str) -> bool,
) -> bool {
    source_identifier_ranges(remainder, identity)
        .next()
        .is_some_and(|(start, end)| {
            start == 0
                && remainder
                    .get(end..)
                    .is_some_and(|after| accepts_suffix(after.trim_start()))
        })
}

fn source_identifier_ranges<'a>(
    line: &'a str,
    identity: &'a str,
) -> impl Iterator<Item = (usize, usize)> + 'a {
    line.match_indices(identity).filter_map(|(start, _)| {
        let end = start + identity.len();
        let has_start_boundary = line.get(..start).is_some_and(|prefix| {
            prefix
                .chars()
                .next_back()
                .is_none_or(|character| !source_identifier_char(character))
        });
        let has_end_boundary = line.get(end..).is_some_and(|suffix| {
            suffix
                .chars()
                .next()
                .is_none_or(|character| !source_identifier_char(character))
        });
        (has_start_boundary && has_end_boundary).then_some((start, end))
    })
}

fn source_identifier_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}
