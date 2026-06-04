use super::{SourceGrepKind, SourceGrepRequest};

pub(super) fn source_grep_queries(request: &SourceGrepRequest) -> Vec<Vec<u8>> {
    let query = request.query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let mut queries = vec![query.as_bytes().to_vec()];
    if request.kind == SourceGrepKind::Hybrid {
        for term in query.split(|character: char| !source_grep_identifier_char(character)) {
            if term.len() < 3 || !term.chars().all(source_grep_identifier_char) {
                continue;
            }
            let bytes = term.as_bytes().to_vec();
            if !queries.iter().any(|existing| existing == &bytes) {
                queries.push(bytes);
            }
            if queries.len() >= 12 {
                break;
            }
        }
    }

    queries
}

pub(super) fn find_query_bytes(haystack: &[u8], queries: &[Vec<u8>]) -> Option<(usize, usize)> {
    for query in queries {
        if query.is_empty() || query.len() > haystack.len() {
            continue;
        }
        if let Some(start) = haystack
            .windows(query.len())
            .position(|window| window == query.as_slice())
        {
            return Some((start, start + query.len()));
        }
    }

    None
}

fn source_grep_identifier_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}
