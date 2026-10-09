//! Ordered, bounded map transactions; publication belongs to the application layer.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{
    DomainError, KnowledgeMap, KnowledgeMapChange, KnowledgeMapSource, KnowledgeMapSourceKind,
};

pub const MAX_BATCH_OPERATIONS: usize = 100;
pub const MAX_BATCH_BYTES: usize = 256 * 1024;

/// Preconditions are populated by planning and mandatory for application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapBatchRequest {
    pub schema_version: u16,
    pub transaction_id: String,
    pub expected_map_version: Option<u64>,
    pub expected_digest: Option<String>,
    pub operations: Vec<MapSourceOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum MapSourceOperation {
    Add {
        id: String,
        topic: String,
        kind: KnowledgeMapSourceKind,
        uri: String,
        source_scope: Option<String>,
        description: Option<String>,
    },
    Update {
        change: KnowledgeMapChange,
    },
    Remove {
        id: String,
    },
}

/// Each step exposes normalized states rather than textual replacements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MapSourceDifference {
    pub operation_index: usize,
    pub before: Option<KnowledgeMapSource>,
    pub after: Option<KnowledgeMapSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MapBatchDiagnostic {
    pub operation_index: Option<usize>,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MapBatchPreview {
    pub differences: Vec<MapSourceDifference>,
    pub affected_routes: Vec<String>,
    pub diagnostics: Vec<MapBatchDiagnostic>,
}

impl MapBatchRequest {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != 1 {
            return Err(DomainError::invalid("schema_version", "must be 1"));
        }
        if self.transaction_id.is_empty()
            || self.transaction_id.len() > 128
            || !self
                .transaction_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
        {
            return Err(DomainError::invalid(
                "transaction_id",
                "must be 1..128 ASCII letters, digits, or -_.:",
            ));
        }
        if self.operations.is_empty() || self.operations.len() > MAX_BATCH_OPERATIONS {
            return Err(DomainError::invalid(
                "operations",
                "must contain 1..100 ordered operations",
            ));
        }
        if self.expected_map_version == Some(0)
            || self.expected_digest.as_ref().is_some_and(|digest| {
                digest.len() != 64
                    || !digest
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        {
            return Err(DomainError::invalid(
                "preconditions",
                "expected version must be positive and digest must be lowercase SHA-256",
            ));
        }
        let bytes = serde_json::to_vec(self)
            .map_err(|error| DomainError::invalid("input", error.to_string()))?;
        if bytes.len() > MAX_BATCH_BYTES {
            return Err(DomainError::invalid("input", "transaction exceeds 256 KiB"));
        }
        Ok(())
    }

    /// Uses private candidate state, so failure cannot leak a successful prefix.
    pub(crate) fn preview(
        &self,
        map: &KnowledgeMap,
        omitted_through: u64,
    ) -> (KnowledgeMap, MapBatchPreview) {
        let mut candidate = map.clone();
        let mut preview = MapBatchPreview {
            differences: Vec::new(),
            affected_routes: Vec::new(),
            diagnostics: Vec::new(),
        };
        let mut routes = BTreeSet::new();
        for (index, operation) in self.operations.iter().enumerate() {
            let id = match operation {
                MapSourceOperation::Add { id, .. } | MapSourceOperation::Remove { id } => id,
                MapSourceOperation::Update { change } => &change.id,
            };
            let before = candidate
                .sources
                .iter()
                .find(|source| source.id == id.trim())
                .cloned();
            let result = apply_operation(&mut candidate, operation.clone(), omitted_through)
                .and_then(|()| candidate.validate_reserved_repository_routes());
            if let Err(error) = result {
                preview.diagnostics.push(MapBatchDiagnostic {
                    operation_index: Some(index),
                    code: "invalid_operation".into(),
                    message: error.to_string(),
                });
                // Later operations depend on this state; do not fabricate their results.
                return (map.clone(), preview);
            }
            let after = candidate
                .sources
                .iter()
                .find(|source| source.id == id.trim())
                .cloned();
            for source in before.iter().chain(after.iter()) {
                routes.insert(source.topic.clone());
            }
            preview.differences.push(MapSourceDifference {
                operation_index: index,
                before,
                after,
            });
        }
        preview.affected_routes = routes.into_iter().collect();
        (candidate, preview)
    }
}

fn apply_operation(
    map: &mut KnowledgeMap,
    operation: MapSourceOperation,
    omitted: u64,
) -> Result<(), DomainError> {
    match operation {
        MapSourceOperation::Add {
            id,
            topic,
            kind,
            uri,
            source_scope,
            description,
        } => {
            validate_local_uri(kind, &uri)?;
            map.add_source_snapshot(
                KnowledgeMapSource::new(id, topic, kind, uri, source_scope, description)?,
                omitted,
            )
        }
        MapSourceOperation::Update { change } => {
            if let Some(source) = map.sources.iter().find(|source| source.id == change.id) {
                validate_local_uri(
                    change.kind.unwrap_or(source.kind),
                    change.uri.as_deref().unwrap_or(&source.uri),
                )?;
            }
            map.update_source_snapshot(change, omitted).map(|_| ())
        }
        MapSourceOperation::Remove { id } => map.remove_source_snapshot(&id, omitted),
    }
}

fn validate_local_uri(kind: KnowledgeMapSourceKind, uri: &str) -> Result<(), DomainError> {
    let uri = uri.trim();
    if matches!(
        kind,
        KnowledgeMapSourceKind::File | KnowledgeMapSourceKind::Config
    ) && (uri.starts_with('/')
        || uri.contains('\\')
        || uri.contains(':')
        || uri.split('/').any(|part| part == ".."))
    {
        return Err(DomainError::invalid(
            "uri",
            "local file/config URI must be repository-relative without parent traversal",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "map_batch_tests.rs"]
mod tests;
