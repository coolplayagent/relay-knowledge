use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::{RetrievalHit, RetrieverSource},
    storage::{GraphSearchRequest, StorageError},
};

use crate::storage::sqlite::retrieval::{
    ScoredHit, local_model::overlap_score, parse_string_array, sort_scored_hits,
};

pub(in crate::storage::sqlite::retrieval) fn community_summary_candidates(
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
                rerank: None,
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

#[cfg(test)]
#[path = "community_tests.rs"]
mod tests;
