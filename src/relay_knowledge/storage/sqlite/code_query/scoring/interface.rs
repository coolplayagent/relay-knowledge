use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

use super::code_query_path_ranking::path_looks_like_test_or_benchmark;

pub(super) fn public_interface_chunk_bonus(
    base_score: f64,
    query: &str,
    content: &str,
    path: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0
        || request.code_query_kind != CodeQueryKind::Hybrid
        || !query_mentions_public_interface(query)
        || !path_looks_like_header(path)
        || path_looks_like_test_or_benchmark(path)
        || !content_looks_like_public_interface(content)
    {
        return 0.0;
    }

    2.25
}

fn query_mentions_public_interface(query: &str) -> bool {
    query
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|term| !term.is_empty())
        .any(|term| {
            matches!(
                term.to_ascii_lowercase().as_str(),
                "api" | "contract" | "interface" | "interfaces" | "public"
            )
        })
}

fn path_looks_like_header(path: &str) -> bool {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    file_name.rsplit_once('.').is_some_and(|(_, extension)| {
        matches!(
            extension.to_ascii_lowercase().as_str(),
            "h" | "hh" | "hpp" | "hxx" | "inc" | "ipp"
        )
    })
}

fn content_looks_like_public_interface(content: &str) -> bool {
    content
        .lines()
        .map(str::trim)
        .take(12)
        .any(interface_declaration_line)
}

fn interface_declaration_line(line: &str) -> bool {
    line.starts_with("class ")
        || line.starts_with("struct ")
        || line.starts_with("interface ")
        || line.starts_with("protocol ")
        || line.starts_with("trait ")
        || line.starts_with("export class ")
        || line.contains(" class ")
        || line.contains(" struct ")
        || line.contains(" interface ")
}

#[cfg(test)]
#[path = "interface_tests.rs"]
mod tests;
