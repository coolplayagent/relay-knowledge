use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::{RetrievalHit, RetrieverSource},
    storage::{GraphSearchRequest, StorageError},
};

use super::{
    ScoredHit, cosine_similarity, evidence_group_key, hashed_vector, overlap_score,
    parse_f64_array, parse_string_array, semantic_overlap_score, sort_scored_hits, split_labels,
    token_signature,
};

const SEMANTIC_SCAN_MULTIPLIER: usize = 8;
const MAX_SEMANTIC_SCAN_LIMIT: usize = 512;

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

    let candidate_limit = bounded_candidate_limit(request)?;
    let mut statement = connection.prepare(
        "
        SELECT document_id, evidence_id, parent_evidence_id, modality, source_scope,
               source_path, entity_labels_json, content, token_signature_json,
               model, dimension, source_hash
        FROM graph_semantic_documents
        WHERE (?1 IS NULL OR source_scope = ?1)
          AND created_graph_version <= ?2
        ORDER BY created_graph_version DESC, document_id ASC
        LIMIT ?3
        ",
    )?;
    let rows = statement.query_map(
        params![
            request.source_scope.as_deref(),
            request.graph_version.get(),
            candidate_limit
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, i64>(10)?,
                row.get::<_, String>(11)?,
            ))
        },
    )?;

    let mut hits = Vec::new();
    for (
        document_id,
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
        hits.push(ScoredHit {
            key: evidence_group_key(group_id),
            hit: RetrievalHit {
                evidence_id: group_id.to_owned(),
                source_scope,
                source_path,
                source_span: None,
                entity_labels: split_labels(labels_json),
                content,
                entities: Vec::new(),
                graph_facts: Vec::new(),
                code_artifact: None,
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
    let candidate_limit = bounded_candidate_limit(request)?;
    let mut statement = connection.prepare(
        "
        SELECT document_id, evidence_id, parent_evidence_id, modality, source_scope,
               source_path, entity_labels_json, content, vector_json, model,
               dimension, source_hash
        FROM graph_vector_documents
        WHERE (?1 IS NULL OR source_scope = ?1)
          AND created_graph_version <= ?2
        ORDER BY created_graph_version DESC, document_id ASC
        LIMIT ?3
        ",
    )?;
    let rows = statement.query_map(
        params![
            request.source_scope.as_deref(),
            request.graph_version.get(),
            candidate_limit
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, i64>(10)?,
                row.get::<_, String>(11)?,
            ))
        },
    )?;

    let mut hits = Vec::new();
    for (
        document_id,
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
        hits.push(ScoredHit {
            key: evidence_group_key(group_id),
            hit: RetrievalHit {
                evidence_id: group_id.to_owned(),
                source_scope,
                source_path,
                source_span: None,
                entity_labels: labels,
                content,
                entities: Vec::new(),
                graph_facts: Vec::new(),
                code_artifact: None,
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

fn bounded_candidate_limit(request: &GraphSearchRequest) -> Result<i64, StorageError> {
    let scan_limit = request
        .limit
        .saturating_mul(SEMANTIC_SCAN_MULTIPLIER)
        .clamp(1, MAX_SEMANTIC_SCAN_LIMIT);
    i64::try_from(scan_limit)
        .map_err(|_| StorageError::InvalidInput("candidate scan limit is too large".to_owned()))
}

pub(super) fn path_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<ScoredHit>, StorageError> {
    let mut hits = Vec::new();
    collect_relation_paths(connection, request, &mut hits)?;
    collect_claim_paths(connection, request, &mut hits)?;
    collect_event_paths(connection, request, &mut hits)?;
    sort_scored_hits(&mut hits);

    Ok(hits)
}

fn collect_relation_paths(
    connection: &Connection,
    request: &GraphSearchRequest,
    hits: &mut Vec<ScoredHit>,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT gr.id, src.label, gr.relation_type, dst.label, gr.evidence_ids_json
        FROM graph_relations gr
        INNER JOIN entities src ON src.id = gr.source_entity_id
        INNER JOIN entities dst ON dst.id = gr.target_entity_id
        WHERE gr.status = 'accepted'
          AND gr.created_graph_version <= ?1
          AND gr.valid_from_graph_version <= ?1
          AND (gr.valid_until_graph_version IS NULL OR gr.valid_until_graph_version >= ?1)
        ORDER BY gr.created_graph_version DESC, gr.id ASC
        ",
    )?;
    let rows = statement.query_map(params![request.graph_version.get()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    for (id, source, relation_type, target, evidence_ids_json) in rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?
    {
        let Some(context) = SupportContext::load(connection, &evidence_ids_json, request)? else {
            continue;
        };
        let text = format!("{source} {relation_type} {target} {}", context.content);
        let score = overlap_score(
            &request.query,
            &text,
            &context.entity_labels,
            context.source_path.as_deref(),
        );
        if score > 0.0 {
            let content = format!(
                "{source} -[{relation_type}]-> {target}\n{}",
                context.content
            );
            hits.push(context.scored(
                content,
                RetrieverSource::GraphPath,
                score,
                format!("relation path {id} supported by scoped evidence"),
            ));
        }
    }

    Ok(())
}

fn collect_claim_paths(
    connection: &Connection,
    request: &GraphSearchRequest,
    hits: &mut Vec<ScoredHit>,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT gc.id, ent.label, gc.predicate, gc.object, gc.evidence_ids_json
        FROM graph_claims gc
        INNER JOIN entities ent ON ent.id = gc.subject_entity_id
        WHERE gc.status = 'accepted'
          AND gc.created_graph_version <= ?1
          AND gc.valid_from_graph_version <= ?1
          AND (gc.valid_until_graph_version IS NULL OR gc.valid_until_graph_version >= ?1)
        ORDER BY gc.created_graph_version DESC, gc.id ASC
        ",
    )?;
    let rows = statement.query_map(params![request.graph_version.get()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    for (id, subject, predicate, object, evidence_ids_json) in rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?
    {
        let Some(context) = SupportContext::load(connection, &evidence_ids_json, request)? else {
            continue;
        };
        let text = format!("{subject} {predicate} {object} {}", context.content);
        let score = overlap_score(
            &request.query,
            &text,
            &context.entity_labels,
            context.source_path.as_deref(),
        );
        if score > 0.0 {
            let content = format!("claim {subject} {predicate} {object}\n{}", context.content);
            hits.push(context.scored(
                content,
                RetrieverSource::GraphPath,
                score,
                format!("schema-guided claim path {id} supported by scoped evidence"),
            ));
        }
    }

    Ok(())
}

fn collect_event_paths(
    connection: &Connection,
    request: &GraphSearchRequest,
    hits: &mut Vec<ScoredHit>,
) -> Result<(), StorageError> {
    for event in load_events(connection, request)? {
        let Some(context) = SupportContext::load(connection, &event.evidence_ids_json, request)?
        else {
            continue;
        };
        let text = format!(
            "{} {} {} {}",
            event.event_type,
            event.occurred_at.as_deref().unwrap_or_default(),
            event.labels,
            context.content
        );
        let score = overlap_score(
            &request.query,
            &text,
            &context.entity_labels,
            context.source_path.as_deref(),
        );
        if score > 0.0 {
            let occurred = occurred_label(event.occurred_at.as_deref());
            let content = format!(
                "event {}{}: {}\n{}",
                event.event_type, occurred, event.labels, context.content
            );
            hits.push(context.scored(
                content,
                RetrieverSource::GraphPath,
                score,
                format!(
                    "schema-guided event path {} supported by scoped evidence",
                    event.id
                ),
            ));
        }
    }

    Ok(())
}

pub(super) fn temporal_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<ScoredHit>, StorageError> {
    let temporal = TemporalQuery::parse(&request.query);
    if !temporal.requested {
        return Ok(Vec::new());
    }

    let mut hits = Vec::new();
    for event in load_events(connection, request)? {
        if !temporal.matches(event.occurred_at.as_deref()) {
            continue;
        }
        let Some(context) = SupportContext::load(connection, &event.evidence_ids_json, request)?
        else {
            continue;
        };
        let text = format!(
            "{} {} {} {}",
            event.event_type,
            event.occurred_at.as_deref().unwrap_or_default(),
            event.labels,
            context.content
        );
        let score = 1.0
            + overlap_score(
                &request.query,
                &text,
                &context.entity_labels,
                context.source_path.as_deref(),
            );
        let occurred = occurred_label(event.occurred_at.as_deref());
        let content = format!(
            "temporal event {}{}: {}\n{}",
            event.event_type, occurred, event.labels, context.content
        );
        hits.push(context.scored(
            content,
            RetrieverSource::Temporal,
            score,
            format!("temporal event {} matched query time constraints", event.id),
        ));
    }
    sort_scored_hits(&mut hits);

    Ok(hits)
}

pub(super) fn community_summary_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<ScoredHit>, StorageError> {
    if !wants_community_summary(&request.query) {
        return Ok(Vec::new());
    }

    let mut hits = Vec::new();
    for scope in community_scopes(connection, request)? {
        let entity_labels =
            entity_labels_for_scope(connection, &scope, request.graph_version.get())?;
        let relation_count = count_scoped_facts(
            connection,
            "graph_relations",
            &scope,
            request.graph_version.get(),
        )?;
        let claim_count = count_scoped_facts(
            connection,
            "graph_claims",
            &scope,
            request.graph_version.get(),
        )?;
        let event_count = count_scoped_facts(
            connection,
            "graph_events",
            &scope,
            request.graph_version.get(),
        )?;
        let content = format!(
            "community summary for {scope}: entities {}; relations {relation_count}; claims {claim_count}; events {event_count}",
            entity_labels.join(", ")
        );
        let score = 1.0 + overlap_score(&request.query, &content, &entity_labels, None);
        hits.push(ScoredHit {
            key: format!("community:{scope}:{}", request.graph_version.get()),
            hit: RetrievalHit {
                evidence_id: format!("community:{scope}:{}", request.graph_version.get()),
                source_scope: scope,
                source_path: None,
                source_span: None,
                content,
                entity_labels,
                entities: Vec::new(),
                graph_facts: Vec::new(),
                code_artifact: None,
                retriever_sources: Vec::new(),
                ranking: Vec::new(),
                score: 0.0,
            },
            source: RetrieverSource::CommunitySummary,
            source_score: score,
            modality: "text_span".to_owned(),
            explanation: None,
        });
    }
    sort_scored_hits(&mut hits);

    Ok(hits)
}

struct EventRow {
    id: String,
    event_type: String,
    occurred_at: Option<String>,
    evidence_ids_json: String,
    labels: String,
}

fn load_events(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<EventRow>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT ge.id, ge.event_type, ge.occurred_at, ge.evidence_ids_json,
               group_concat(ent.label, ' ')
        FROM graph_events ge
        INNER JOIN graph_event_entities gee ON gee.event_id = ge.id
        INNER JOIN entities ent ON ent.id = gee.entity_id
        WHERE ge.status = 'accepted'
          AND ge.created_graph_version <= ?1
          AND ge.valid_from_graph_version <= ?1
          AND (ge.valid_until_graph_version IS NULL OR ge.valid_until_graph_version >= ?1)
        GROUP BY ge.id, ge.event_type, ge.occurred_at, ge.evidence_ids_json
        ORDER BY ge.occurred_at DESC, ge.id ASC
        ",
    )?;
    let rows = statement.query_map(params![request.graph_version.get()], |row| {
        Ok(EventRow {
            id: row.get(0)?,
            event_type: row.get(1)?,
            occurred_at: row.get(2)?,
            evidence_ids_json: row.get(3)?,
            labels: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

#[derive(Clone)]
struct SupportContext {
    group_id: String,
    source_scope: String,
    source_path: Option<String>,
    content: String,
    entity_labels: Vec<String>,
    modality: String,
}

impl SupportContext {
    fn load(
        connection: &Connection,
        evidence_ids_json: &str,
        request: &GraphSearchRequest,
    ) -> Result<Option<Self>, StorageError> {
        let evidence_ids = parse_string_array(evidence_ids_json)?;
        if evidence_ids.is_empty() {
            return Ok(request.source_scope.is_none().then(|| Self {
                group_id: format!("graph:{}", request.graph_version.get()),
                source_scope: "graph".to_owned(),
                source_path: None,
                content: String::new(),
                entity_labels: Vec::new(),
                modality: "text_span".to_owned(),
            }));
        }

        let mut combined: Option<Self> = None;
        for evidence_id in evidence_ids {
            if let Some(context) = Self::load_one(connection, &evidence_id, request)? {
                match &mut combined {
                    Some(existing) => existing.merge(context),
                    None => combined = Some(context),
                }
            }
        }

        Ok(combined)
    }

    fn load_one(
        connection: &Connection,
        evidence_id: &str,
        request: &GraphSearchRequest,
    ) -> Result<Option<Self>, StorageError> {
        let row = connection
            .query_row(
                "
                SELECT id, parent_evidence_id, modality, source_scope, source_path, content
                FROM evidence
                WHERE id = ?1
                  AND (?2 IS NULL OR source_scope = ?2)
                  AND created_graph_version <= ?3
                  AND status IN ('accepted', 'proposed')
                ",
                params![
                    evidence_id,
                    request.source_scope.as_deref(),
                    request.graph_version.get()
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, parent, modality, source_scope, source_path, content)) = row else {
            return Ok(None);
        };

        Ok(Some(Self {
            group_id: parent.unwrap_or(id),
            source_scope,
            source_path,
            content,
            entity_labels: super::entities_for_evidence(connection, evidence_id)?
                .into_iter()
                .map(|entity| entity.label)
                .collect(),
            modality,
        }))
    }

    fn scored(
        self,
        content: String,
        source: RetrieverSource,
        score: f64,
        explanation: String,
    ) -> ScoredHit {
        ScoredHit {
            key: evidence_group_key(&self.group_id),
            hit: RetrievalHit {
                evidence_id: self.group_id,
                source_scope: self.source_scope,
                source_path: self.source_path,
                source_span: None,
                content,
                entity_labels: self.entity_labels,
                entities: Vec::new(),
                graph_facts: Vec::new(),
                code_artifact: None,
                retriever_sources: Vec::new(),
                ranking: Vec::new(),
                score: 0.0,
            },
            source,
            source_score: score,
            modality: self.modality,
            explanation: Some(explanation),
        }
    }

    fn merge(&mut self, other: Self) {
        if !other.content.is_empty() && !self.content.contains(&other.content) {
            if !self.content.is_empty() {
                self.content.push_str("\n\n");
            }
            self.content.push_str(&other.content);
        }
        if self.source_path.is_none() {
            self.source_path = other.source_path;
        }
        for label in other.entity_labels {
            if !self.entity_labels.contains(&label) {
                self.entity_labels.push(label);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TemporalQuery {
    requested: bool,
    as_of: Option<String>,
    as_of_date: Option<TemporalDate>,
    time_terms: Vec<String>,
}

impl TemporalQuery {
    fn parse(query: &str) -> Self {
        let lowered = query.to_ascii_lowercase();
        let scrubbed_query = query
            .split_whitespace()
            .filter(|token| strip_as_of_value(token).is_none())
            .collect::<Vec<_>>()
            .join(" ");
        let time_terms = token_signature(&scrubbed_query, &[], None, "")
            .into_iter()
            .filter(|term| term.len() == 4 && term.chars().all(|ch| ch.is_ascii_digit()))
            .collect::<Vec<_>>();
        let as_of = extract_as_of(query);
        let as_of_date = as_of.as_deref().and_then(TemporalDate::parse);
        let requested = as_of.is_some()
            || !time_terms.is_empty()
            || ["when", "timeline", "history", "temporal"]
                .iter()
                .any(|needle| lowered.contains(needle));

        Self {
            requested,
            as_of,
            as_of_date,
            time_terms,
        }
    }

    fn matches(&self, occurred_at: Option<&str>) -> bool {
        let Some(occurred_at) = occurred_at else {
            return false;
        };
        if self.time_terms.is_empty() && self.as_of.is_none() {
            return true;
        }
        if let Some(as_of) = self.as_of_date {
            let Some(occurred) = TemporalDate::parse(occurred_at) else {
                return false;
            };
            if !occurred.is_on_or_before(as_of) {
                return false;
            }
            return self.time_terms.is_empty()
                || self
                    .time_terms
                    .iter()
                    .any(|term| occurred_at.contains(term));
        }

        self.time_terms
            .iter()
            .any(|term| occurred_at.contains(term))
    }
}

fn extract_as_of(query: &str) -> Option<String> {
    query.split_whitespace().find_map(|token| {
        strip_as_of_value(token)
            .map(|value| {
                value
                    .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
                    .to_owned()
            })
            .filter(|value| !value.is_empty())
    })
}

fn strip_as_of_value(token: &str) -> Option<&str> {
    let lowered = token.to_ascii_lowercase();
    ["as_of:", "as-of:"]
        .iter()
        .find_map(|prefix| lowered.starts_with(prefix).then(|| &token[prefix.len()..]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TemporalDate {
    year: u16,
    month: Option<u8>,
    day: Option<u8>,
}

impl TemporalDate {
    fn parse(value: &str) -> Option<Self> {
        value.split_whitespace().find_map(|token| {
            let token = token
                .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '/');
            let token = token
                .split(|ch: char| !ch.is_ascii_digit() && ch != '-' && ch != '/')
                .next()
                .unwrap_or_default();
            let separator = if token.contains('-') { '-' } else { '/' };
            let parts = token.split(separator).collect::<Vec<_>>();
            let year = parts.first().copied()?;
            if year.len() != 4 || !year.chars().all(|ch| ch.is_ascii_digit()) {
                return None;
            }
            let year = year.parse::<u16>().ok()?;
            let month = match parts.get(1).copied() {
                Some(value) => Some(parse_date_component(value)?),
                None => None,
            };
            let day = match parts.get(2).copied() {
                Some(value) => Some(parse_date_component(value)?),
                None => None,
            };
            if parts.len() > 3
                || month.is_some_and(|value| !(1..=12).contains(&value))
                || day.is_some_and(|value| !(1..=31).contains(&value))
            {
                return None;
            }

            Some(Self { year, month, day })
        })
    }

    fn is_on_or_before(self, cutoff: Self) -> bool {
        self.lower_bound() <= cutoff.upper_bound()
    }

    fn lower_bound(self) -> (u16, u8, u8) {
        (self.year, self.month.unwrap_or(1), self.day.unwrap_or(1))
    }

    fn upper_bound(self) -> (u16, u8, u8) {
        (self.year, self.month.unwrap_or(12), self.day.unwrap_or(31))
    }
}

fn parse_date_component(value: &str) -> Option<u8> {
    (!value.is_empty() && value.len() <= 2 && value.chars().all(|ch| ch.is_ascii_digit()))
        .then(|| value.parse::<u8>().ok())
        .flatten()
}

fn community_scopes(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<String>, StorageError> {
    if let Some(scope) = &request.source_scope {
        return Ok(vec![scope.clone()]);
    }
    let mut statement = connection.prepare(
        "
        SELECT DISTINCT source_scope
        FROM evidence
        WHERE created_graph_version <= ?1
          AND status IN ('accepted', 'proposed')
        ORDER BY source_scope ASC
        ",
    )?;
    let rows = statement.query_map(params![request.graph_version.get()], |row| row.get(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn entity_labels_for_scope(
    connection: &Connection,
    source_scope: &str,
    graph_version: u64,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT DISTINCT ent.label
        FROM evidence e
        INNER JOIN evidence_entities ee ON ee.evidence_id = e.id
        INNER JOIN entities ent ON ent.id = ee.entity_id
        WHERE e.source_scope = ?1
          AND e.created_graph_version <= ?2
          AND e.status IN ('accepted', 'proposed')
        ORDER BY ent.label ASC
        LIMIT 12
        ",
    )?;
    let rows = statement.query_map(params![source_scope, graph_version], |row| row.get(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn count_scoped_facts(
    connection: &Connection,
    table: &'static str,
    source_scope: &str,
    graph_version: u64,
) -> Result<usize, StorageError> {
    let table = match table {
        "graph_relations" | "graph_claims" | "graph_events" => table,
        _ => {
            return Err(StorageError::InvalidInput(
                "unsupported fact table".to_owned(),
            ));
        }
    };
    let mut statement = connection.prepare(&format!(
        "SELECT evidence_ids_json
         FROM {table}
         WHERE status = 'accepted'
           AND created_graph_version <= ?1
           AND valid_from_graph_version <= ?1
           AND (valid_until_graph_version IS NULL OR valid_until_graph_version >= ?1)"
    ))?;
    let rows = statement.query_map(params![graph_version], |row| row.get::<_, String>(0))?;
    let mut count = 0usize;
    for evidence_ids_json in rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?
    {
        let evidence_ids = parse_string_array(&evidence_ids_json)?;
        for evidence_id in evidence_ids {
            if evidence_scope_at(connection, &evidence_id, graph_version)?.as_deref()
                == Some(source_scope)
            {
                count += 1;
                break;
            }
        }
    }

    Ok(count)
}

fn evidence_scope_at(
    connection: &Connection,
    evidence_id: &str,
    graph_version: u64,
) -> Result<Option<String>, StorageError> {
    connection
        .query_row(
            "
            SELECT source_scope
            FROM evidence
            WHERE id = ?1
              AND created_graph_version <= ?2
              AND status IN ('accepted', 'proposed')
            ",
            params![evidence_id, graph_version],
            |row| row.get(0),
        )
        .optional()
        .map_err(StorageError::from)
}

fn wants_community_summary(query: &str) -> bool {
    let lowered = query.to_ascii_lowercase();
    ["summary", "overview", "community", "global", "map"]
        .iter()
        .any(|needle| lowered.contains(needle))
}

fn occurred_label(occurred_at: Option<&str>) -> String {
    occurred_at
        .map(|value| format!(" at {value}"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporal_query_extracts_as_of_and_years() {
        let temporal = TemporalQuery::parse("as_of:2026-05-13 timeline Rust 2024");

        assert!(temporal.requested);
        assert_eq!(temporal.as_of.as_deref(), Some("2026-05-13"));
        assert!(temporal.matches(Some("2024-01-01")));
        assert!(!temporal.time_terms.contains(&"2026".to_owned()));
    }

    #[test]
    fn temporal_query_matches_keyword_only_timelines() {
        let temporal = TemporalQuery::parse("timeline of Rust releases");

        assert!(temporal.requested);
        assert!(temporal.matches(Some("2026-05-13")));
        assert!(!temporal.matches(None));
    }

    #[test]
    fn temporal_query_applies_case_insensitive_as_of_dates_strictly() {
        let temporal = TemporalQuery::parse("AS_OF:2026-10-01 timeline");

        assert_eq!(temporal.as_of.as_deref(), Some("2026-10-01"));
        assert!(temporal.matches(Some("2026-2-01")));
        assert!(temporal.matches(Some("2026-10-01T23:59:59Z")));
        assert!(!temporal.matches(Some("2026-11-01")));
        assert!(!temporal.matches(Some("undated event")));
    }

    #[test]
    fn bounded_candidate_limit_scales_with_request_limit() {
        let request = GraphSearchRequest {
            query: "semantic".to_owned(),
            source_scope: None,
            graph_version: crate::domain::GraphVersion::new(1),
            limit: 10,
        };

        assert_eq!(bounded_candidate_limit(&request).unwrap(), 80);
    }
}
