use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{
    domain::{RetrievalHit, RetrieverSource},
    storage::{GraphSearchRequest, StorageError},
};

use super::{
    ScoredHit, context::code_artifact_for_document, cosine_similarity, evidence_group_key,
    hashed_vector, overlap_score, parse_f64_array, parse_string_array, semantic_overlap_score,
    sort_scored_hits, split_labels, token_signature,
};

const DERIVED_RESULT_MULTIPLIER: usize = 8;
const MAX_DERIVED_RESULT_LIMIT: usize = 512;
const MAX_DERIVED_QUERY_TERMS: usize = 16;
const VECTOR_LEXICAL_COVERAGE_WEIGHT: f64 = 0.05;
const SEMANTIC_CANDIDATE_FIELDS: &[DerivedCandidateField] = &[DerivedCandidateField::json_token(
    "lower(token_signature_json)",
)];
const VECTOR_CANDIDATE_FIELDS: &[DerivedCandidateField] = &[
    DerivedCandidateField::contains("lower(content)"),
    DerivedCandidateField::contains("lower(coalesce(source_path, ''))"),
    DerivedCandidateField::contains("lower(entity_labels_json)"),
];

pub(super) fn semantic_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<ScoredHit>, StorageError> {
    let query_terms = token_signature(&request.query, &[], None)
        .into_iter()
        .collect::<BTreeSet<_>>();
    if query_terms.is_empty() {
        return Ok(Vec::new());
    }

    let result_limit = bounded_candidate_limit(request);
    let (scope_version_condition, scope_version_values) = derived_scope_version_filter(request)?;
    let (candidate_condition, ranking_expression, candidate_values) =
        derived_candidate_filter(&query_terms, SEMANTIC_CANDIDATE_FIELDS);
    let sql = format!(
        "
        SELECT document_id, document_kind, evidence_id, parent_evidence_id, modality,
               source_scope, source_path, entity_labels_json, content, token_signature_json,
               model, dimension, source_hash
        FROM graph_semantic_documents
        WHERE {scope_version_condition}
          AND ({candidate_condition})
        ORDER BY {ranking_expression} DESC, created_graph_version DESC, document_id ASC
        LIMIT ?
        ",
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(derived_candidate_values(
            scope_version_values,
            candidate_values,
            result_limit,
        )?),
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, i64>(11)?,
                row.get::<_, String>(12)?,
            ))
        },
    )?;

    let mut hits = Vec::new();
    for row in rows {
        let (
            document_id,
            document_kind,
            evidence_id,
            parent_evidence_id,
            modality,
            source_scope,
            source_path,
            labels_json,
            content,
            signature_json,
            model,
            dimension,
            source_hash,
        ) = row.map_err(StorageError::from)?;
        let document_terms = parse_string_array(&signature_json)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        let score = semantic_overlap_score(&query_terms, &document_terms);
        if score <= 0.0 {
            continue;
        }

        let group_id = parent_evidence_id
            .as_deref()
            .unwrap_or(evidence_id.as_str());
        let key = if document_kind == "evidence" {
            evidence_group_key(group_id)
        } else {
            document_id.clone()
        };
        let code_artifact =
            code_artifact_for_document(&document_kind, &evidence_id, source_path.as_deref());
        hits.push(ScoredHit {
            key,
            hit: RetrievalHit {
                evidence_id: group_id.to_owned(),
                source_scope,
                source_path,
                source_span: None,
                entity_labels: split_labels(labels_json),
                content,
                entities: Vec::new(),
                graph_facts: Vec::new(),
                code_artifact,
                retriever_sources: Vec::new(),
                ranking: Vec::new(),
                rerank: None,
                score: 0.0,
            },
            source: RetrieverSource::Semantic,
            source_score: score,
            modality,
            explanation: Some(format!(
                "semantic read model {model} dimension={dimension} source_hash={source_hash} document={document_id}"
            )),
        });
    }
    sort_scored_hits(&mut hits);

    Ok(hits)
}

pub(super) fn vector_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<ScoredHit>, StorageError> {
    let result_limit = bounded_candidate_limit(request);
    let query_terms = token_signature(&request.query, &[], None)
        .into_iter()
        .collect::<BTreeSet<_>>();
    if query_terms.is_empty() {
        return Ok(Vec::new());
    }
    let (scope_version_condition, scope_version_values) = derived_scope_version_filter(request)?;
    let (candidate_condition, ranking_expression, candidate_values) =
        derived_candidate_filter(&query_terms, VECTOR_CANDIDATE_FIELDS);
    let sql = format!(
        "
        SELECT document_id, document_kind, evidence_id, parent_evidence_id, modality,
               source_scope, source_path, entity_labels_json, content, vector_json, model,
               dimension, source_hash
        FROM graph_vector_documents
        WHERE {scope_version_condition}
          AND ({candidate_condition})
        ORDER BY {ranking_expression} DESC, created_graph_version DESC, document_id ASC
        LIMIT ?
        ",
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(derived_candidate_values(
            scope_version_values,
            candidate_values,
            result_limit,
        )?),
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, i64>(11)?,
                row.get::<_, String>(12)?,
            ))
        },
    )?;

    let mut hits = Vec::new();
    let mut query_vectors = QueryVectorCache::new(&request.query);
    for row in rows {
        let (
            document_id,
            document_kind,
            evidence_id,
            parent_evidence_id,
            modality,
            source_scope,
            source_path,
            labels_json,
            content,
            vector_json,
            model,
            dimension,
            source_hash,
        ) = row.map_err(StorageError::from)?;
        let labels = split_labels(labels_json);
        let lexical_overlap =
            overlap_score(&request.query, &content, &labels, source_path.as_deref());
        if lexical_overlap <= 0.0 {
            continue;
        }
        let dimension = usize::try_from(dimension).map_err(|_| {
            StorageError::InvalidInput("vector dimension must be non-negative".to_owned())
        })?;
        if dimension == 0 {
            continue;
        }
        let cosine = cosine_similarity(
            query_vectors.vector(dimension),
            &parse_f64_array(&vector_json)?,
        );
        if cosine <= 0.0 {
            continue;
        }
        let score = vector_source_score(cosine, lexical_overlap, query_terms.len());

        let group_id = parent_evidence_id
            .as_deref()
            .unwrap_or(evidence_id.as_str());
        let key = if document_kind == "evidence" {
            evidence_group_key(group_id)
        } else {
            document_id.clone()
        };
        let code_artifact =
            code_artifact_for_document(&document_kind, &evidence_id, source_path.as_deref());
        hits.push(ScoredHit {
            key,
            hit: RetrievalHit {
                evidence_id: group_id.to_owned(),
                source_scope,
                source_path,
                source_span: None,
                entity_labels: labels,
                content,
                entities: Vec::new(),
                graph_facts: Vec::new(),
                code_artifact,
                retriever_sources: Vec::new(),
                ranking: Vec::new(),
                rerank: None,
                score: 0.0,
            },
            source: RetrieverSource::Vector,
            source_score: score,
            modality,
            explanation: Some(format!(
                "vector ANN read model {model} dimension={dimension} source_hash={source_hash} document={document_id}"
            )),
        });
    }
    sort_scored_hits(&mut hits);

    Ok(hits)
}

struct QueryVectorCache<'a> {
    query: &'a str,
    vectors: BTreeMap<usize, Vec<f64>>,
}

impl<'a> QueryVectorCache<'a> {
    fn new(query: &'a str) -> Self {
        Self {
            query,
            vectors: BTreeMap::new(),
        }
    }

    fn vector(&mut self, dimension: usize) -> &[f64] {
        self.vectors
            .entry(dimension)
            .or_insert_with(|| hashed_vector(self.query, &[], None, dimension))
    }
}

fn bounded_candidate_limit(request: &GraphSearchRequest) -> usize {
    request
        .limit
        .saturating_mul(DERIVED_RESULT_MULTIPLIER)
        .clamp(1, MAX_DERIVED_RESULT_LIMIT)
}

fn vector_source_score(cosine: f64, lexical_overlap: f64, query_term_count: usize) -> f64 {
    if cosine <= 0.0 {
        return 0.0;
    }
    if query_term_count == 0 {
        return cosine;
    }
    let lexical_coverage = (lexical_overlap / query_term_count as f64).clamp(0.0, 1.0);

    cosine + lexical_coverage * VECTOR_LEXICAL_COVERAGE_WEIGHT
}

#[derive(Clone, Copy)]
struct DerivedCandidateField {
    expression: &'static str,
    pattern_kind: DerivedPatternKind,
}

impl DerivedCandidateField {
    const fn contains(expression: &'static str) -> Self {
        Self {
            expression,
            pattern_kind: DerivedPatternKind::Contains,
        }
    }

    const fn json_token(expression: &'static str) -> Self {
        Self {
            expression,
            pattern_kind: DerivedPatternKind::JsonToken,
        }
    }

    fn pattern(self, term: &str) -> String {
        match self.pattern_kind {
            DerivedPatternKind::Contains => format!("%{}%", escape_like_pattern(term)),
            DerivedPatternKind::JsonToken => {
                format!("%\"{}\"%", escape_like_pattern(term))
            }
        }
    }
}

#[derive(Clone, Copy)]
enum DerivedPatternKind {
    Contains,
    JsonToken,
}

fn derived_candidate_filter(
    query_terms: &BTreeSet<String>,
    fields: &[DerivedCandidateField],
) -> (String, String, Vec<Value>) {
    let terms = query_terms
        .iter()
        .take(MAX_DERIVED_QUERY_TERMS)
        .map(String::as_str)
        .collect::<Vec<_>>();
    let mut values = Vec::new();

    let candidate_groups = terms
        .iter()
        .map(|term| {
            let clauses = fields
                .iter()
                .map(|field| {
                    values.push(Value::Text(field.pattern(term)));
                    format!("{} LIKE ? ESCAPE '\\'", field.expression)
                })
                .collect::<Vec<_>>();
            format!("({})", clauses.join(" OR "))
        })
        .collect::<Vec<_>>();

    let mut ranking_terms = Vec::new();
    for term in &terms {
        for field in fields {
            values.push(Value::Text(field.pattern(term)));
            ranking_terms.push(format!(
                "CASE WHEN {} LIKE ? ESCAPE '\\' THEN 1 ELSE 0 END",
                field.expression
            ));
        }
    }

    (
        candidate_groups.join(" OR "),
        ranking_terms.join(" + "),
        values,
    )
}

fn escape_like_pattern(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }

    escaped
}

fn derived_scope_version_filter(
    request: &GraphSearchRequest,
) -> Result<(&'static str, Vec<Value>), StorageError> {
    let graph_version = i64::try_from(request.graph_version.get()).map_err(|_| {
        StorageError::InvalidInput("graph version is too large for sqlite query".to_owned())
    })?;
    match &request.source_scope {
        Some(scope) => Ok((
            "source_scope = ? AND created_graph_version <= ?",
            vec![Value::Text(scope.clone()), Value::Integer(graph_version)],
        )),
        None => Ok((
            "created_graph_version <= ?",
            vec![Value::Integer(graph_version)],
        )),
    }
}

fn derived_candidate_values(
    mut scope_version_values: Vec<Value>,
    candidate_values: Vec<Value>,
    result_limit: usize,
) -> Result<Vec<Value>, StorageError> {
    let result_limit = i64::try_from(result_limit).map_err(|_| {
        StorageError::InvalidInput("candidate result limit is too large".to_owned())
    })?;
    scope_version_values.reserve(candidate_values.len() + 1);
    scope_version_values.extend(candidate_values);
    scope_version_values.push(Value::Integer(result_limit));

    Ok(scope_version_values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_candidate_limit_scales_with_request_limit() {
        let request = GraphSearchRequest {
            query: "semantic".to_owned(),
            source_scope: None,
            graph_version: crate::domain::GraphVersion::new(1),
            limit: 10,
            disabled_retriever_sources: Vec::new(),
        };

        assert_eq!(bounded_candidate_limit(&request), 80);
    }

    #[test]
    fn derived_scope_version_filter_uses_indexable_scope_predicate() {
        let scoped_request = GraphSearchRequest {
            query: "semantic".to_owned(),
            source_scope: Some("repo-a".to_owned()),
            graph_version: crate::domain::GraphVersion::new(7),
            limit: 10,
            disabled_retriever_sources: Vec::new(),
        };
        let unscoped_request = GraphSearchRequest {
            source_scope: None,
            ..scoped_request.clone()
        };

        let (scoped_condition, scoped_values) =
            derived_scope_version_filter(&scoped_request).expect("scoped filter should build");
        let (unscoped_condition, unscoped_values) =
            derived_scope_version_filter(&unscoped_request).expect("unscoped filter should build");

        assert_eq!(
            scoped_condition,
            "source_scope = ? AND created_graph_version <= ?"
        );
        assert_eq!(
            scoped_values,
            vec![Value::Text("repo-a".to_owned()), Value::Integer(7)]
        );
        assert_eq!(unscoped_condition, "created_graph_version <= ?");
        assert_eq!(unscoped_values, vec![Value::Integer(7)]);
    }

    #[test]
    fn derived_candidate_filter_caps_query_terms() {
        let query_terms = (0..40)
            .map(|index| format!("term{index}"))
            .collect::<BTreeSet<_>>();
        let fields = [
            DerivedCandidateField::contains("lower(content)"),
            DerivedCandidateField::contains("lower(source_path)"),
            DerivedCandidateField::contains("lower(entity_labels_json)"),
        ];

        let (condition, ranking, values) = derived_candidate_filter(&query_terms, &fields);

        assert!(condition.contains("lower(content) LIKE ? ESCAPE '\\'"));
        assert!(ranking.contains("CASE WHEN lower(content) LIKE ? ESCAPE '\\'"));
        assert_eq!(values.len(), MAX_DERIVED_QUERY_TERMS * fields.len() * 2);
    }

    #[test]
    fn derived_candidate_filter_uses_literal_patterns_for_identifiers() {
        let query_terms = BTreeSet::from(["retry_policy".to_owned()]);

        let (_, _, contains_values) = derived_candidate_filter(
            &query_terms,
            &[DerivedCandidateField::contains("lower(content)")],
        );
        let (_, _, token_values) = derived_candidate_filter(
            &query_terms,
            &[DerivedCandidateField::json_token(
                "lower(token_signature_json)",
            )],
        );

        assert_eq!(
            contains_values,
            vec![
                Value::Text("%retry\\_policy%".to_owned()),
                Value::Text("%retry\\_policy%".to_owned()),
            ]
        );
        assert_eq!(
            token_values,
            vec![
                Value::Text("%\"retry\\_policy\"%".to_owned()),
                Value::Text("%\"retry\\_policy\"%".to_owned()),
            ]
        );
    }

    #[test]
    fn query_vector_cache_reuses_vectors_by_dimension() {
        let mut cache = QueryVectorCache::new("semantic vector freshness");
        let first = cache.vector(16).to_vec();
        let second = cache.vector(16).to_vec();

        assert_eq!(first, second);
        assert_eq!(cache.vectors.len(), 1);
        assert_eq!(cache.vector(8).len(), 8);
        assert_eq!(cache.vectors.len(), 2);
    }

    #[test]
    fn vector_source_score_uses_lexical_coverage_as_bounded_tie_breaker() {
        let fuller_match = vector_source_score(0.40, 4.0, 4);
        let sparse_match = vector_source_score(0.42, 1.0, 4);
        let stronger_vector_match = vector_source_score(0.70, 1.0, 4);

        assert!(fuller_match > sparse_match);
        assert!(stronger_vector_match > fuller_match);
        assert_eq!(vector_source_score(-0.5, 4.0, 4), 0.0);
        assert_eq!(vector_source_score(0.5, 4.0, 0), 0.5);
    }

    #[test]
    fn overlap_score_matches_identifier_variants_after_fast_path_miss() {
        let labels = vec!["RuntimeBudget".to_owned()];

        assert_eq!(
            overlap_score(
                "retry_policy",
                "Retry policy controls the runtime budget",
                &labels,
                Some("src/runtime/budget.rs"),
            ),
            2.0
        );
    }
}
