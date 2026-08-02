use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::{ContextGraphFact, RetrievalHit, RetrieverSource},
    storage::{GraphSearchRequest, StorageError},
};

use crate::storage::sqlite::retrieval::{
    ScoredHit, context::entities_for_evidence, evidence_group_key, parse_string_array,
};

#[derive(Clone)]
pub(super) struct SupportContext {
    group_id: String,
    source_scope: String,
    pub(super) source_path: Option<String>,
    pub(super) content: String,
    pub(super) entity_labels: Vec<String>,
    pub(super) evidence_ids: Vec<String>,
    modality: String,
}

impl SupportContext {
    pub(super) fn load(
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
                evidence_ids: Vec::new(),
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
            entity_labels: entities_for_evidence(connection, evidence_id)?
                .into_iter()
                .map(|entity| entity.label)
                .collect(),
            evidence_ids: vec![evidence_id.to_owned()],
            modality,
        }))
    }

    pub(super) fn scored(
        self,
        content: String,
        source: RetrieverSource,
        score: f64,
        explanation: String,
        graph_fact: Option<ContextGraphFact>,
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
                graph_facts: graph_fact.into_iter().collect(),
                code_artifact: None,
                retriever_sources: Vec::new(),
                ranking: Vec::new(),
                rerank: None,
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
        for evidence_id in other.evidence_ids {
            if !self.evidence_ids.contains(&evidence_id) {
                self.evidence_ids.push(evidence_id);
            }
        }
        for label in other.entity_labels {
            if !self.entity_labels.contains(&label) {
                self.entity_labels.push(label);
            }
        }
    }
}

#[cfg(test)]
#[path = "support_tests.rs"]
mod tests;
