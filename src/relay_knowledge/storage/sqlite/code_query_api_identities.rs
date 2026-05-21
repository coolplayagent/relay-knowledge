use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

const MAX_HYBRID_API_IDENTITIES: usize = 6;
const MIN_SIMPLE_API_IDENTITY_LEN: usize = 4;
const SCOPED_API_IDENTITY_BONUS: f64 = 2.0;
const SIMPLE_API_IDENTITY_BONUS: f64 = 1.0;
const MAX_SEQUENCE_COVERAGE_BONUS: f64 = 0.75;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ApiSymbolIdentity {
    leaf_name: String,
    scoped_terms: Option<Vec<String>>,
}

impl ApiSymbolIdentity {
    pub(super) fn leaf_name(&self) -> &str {
        &self.leaf_name
    }

    fn is_scoped(&self) -> bool {
        self.scoped_terms.is_some()
    }

    pub(super) fn matches_symbol(
        &self,
        name: &str,
        qualified_name: &str,
        signature: &str,
        canonical_symbol_id: &str,
    ) -> bool {
        if name != self.leaf_name {
            return false;
        }
        let Some(scoped_terms) = &self.scoped_terms else {
            return true;
        };

        [qualified_name, signature, canonical_symbol_id]
            .iter()
            .any(|field| contains_scoped_terms(field, scoped_terms))
    }
}

pub(super) fn hybrid_api_symbol_identities(
    query: &str,
    request: &CodeRetrievalRequest,
) -> Vec<ApiSymbolIdentity> {
    if request.code_query_kind != CodeQueryKind::Hybrid {
        return Vec::new();
    }

    let mut identities = Vec::new();
    for raw in query.split_whitespace().map(str::trim) {
        let Some(identity) = api_symbol_identity_from_token(raw) else {
            continue;
        };
        if !identities.contains(&identity) {
            identities.push(identity);
        }
        if identities.len() >= MAX_HYBRID_API_IDENTITIES {
            break;
        }
    }

    if identities.len() >= 2 {
        identities
    } else {
        Vec::new()
    }
}

pub(super) fn api_identity_symbol_bonus(
    identities: &[ApiSymbolIdentity],
    name: &str,
    qualified_name: &str,
    signature: &str,
    canonical_symbol_id: &str,
) -> f64 {
    let Some(identity) = identities.iter().find(|identity| {
        identity.matches_symbol(name, qualified_name, signature, canonical_symbol_id)
    }) else {
        return 0.0;
    };
    let coverage_bonus =
        ((identities.len().saturating_sub(1).min(3)) as f64 * 0.5).min(MAX_SEQUENCE_COVERAGE_BONUS);
    if identity.is_scoped() {
        SCOPED_API_IDENTITY_BONUS + coverage_bonus
    } else {
        SIMPLE_API_IDENTITY_BONUS + coverage_bonus
    }
}

fn api_symbol_identity_from_token(token: &str) -> Option<ApiSymbolIdentity> {
    let token = token.trim_matches(|character: char| {
        !(character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | ':'))
    });
    if token.is_empty()
        || token.contains('/')
        || token.contains('\\')
        || token_has_path_like_extension(token)
    {
        return None;
    }

    if token.contains('.') || token.contains("::") {
        let terms = identity_terms(token);
        if terms.len() >= 2 {
            return Some(ApiSymbolIdentity {
                leaf_name: terms.last()?.clone(),
                scoped_terms: Some(
                    terms
                        .into_iter()
                        .map(|term| term.to_ascii_lowercase())
                        .collect(),
                ),
            });
        }
    }

    (simple_api_identity_token(token)).then(|| ApiSymbolIdentity {
        leaf_name: token.to_owned(),
        scoped_terms: None,
    })
}

fn simple_api_identity_token(token: &str) -> bool {
    token.len() >= MIN_SIMPLE_API_IDENTITY_LEN
        && token
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        && token_has_case_boundary(token)
}

fn token_has_case_boundary(token: &str) -> bool {
    let mut previous = None;
    token.chars().any(|character| {
        let boundary = character.is_ascii_uppercase()
            && previous.is_some_and(|previous: char| previous.is_ascii_lowercase());
        previous = Some(character);
        boundary
    })
}

fn token_has_path_like_extension(token: &str) -> bool {
    let Some((stem, extension)) = token.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty() && file_extension_is_path_like(extension)
}

fn file_extension_is_path_like(extension: &str) -> bool {
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "c" | "cc"
            | "cpp"
            | "cs"
            | "go"
            | "gradle"
            | "h"
            | "hh"
            | "hpp"
            | "hxx"
            | "java"
            | "js"
            | "json"
            | "jsx"
            | "kt"
            | "md"
            | "php"
            | "py"
            | "rb"
            | "rs"
            | "scala"
            | "sh"
            | "swift"
            | "ts"
            | "tsx"
            | "txt"
            | "xml"
            | "yaml"
            | "yml"
    )
}

fn identity_terms(token: &str) -> Vec<String> {
    token
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect()
}

fn contains_scoped_terms(field: &str, query_terms: &[String]) -> bool {
    let field_terms = scoped_terms(field);
    field_terms
        .windows(query_terms.len())
        .any(|window| window == query_terms)
}

fn scoped_terms(value: &str) -> Vec<String> {
    value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

    #[test]
    fn hybrid_api_identities_extract_scoped_and_camel_tokens() {
        let request = request(CodeQueryKind::Hybrid);

        let identities = hybrid_api_symbol_identities(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue worker.go",
            &request,
        );

        assert_eq!(identities.len(), 4);
        assert_eq!(identities[0].leaf_name(), "New");
        assert_eq!(identities[1].leaf_name(), "RegisterWorkflow");
        assert_eq!(identities[3].leaf_name(), "InterruptCh");
    }

    #[test]
    fn hybrid_api_identity_bonus_matches_later_sequence_symbols() {
        let request = request(CodeQueryKind::Hybrid);
        let identities = hybrid_api_symbol_identities(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
            &request,
        );

        assert!(
            api_identity_symbol_bonus(
                &identities,
                "InterruptCh",
                "worker.InterruptCh",
                "func InterruptCh() <-chan interface{}",
                "repo://repo/worker.InterruptCh",
            ) >= SIMPLE_API_IDENTITY_BONUS
        );
        assert!(
            api_identity_symbol_bonus(
                &identities,
                "New",
                "worker.New",
                "func New(client Client, taskQueue string) Worker",
                "repo://repo/worker.New",
            ) >= SCOPED_API_IDENTITY_BONUS
        );
        assert_eq!(
            api_identity_symbol_bonus(&identities, "TaskQueue", "worker.TaskQueue", "", ""),
            0.0
        );
    }

    #[test]
    fn api_identity_extraction_requires_hybrid_multi_identity_context() {
        assert!(
            hybrid_api_symbol_identities("InterruptCh", &request(CodeQueryKind::Hybrid)).is_empty()
        );
        assert!(
            hybrid_api_symbol_identities(
                "worker.New RegisterWorkflow",
                &request(CodeQueryKind::Definition),
            )
            .is_empty()
        );
    }

    fn request(kind: CodeQueryKind) -> CodeRetrievalRequest {
        let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate");
        CodeRetrievalRequest::new("query", selector, kind, 10, FreshnessPolicy::AllowStale)
            .expect("request should validate")
    }
}
