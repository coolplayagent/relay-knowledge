use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

const MAX_HYBRID_API_IDENTITIES: usize = 6;
const MIN_SIMPLE_API_IDENTITY_LEN: usize = 4;
const SCOPED_API_IDENTITY_BASE_BONUS: f64 = 2.35;
const SIMPLE_API_IDENTITY_BASE_BONUS: f64 = 2.0;
const API_IDENTITY_FACET_BONUS_STEP: f64 = 0.45;
const MAX_API_IDENTITY_FACET_BONUS: f64 = 1.35;
const SHARED_SCOPED_OWNER_API_IDENTITY_BONUS: f64 = 4.1;

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

    fn owner_terms(&self) -> Option<&[String]> {
        let scoped_terms = self.scoped_terms.as_ref()?;
        let owner_len = scoped_terms.len().checked_sub(1)?;
        (owner_len > 0).then_some(&scoped_terms[..owner_len])
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
    let facet_bonus = api_identity_facet_bonus(identities.len());
    let owner_bonus =
        shared_scoped_owner_bonus(identity, identities, &[qualified_name, canonical_symbol_id]);
    if identity.is_scoped() {
        SCOPED_API_IDENTITY_BASE_BONUS + facet_bonus
    } else {
        SIMPLE_API_IDENTITY_BASE_BONUS + facet_bonus + owner_bonus
    }
}

fn api_identity_facet_bonus(identity_count: usize) -> f64 {
    (identity_count.saturating_sub(1) as f64 * API_IDENTITY_FACET_BONUS_STEP)
        .min(MAX_API_IDENTITY_FACET_BONUS)
}

fn shared_scoped_owner_bonus(
    identity: &ApiSymbolIdentity,
    identities: &[ApiSymbolIdentity],
    fields: &[&str],
) -> f64 {
    if identity.is_scoped() {
        return 0.0;
    }

    if identities
        .iter()
        .filter_map(ApiSymbolIdentity::owner_terms)
        .any(|owner_terms| {
            fields
                .iter()
                .any(|field| contains_scoped_terms(field, owner_terms))
        })
    {
        SHARED_SCOPED_OWNER_API_IDENTITY_BONUS
    } else {
        0.0
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
            ) >= SIMPLE_API_IDENTITY_BASE_BONUS
        );
        assert!(
            api_identity_symbol_bonus(
                &identities,
                "New",
                "worker.New",
                "func New(client Client, taskQueue string) Worker",
                "repo://repo/worker.New",
            ) >= SCOPED_API_IDENTITY_BASE_BONUS
        );
        assert_eq!(
            api_identity_symbol_bonus(&identities, "TaskQueue", "worker.TaskQueue", "", ""),
            0.0
        );
    }

    #[test]
    fn multi_identity_queries_give_each_api_facet_enough_direct_symbol_weight() {
        let request = request(CodeQueryKind::Hybrid);
        let identities = hybrid_api_symbol_identities(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
            &request,
        );

        let later_facets = [
            ("RegisterWorkflow", "func RegisterWorkflow(w interface{})"),
            ("RegisterActivity", "func RegisterActivity(a interface{})"),
            ("InterruptCh", "func InterruptCh() <-chan interface{}"),
        ];
        for (name, signature) in later_facets {
            assert!(
                api_identity_symbol_bonus(&identities, name, name, signature, "")
                    >= SIMPLE_API_IDENTITY_BASE_BONUS + 1.0,
                "{name} should carry enough facet weight to survive broad lexical usage chunks",
            );
        }
    }

    #[test]
    fn simple_api_facets_prefer_symbols_under_scoped_query_owner() {
        let request = request(CodeQueryKind::Hybrid);
        let identities = hybrid_api_symbol_identities(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
            &request,
        );

        let public_worker_bonus = api_identity_symbol_bonus(
            &identities,
            "InterruptCh",
            "worker.InterruptCh",
            "func InterruptCh() <-chan interface{}",
            "repo://repo/worker.InterruptCh",
        );
        let internal_bonus = api_identity_symbol_bonus(
            &identities,
            "InterruptCh",
            "internal.InterruptCh",
            "func InterruptCh() <-chan interface{}",
            "repo://repo/internal.InterruptCh",
        );

        assert!(public_worker_bonus >= internal_bonus + 4.0);
    }

    #[test]
    fn shared_owner_bonus_ignores_signature_type_mentions() {
        let request = request(CodeQueryKind::Hybrid);
        let identities = hybrid_api_symbol_identities(
            "client.Dial envconfig MustLoadDefaultClientOptions workflow client",
            &request,
        );

        let envconfig_bonus = api_identity_symbol_bonus(
            &identities,
            "MustLoadDefaultClientOptions",
            "envconfig.MustLoadDefaultClientOptions",
            "func MustLoadDefaultClientOptions() client.Options",
            "repo://repo/envconfig.MustLoadDefaultClientOptions",
        );

        assert!(envconfig_bonus < SIMPLE_API_IDENTITY_BASE_BONUS + 2.0);
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
