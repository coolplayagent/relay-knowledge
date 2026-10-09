//! Separates repository navigation, captured evidence, authored facts and retrieval readiness.
use super::{
    SourceAuditReport,
    bundle::{AuthoredBundleAudit, document_id, load_bundle},
    reader,
    requirements::{RequirementAudit, audit_requirements},
};
use crate::{
    api::{ApiError, RequestContext},
    application::{KnowledgeMapService, RelayKnowledgeService},
    domain::{
        CodeContentIntegrityState, GraphVersion, IndexKind, IndexState, IndexStatus,
        research::{ResearchDelivery, ResearchRepositoryState},
    },
};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ResearchStatusRequest {
    pub root: PathBuf,
    pub delivery: ResearchDelivery,
    pub catalog: Option<PathBuf>,
    pub bundle: Option<PathBuf>,
    pub scope: Option<String>,
    pub requirements: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct ResearchMapState {
    pub valid: bool,
    pub version: Option<u64>,
    pub route_count: usize,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ResearchStatusResponse {
    pub root: String,
    pub delivery: ResearchDelivery,
    pub map: ResearchMapState,
    pub sources: Option<SourceAuditReport>,
    pub sources_error: Option<String>,
    pub bundle: Option<AuthoredBundleAudit>,
    pub bundle_error: Option<String>,
    pub repository_index: ResearchRepositoryState,
    pub graph_version: GraphVersion,
    pub graph_indexes: Vec<IndexStatus>,
    pub requirements: Option<RequirementAudit>,
    pub requirements_error: Option<String>,
    pub readiness: String,
    pub content_verdict: String,
    pub next_steps: Vec<String>,
}

impl RelayKnowledgeService {
    pub async fn research_status(
        &self,
        request: ResearchStatusRequest,
        context: RequestContext,
    ) -> Result<ResearchStatusResponse, ApiError> {
        let root = request.root.clone();
        let canonical = reader::compute(move |_| {
            std::fs::canonicalize(root)
                .map_err(|error| ApiError::invalid_argument(error.to_string()))
        })
        .await?;
        let map_service = KnowledgeMapService::new(canonical.clone());
        let validation = map_service
            .validate(&context)
            .await
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let view = map_service.show_filtered(&context, None, None).await.ok();
        let map = ResearchMapState {
            valid: validation.valid,
            version: view.as_ref().map(|view| view.map.map_version),
            route_count: view.as_ref().map_or(0, |view| view.map.routes.len()),
            diagnostics: validation.diagnostics,
        };
        let (sources, bundle, requirements) = reader::run(canonical.clone(), move |reader| {
            let sources = request
                .catalog
                .map(|path| super::audit::audit_catalog(reader, &path))
                .transpose();
            let bundle = request
                .bundle
                .map(|path| {
                    let scope = request.scope.as_ref().ok_or_else(|| {
                        ApiError::invalid_argument("bundle status requires --scope")
                    })?;
                    load_bundle(reader, &path, scope).map(|loaded| loaded.report)
                })
                .transpose();
            let requirements = request
                .requirements
                .map(|path| audit_requirements(reader, &path))
                .transpose();
            Ok((sources, bundle, requirements))
        })
        .await?;
        let repository_index = match self
            .research_repository_state(canonical.to_string_lossy().into_owned())
            .await
        {
            Ok(state) => state,
            Err(error) => ResearchRepositoryState {
                state: "unknown".into(),
                requested_ref: "HEAD".into(),
                resolved_target: None,
                registration: None,
                served_scope: None,
                diagnostic: Some(error.message),
            },
        };
        let store = self
            .store()
            .await
            .map_err(|error| ApiError::storage_unavailable(error.to_string()))?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(|error| ApiError::storage_unavailable(error.to_string()))?;
        let graph_indexes = store
            .index_statuses()
            .await
            .map_err(|error| ApiError::storage_unavailable(error.to_string()))?;
        let sources_error = sources.as_ref().err().map(|error| error.message.clone());
        let bundle_error = bundle.as_ref().err().map(|error| error.message.clone());
        let requirements_error = requirements
            .as_ref()
            .err()
            .map(|error| error.message.clone());
        let mut bundle = bundle.ok().flatten();
        if let Some(bundle) = &mut bundle {
            let stored = store
                .evidence_document(
                    document_id(
                        &bundle.bundle_id,
                        &bundle.source_scope,
                        &bundle.bundle_sha256,
                    ),
                    bundle.source_scope.clone(),
                )
                .await
                .map_err(|error| ApiError::storage_unavailable(error.to_string()))?;
            bundle.stored_fact_status = stored.as_ref().map(|record| record.status);
            bundle.import_state = match stored {
                None => "not_imported",
                Some(record) => {
                    let expected = bundle.bundle_sha256.clone();
                    if reader::compute(move |_| {
                        Ok(reader::digest(record.content.as_bytes()) == expected)
                    })
                    .await?
                    {
                        "imported"
                    } else {
                        "stored_digest_mismatch"
                    }
                }
            }
            .into();
        }
        let mut response = ResearchStatusResponse {
            root: canonical.to_string_lossy().into_owned(),
            delivery: request.delivery,
            map,
            sources: sources.ok().flatten(),
            sources_error,
            bundle,
            bundle_error,
            repository_index,
            graph_version,
            graph_indexes,
            requirements: requirements.ok().flatten(),
            requirements_error,
            readiness: "unknown".into(),
            content_verdict: "unknown".into(),
            next_steps: Vec::new(),
        };
        evaluate_readiness(&mut response);
        Ok(response)
    }
}

fn evaluate_readiness(response: &mut ResearchStatusResponse) {
    if !response.map.valid {
        response.next_steps.push("Initialize or repair the navigation map; map validity does not prove capture completeness.".into());
    }
    if response.sources_error.is_some()
        || response
            .sources
            .as_ref()
            .is_some_and(|report| !report.integrity_valid)
    {
        response
            .next_steps
            .push("Repair source catalog diagnostics while preserving original bytes.".into());
    }
    if response.bundle_error.is_some()
        || response.bundle.as_ref().is_some_and(|report| !report.valid)
    {
        response
            .next_steps
            .push("Repair authored bundle scope, endpoints and evidence bindings.".into());
    }
    let ready = match response.delivery {
        ResearchDelivery::Archive => response
            .sources
            .as_ref()
            .is_some_and(|report| report.integrity_valid),
        ResearchDelivery::AuthoredGraph => {
            response.bundle.as_ref().is_some_and(|report| report.valid)
        }
        ResearchDelivery::Graphrag => {
            if response.bundle_error.is_some() {
                false
            } else if let Some(bundle) = &response.bundle {
                let indexes_fresh = IndexKind::ALL.iter().all(|kind| {
                    response.graph_indexes.iter().any(|index| {
                        index.kind == *kind
                            && index.state == IndexState::Fresh
                            && index.indexed_graph_version == response.graph_version
                    })
                });
                let ready = bundle.valid
                    && bundle.import_state == "imported"
                    && matches!(
                        bundle.stored_fact_status,
                        Some(
                            crate::domain::FactStatus::Proposed
                                | crate::domain::FactStatus::Accepted
                        )
                    )
                    && indexes_fresh;
                if !ready {
                    response.next_steps.push("Import the verified bundle as proposed facts and refresh its derived graph indexes.".into());
                }
                ready
            } else {
                let ready = response.repository_index.state == "fresh"
                    && response
                        .repository_index
                        .served_scope
                        .as_ref()
                        .is_some_and(|scope| {
                            scope.content_integrity.state == CodeContentIntegrityState::Complete
                        });
                if !ready {
                    response.next_steps.push("Register/index this repository for the requested HEAD target, then resolve stale or partial content diagnostics.".into());
                }
                ready
            }
        }
    };
    response.readiness = if ready {
        "ready_for_review"
    } else {
        "needs_action"
    }
    .into();
    if response.delivery == ResearchDelivery::Archive && response.sources.is_none() {
        response.next_steps.push(
            "Provide an explicit capture catalog; navigation alone is not an archive audit.".into(),
        );
    }
    if response.delivery == ResearchDelivery::AuthoredGraph && response.bundle.is_none() {
        response.next_steps.push("Provide an authored evidence bundle; runtime import is optional for a versioned research deliverable.".into());
    }
    if response.requirements_error.is_some()
        || response.requirements.as_ref().is_some_and(|audit| {
            audit
                .requirements
                .iter()
                .any(|item| item.evidence_integrity != "verified")
        })
    {
        response.readiness = "needs_action".into();
        response.next_steps.push(
            "Repair the requirement evidence bindings before requesting content review.".into(),
        );
    }
    response.next_steps.push("Content completion remains unknown until a source-bound review establishes the stated requirements; hashes and freshness alone cannot prove it.".into());
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
