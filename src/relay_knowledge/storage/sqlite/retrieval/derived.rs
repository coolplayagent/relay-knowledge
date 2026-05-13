use std::collections::BTreeSet;

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

pub(super) fn semantic_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<ScoredHit>, StorageError> {
    let query_terms = token_signature(&request.query, &[], None, "")
        .into_iter()
        .collect::<BTreeSet<_>>();
    if query_terms.is_empty() {
        return Ok(Vec::new());
    }

    let result_limit = bounded_candidate_limit(request);
    let (candidate_condition, ranking_expression, candidate_values) = derived_candidate_filter(
        &query_terms,
        &[
            "lower(token_signature_json)",
            "lower(content)",
            "lower(coalesce(source_path, ''))",
            "lower(entity_labels_json)",
        ],
    );
    let sql = format!(
        "
        SELECT document_id, document_kind, evidence_id, parent_evidence_id, modality,
               source_scope, source_path, entity_labels_json, content, token_signature_json,
               model, dimension, source_hash
        FROM graph_semantic_documents
        WHERE (? IS NULL OR source_scope = ?)
          AND created_graph_version <= ?
          AND ({candidate_condition})
        ORDER BY {ranking_expression} DESC, created_graph_version DESC, document_id ASC
        LIMIT ?
        ",
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(derived_candidate_values(
            request,
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
    for (
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
    ) in rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?
    {
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
    let query_terms = token_signature(&request.query, &[], None, "")
        .into_iter()
        .collect::<BTreeSet<_>>();
    if query_terms.is_empty() {
        return Ok(Vec::new());
    }
    let (candidate_condition, ranking_expression, candidate_values) = derived_candidate_filter(
        &query_terms,
        &[
            "lower(content)",
            "lower(coalesce(source_path, ''))",
            "lower(entity_labels_json)",
        ],
    );
    let sql = format!(
        "
        SELECT document_id, document_kind, evidence_id, parent_evidence_id, modality,
               source_scope, source_path, entity_labels_json, content, vector_json, model,
               dimension, source_hash
        FROM graph_vector_documents
        WHERE (? IS NULL OR source_scope = ?)
          AND created_graph_version <= ?
          AND ({candidate_condition})
        ORDER BY {ranking_expression} DESC, created_graph_version DESC, document_id ASC
        LIMIT ?
        ",
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(derived_candidate_values(
            request,
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
    for (
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
    ) in rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?
    {
        let labels = split_labels(labels_json);
        if overlap_score(&request.query, &content, &labels, source_path.as_deref()) <= 0.0 {
            continue;
        }
        let dimension = usize::try_from(dimension).map_err(|_| {
            StorageError::InvalidInput("vector dimension must be non-negative".to_owned())
        })?;
        if dimension == 0 {
            continue;
        }
        let score = cosine_similarity(
            &hashed_vector(&request.query, &[], None, "", dimension),
            &parse_f64_array(&vector_json)?,
        );
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
                entity_labels: labels,
                content,
                entities: Vec::new(),
                graph_facts: Vec::new(),
                code_artifact,
                retriever_sources: Vec::new(),
                ranking: Vec::new(),
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

fn bounded_candidate_limit(request: &GraphSearchRequest) -> usize {
    request
        .limit
        .saturating_mul(DERIVED_RESULT_MULTIPLIER)
        .clamp(1, MAX_DERIVED_RESULT_LIMIT)
}

fn derived_candidate_filter(
    query_terms: &BTreeSet<String>,
    fields: &[&str],
) -> (String, String, Vec<Value>) {
    let terms = query_terms
        .iter()
        .take(MAX_DERIVED_QUERY_TERMS)
        .map(|term| format!("%{term}%"))
        .collect::<Vec<_>>();
    let mut values = Vec::new();

    let candidate_groups = terms
        .iter()
        .map(|pattern| {
            let clauses = fields
                .iter()
                .map(|field| {
                    values.push(Value::Text(pattern.clone()));
                    format!("{field} LIKE ?")
                })
                .collect::<Vec<_>>();
            format!("({})", clauses.join(" OR "))
        })
        .collect::<Vec<_>>();

    let mut ranking_terms = Vec::new();
    for pattern in &terms {
        for field in fields {
            values.push(Value::Text(pattern.clone()));
            ranking_terms.push(format!("CASE WHEN {field} LIKE ? THEN 1 ELSE 0 END"));
        }
    }

    (
        candidate_groups.join(" OR "),
        ranking_terms.join(" + "),
        values,
    )
}

fn derived_candidate_values(
    request: &GraphSearchRequest,
    candidate_values: Vec<Value>,
    result_limit: usize,
) -> Result<Vec<Value>, StorageError> {
    let graph_version = i64::try_from(request.graph_version.get()).map_err(|_| {
        StorageError::InvalidInput("graph version is too large for sqlite query".to_owned())
    })?;
    let result_limit = i64::try_from(result_limit).map_err(|_| {
        StorageError::InvalidInput("candidate result limit is too large".to_owned())
    })?;
    let source_scope = request
        .source_scope
        .as_ref()
        .map_or(Value::Null, |scope| Value::Text(scope.clone()));
    let mut values = Vec::with_capacity(candidate_values.len() + 4);
    values.push(source_scope.clone());
    values.push(source_scope);
    values.push(Value::Integer(graph_version));
    values.extend(candidate_values);
    values.push(Value::Integer(result_limit));

    Ok(values)
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
    fn derived_candidate_filter_caps_query_terms() {
        let query_terms = (0..40)
            .map(|index| format!("term{index}"))
            .collect::<BTreeSet<_>>();
        let fields = [
            "lower(content)",
            "lower(source_path)",
            "lower(entity_labels_json)",
        ];

        let (condition, ranking, values) = derived_candidate_filter(&query_terms, &fields);

        assert!(condition.contains("lower(content) LIKE ?"));
        assert!(ranking.contains("CASE WHEN lower(content) LIKE ?"));
        assert_eq!(values.len(), MAX_DERIVED_QUERY_TERMS * fields.len() * 2);
    }
}
