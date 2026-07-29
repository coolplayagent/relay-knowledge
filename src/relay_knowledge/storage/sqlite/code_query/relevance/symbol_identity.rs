//! Parses scoped symbol identities and matches them against indexed symbol surfaces.

use super::tokens::{
    escape_sql_like, identity_terms, simple_identity_token, token_has_path_like_extension,
};

pub(in crate::storage::sqlite::code::code_query) struct SymbolIdentityQuery {
    leaf_name: String,
    scoped_terms: Option<Vec<String>>,
}

impl SymbolIdentityQuery {
    pub(in crate::storage::sqlite::code::code_query) fn from_query(query: &str) -> Option<Self> {
        for raw_token in query.split_whitespace().map(str::trim) {
            if raw_token.contains('/')
                || raw_token.contains('\\')
                || token_has_path_like_extension(raw_token)
            {
                continue;
            }
            if raw_token.contains("::") || raw_token.contains('.') {
                let terms = identity_terms(raw_token);
                if terms.len() >= 2 {
                    return Some(Self {
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
        }

        let mut tokens = query.split_whitespace().map(str::trim);
        let token = tokens.next()?;
        if tokens.next().is_some() || !simple_identity_token(token) {
            return None;
        }

        Some(Self {
            leaf_name: token.to_owned(),
            scoped_terms: None,
        })
    }

    pub(in crate::storage::sqlite::code::code_query) fn leaf_name(&self) -> &str {
        &self.leaf_name
    }

    pub(in crate::storage::sqlite::code::code_query) fn is_scoped(&self) -> bool {
        self.scoped_terms.is_some()
    }

    pub(in crate::storage::sqlite::code::code_query) fn scoped_like_pattern(
        &self,
    ) -> Option<String> {
        let scoped_terms = self.scoped_terms.as_ref()?;
        let mut pattern = String::from("%");
        for term in scoped_terms {
            pattern.push_str(&escape_sql_like(term));
            pattern.push('%');
        }

        Some(pattern)
    }

    pub(in crate::storage::sqlite::code::code_query) fn matches_symbol(
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

pub(in crate::storage::sqlite::code::code_query) fn query_is_single_symbol_identity(
    query: &str,
) -> bool {
    let mut tokens = query.split_whitespace();
    let Some(token) = tokens.next() else {
        return false;
    };

    tokens.next().is_none() && SymbolIdentityQuery::from_query(token).is_some()
}

pub(in crate::storage::sqlite::code::code_query) fn scoped_query_terms(
    query: &str,
) -> Option<Vec<String>> {
    let scoped_token = query
        .split_whitespace()
        .find(|token| token.contains("::") || token.contains('.'))?;
    let terms = scoped_terms(scoped_token);
    (terms.len() >= 2).then_some(terms)
}

pub(in crate::storage::sqlite::code::code_query) fn contains_scoped_terms(
    field: &str,
    query_terms: &[String],
) -> bool {
    if query_terms.is_empty() {
        return false;
    }
    let field_terms = scoped_terms(field);
    field_terms
        .windows(query_terms.len())
        .any(|window| window == query_terms)
        || contains_constructor_nested_scoped_terms(&field_terms, query_terms)
}

fn contains_constructor_nested_scoped_terms(
    field_terms: &[String],
    query_terms: &[String],
) -> bool {
    if query_terms.len() != 2 {
        return false;
    }
    for start in 0..field_terms.len().saturating_sub(2) {
        if field_terms[start] != query_terms[0] || field_terms[start + 1] != query_terms[0] {
            continue;
        }
        let tail = &field_terms[start + 2..];
        let Some(leaf_index) = tail.iter().position(|term| term == &query_terms[1]) else {
            continue;
        };
        if tail[..leaf_index]
            .iter()
            .all(|term| matches!(term.as_str(), "constructor" | "init" | "new"))
        {
            return true;
        }
    }

    false
}

fn scoped_terms(value: &str) -> Vec<String> {
    value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}
