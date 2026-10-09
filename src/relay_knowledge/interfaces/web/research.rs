//! Typed research adapters on the existing same-origin, bounded Web operation route.
use super::WebError;
use crate::{
    api::{ApiMetadata, RequestContext},
    application::{
        KnowledgeMapService, RelayKnowledgeService,
        research::{ResearchRootSelection, ResearchService, ResearchStatusRequest},
    },
    domain::{GraphVersion, map_batch::MapBatchRequest, research::ResearchDelivery},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", deny_unknown_fields)]
enum ResearchOperation {
    #[serde(rename = "sources.audit")]
    SourcesAudit {
        target: ResearchRootSelection,
        input: PathBuf,
    },
    #[serde(rename = "evidence.validate")]
    Validate {
        target: ResearchRootSelection,
        input: PathBuf,
        source_scope: String,
    },
    #[serde(rename = "evidence.import")]
    Import {
        target: ResearchRootSelection,
        input: PathBuf,
        source_scope: String,
    },
    #[serde(rename = "evidence.export")]
    Export {
        id: String,
        source_scope: String,
        revision: String,
    },
    #[serde(rename = "evidence.view")]
    View {
        target: ResearchRootSelection,
        input: PathBuf,
        source_scope: String,
        focus: Option<String>,
    },
    #[serde(rename = "evidence.impact")]
    Impact {
        target: ResearchRootSelection,
        input: PathBuf,
        source_scope: String,
        node: String,
        label: String,
    },
    #[serde(rename = "research.status")]
    Status {
        target: ResearchRootSelection,
        delivery: ResearchDelivery,
        catalog: Option<PathBuf>,
        bundle: Option<PathBuf>,
        source_scope: Option<String>,
        requirements: Option<PathBuf>,
    },
    #[serde(rename = "knowledge.map.plan")]
    MapPlan {
        target: ResearchRootSelection,
        transaction: MapBatchRequest,
    },
    #[serde(rename = "knowledge.map.apply")]
    MapApply {
        target: ResearchRootSelection,
        transaction: MapBatchRequest,
    },
}

pub(super) async fn dispatch(
    service: &RelayKnowledgeService,
    payload: &Value,
    context: RequestContext,
) -> Result<(ApiMetadata, Value), WebError> {
    let operation: ResearchOperation = serde_json::from_value(payload.clone())
        .map_err(|error| WebError::bad_request(format!("invalid research operation: {error}")))?;
    let response = match operation {
        ResearchOperation::SourcesAudit { target, input } => {
            let root = service.authorized_research_root(target, None).await?;
            json!(ResearchService::new(root).audit_sources(input).await?)
        }
        ResearchOperation::Validate {
            target,
            input,
            source_scope,
        } => {
            let root = service
                .authorized_research_root(target, Some(&source_scope))
                .await?;
            json!(
                ResearchService::new(root)
                    .validate_bundle(input, source_scope)
                    .await?
            )
        }
        ResearchOperation::Import {
            target,
            input,
            source_scope,
        } => {
            let root = service
                .authorized_research_root(target, Some(&source_scope))
                .await?;
            json!(
                service
                    .import_evidence_bundle(root, input, source_scope, context.clone())
                    .await?
            )
        }
        ResearchOperation::Export {
            id,
            source_scope,
            revision,
        } => json!(
            service
                .export_evidence_bundle(id, source_scope, revision)
                .await?
        ),
        ResearchOperation::View {
            target,
            input,
            source_scope,
            focus,
        } => {
            let root = service
                .authorized_research_root(target, Some(&source_scope))
                .await?;
            json!(
                ResearchService::new(root)
                    .bundle_view(input, source_scope, focus)
                    .await?
            )
        }
        ResearchOperation::Impact {
            target,
            input,
            source_scope,
            node,
            label,
        } => {
            let root = service
                .authorized_research_root(target, Some(&source_scope))
                .await?;
            json!(
                ResearchService::new(root)
                    .revise_bundle(input, source_scope, node, label)
                    .await?
            )
        }
        ResearchOperation::Status {
            target,
            delivery,
            catalog,
            bundle,
            source_scope,
            requirements,
        } => {
            let root = service
                .authorized_research_root(target, source_scope.as_deref())
                .await?;
            json!(
                service
                    .research_status(
                        ResearchStatusRequest {
                            root,
                            delivery,
                            catalog,
                            bundle,
                            scope: source_scope,
                            requirements
                        },
                        context.clone()
                    )
                    .await?
            )
        }
        ResearchOperation::MapPlan {
            target,
            transaction,
        } => {
            let root = service.authorized_research_root(target, None).await?;
            json!(
                KnowledgeMapService::new(root)
                    .source_batch(&context, transaction, false)
                    .await
                    .map_err(super::knowledge_map_web_error)?
            )
        }
        ResearchOperation::MapApply {
            target,
            transaction,
        } => {
            let root = service.authorized_research_root(target, None).await?;
            json!(
                KnowledgeMapService::new(root)
                    .source_batch(&context, transaction, true)
                    .await
                    .map_err(super::knowledge_map_web_error)?
            )
        }
    };
    let version = GraphVersion::new(
        response
            .get("graph_version")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    );
    Ok((ApiMetadata::graph_only(&context, version), response))
}

#[cfg(test)]
#[path = "research_tests.rs"]
mod tests;
