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
mod tests {
    use super::public_interface_chunk_bonus;
    use crate::domain::{
        CodeQueryKind, CodeRepositorySelector, CodeRetrievalRequest, FreshnessPolicy,
    };

    #[test]
    fn interface_intent_boosts_public_header_declarations() {
        let request = hybrid_request("cache interface lookup insert total charge");
        let bonus = public_interface_chunk_bonus(
            4.0,
            &request.query,
            "class LEVELDB_EXPORT Cache {\n public:\n  virtual Handle* Insert() = 0;\n};",
            "include/leveldb/cache.h",
            &request,
        );

        assert!(bonus > 0.0);
    }

    #[test]
    fn interface_bonus_ignores_implementation_and_non_interface_queries() {
        let request = hybrid_request("cache lookup insert total charge");

        assert_eq!(
            public_interface_chunk_bonus(
                4.0,
                &request.query,
                "class Cache { public: Handle* Insert(); };",
                "include/leveldb/cache.h",
                &request,
            ),
            0.0
        );
        let interface_request = hybrid_request("cache interface lookup insert total charge");
        assert_eq!(
            public_interface_chunk_bonus(
                4.0,
                &interface_request.query,
                "class LRUCache { public: Handle* Insert(); };",
                "util/cache.cc",
                &interface_request,
            ),
            0.0
        );
    }

    fn hybrid_request(query: &str) -> CodeRetrievalRequest {
        let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate");
        CodeRetrievalRequest::new(
            query,
            selector,
            CodeQueryKind::Hybrid,
            10,
            FreshnessPolicy::AllowStale,
        )
        .expect("request should validate")
    }
}
