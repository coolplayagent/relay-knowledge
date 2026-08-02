//! Scores lexical query terms and exact paths against bounded result fields.

use super::super::identifiers::identifier_terms_equivalent;
use super::tokens::{identifier_field_matches_token, identifier_match_terms, score_query_tokens};

pub(in crate::storage::sqlite::code::query) struct ScoreQuery {
    tokens: Vec<String>,
}

struct ScoreField<'field> {
    original: &'field str,
    lower: Option<String>,
    identifier_terms: Option<Vec<String>>,
}

impl<'field> ScoreField<'field> {
    fn new(field: &'field str) -> Self {
        Self {
            original: field.trim(),
            lower: None,
            identifier_terms: None,
        }
    }

    fn lower(&mut self) -> &str {
        self.lower
            .get_or_insert_with(|| self.original.to_lowercase())
            .as_str()
    }

    fn matches_lower_token(&mut self, token: &str) -> bool {
        if self.original.is_ascii() && token.is_ascii() {
            self.original.eq_ignore_ascii_case(token)
        } else {
            self.lower() == token
        }
    }

    fn matches_identifier_token(&mut self, token: &str, cache_terms: bool) -> bool {
        if !cache_terms {
            return identifier_field_matches_token(self.original, token);
        }
        let terms = self
            .identifier_terms
            .get_or_insert_with(|| identifier_match_terms(self.original));
        terms
            .iter()
            .any(|term| identifier_terms_equivalent(term, token))
    }
}

impl ScoreQuery {
    pub(in crate::storage::sqlite::code::query) fn new(query: &str) -> Self {
        let tokens = score_query_tokens(query);

        Self { tokens }
    }

    pub(in crate::storage::sqlite::code::query) fn score<'field>(
        &self,
        fields: impl IntoIterator<Item = &'field str>,
    ) -> f64 {
        let mut fields = fields.into_iter().map(ScoreField::new).collect::<Vec<_>>();
        let cache_identifier_terms = self.tokens.len() > 1;
        let mut score = 0.0;
        for token in &self.tokens {
            let mut token_score = 0.0_f64;
            for field in &mut fields {
                if field.matches_lower_token(token) {
                    token_score = token_score.max(4.0);
                    break;
                } else if token_score < 2.0
                    && field.matches_identifier_token(token, cache_identifier_terms)
                {
                    token_score = token_score.max(2.0);
                } else if token_score < 0.5 && field.lower().contains(token) {
                    token_score = token_score.max(0.5);
                }
            }
            score += token_score;
        }

        score
    }
}

pub(in crate::storage::sqlite::code::query) fn score_text<'field>(
    query: &str,
    fields: impl IntoIterator<Item = &'field str>,
) -> f64 {
    ScoreQuery::new(query).score(fields)
}

pub(in crate::storage::sqlite::code::query) fn score_exact_path(query: &str, path: &str) -> f64 {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return 0.0;
    }
    let path = path.trim().to_lowercase();
    if path == query {
        return 4.0;
    }
    if path.rsplit('/').next().is_some_and(|name| name == query) {
        return 2.0;
    }

    0.0
}
