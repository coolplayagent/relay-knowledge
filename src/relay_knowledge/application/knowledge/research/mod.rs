//! Repository-owned research artifacts; navigation and derived indexes stay separate.

mod access;
pub use access::ResearchRootSelection;
mod audit;
mod bundle;
mod import;
mod projection;
mod reader;
mod requirements;
mod status;
pub use requirements::{RequirementAudit, RequirementEvidenceAudit};
pub use status::{ResearchMapState, ResearchStatusRequest, ResearchStatusResponse};
mod view;

use crate::api::ApiError;
pub use audit::{ArtifactAudit, CaptureAudit, ResearchDiagnostic, SourceAuditReport};
pub use bundle::{AuthoredBundleAudit, AuthoredRelationAudit};
pub use import::{AuthoredBundleExportResponse, AuthoredBundleImportResponse};
use std::path::PathBuf;

/// Local operations are read-only and isolate heavy parsing and file I/O.
pub struct ResearchService {
    root: PathBuf,
}

impl ResearchService {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub async fn validate_bundle(
        &self,
        input: PathBuf,
        scope: String,
    ) -> Result<AuthoredBundleAudit, ApiError> {
        reader::run(self.root.clone(), move |reader| {
            bundle::load_bundle(reader, &input, &scope).map(|loaded| loaded.report)
        })
        .await
    }

    pub async fn revise_bundle(
        &self,
        input: PathBuf,
        scope: String,
        node: String,
        label: String,
    ) -> Result<serde_json::Value, ApiError> {
        reader::run(self.root.clone(), move |reader| {
            let loaded = bundle::load_bundle(reader, &input, &scope)?;
            let (revision, affected) = loaded.bundle.revise_label(&node, &label, loaded.report.bundle_sha256.clone()).map_err(|error| ApiError::invalid_argument(error.to_string()))?;
            Ok(serde_json::json!({"state":"proposed_revision", "audit":loaded.report,"affected_relation_indices":affected,"revision":revision}))
        }).await
    }

    pub async fn bundle_view(
        &self,
        input: PathBuf,
        scope: String,
        focus: Option<String>,
    ) -> Result<serde_json::Value, ApiError> {
        reader::run(self.root.clone(), move |reader| {
            let loaded = bundle::load_bundle(reader, &input, &scope)?;
            Ok(serde_json::json!({"audit":loaded.report,"view":view::graph_view(loaded.bundle.graph, focus.as_deref())?}))
        }).await
    }

    pub async fn audit_sources(&self, input: PathBuf) -> Result<SourceAuditReport, ApiError> {
        reader::run(self.root.clone(), move |reader| {
            audit::audit_catalog(reader, &input)
        })
        .await
    }
}

#[cfg(test)]
#[path = "test_support.rs"]
mod test_support;

#[cfg(test)]
pub(crate) use test_support::TEST_LOCK as RESEARCH_TEST_LOCK;
