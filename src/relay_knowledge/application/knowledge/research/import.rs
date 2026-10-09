//! Bundle import/export uses existing graph storage and fact lifecycle.
use super::{
    bundle::{AuthoredBundleAudit, document_id, load_bundle},
    reader,
};
use crate::{
    api::{ApiError, IngestResponse, RequestContext},
    application::RelayKnowledgeService,
    domain::{
        FactStatus, GraphVersion,
        research::{AuthoredEvidenceBundle, validate_sha256},
    },
};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
pub struct AuthoredBundleImportResponse {
    pub state: String,
    pub audit: AuthoredBundleAudit,
    pub document_id: String,
    pub fact_status: FactStatus,
    pub graph_version: Option<GraphVersion>,
    pub ingest: Option<IngestResponse>,
}

#[derive(Debug, Serialize)]
pub struct AuthoredBundleExportResponse {
    pub bundle: AuthoredEvidenceBundle,
    pub bundle_sha256: String,
    pub fact_status: FactStatus,
    pub graph_version: GraphVersion,
}

impl RelayKnowledgeService {
    pub async fn import_evidence_bundle(
        &self,
        root: PathBuf,
        input: PathBuf,
        scope: String,
        context: RequestContext,
    ) -> Result<AuthoredBundleImportResponse, ApiError> {
        let (loaded, ingest) = reader::run(root, move |reader| {
            let loaded = load_bundle(reader, &input, &scope)?;
            let ingest = if loaded.report.valid {
                Some(super::projection::ingest_request(&loaded)?)
            } else {
                None
            };
            Ok((loaded, ingest))
        })
        .await?;
        let id = document_id(
            &loaded.bundle.id,
            &loaded.bundle.source_scope,
            &loaded.report.bundle_sha256,
        );
        let mut response = AuthoredBundleImportResponse {
            state: "invalid".into(),
            audit: loaded.report,
            document_id: id.clone(),
            fact_status: FactStatus::Proposed,
            graph_version: None,
            ingest: None,
        };
        let Some(ingest) = ingest else {
            return Ok(response);
        };
        let store = self
            .store()
            .await
            .map_err(|error| ApiError::storage_unavailable(error.to_string()))?;
        if let Some(existing) = store
            .evidence_document(id, loaded.bundle.source_scope.clone())
            .await
            .map_err(|error| ApiError::storage_unavailable(error.to_string()))?
        {
            let expected = response.audit.bundle_sha256.clone();
            let content = existing.content;
            if !reader::compute(move |_| Ok(reader::digest(content.as_bytes()) == expected)).await?
            {
                return Err(ApiError::invalid_argument(
                    "stored bundle content does not match its immutable revision",
                ));
            }
            response.state = "already_imported".into();
            response.audit.import_state = "imported".into();
            response.fact_status = existing.status;
            response.audit.stored_fact_status = Some(existing.status);
            response.graph_version = Some(existing.graph_version);
            return Ok(response);
        }
        if let Some(previous) = &loaded.bundle.supersedes {
            let old_id = document_id(&loaded.bundle.id, &loaded.bundle.source_scope, previous);
            if store
                .evidence_document(old_id, loaded.bundle.source_scope.clone())
                .await
                .map_err(|error| ApiError::storage_unavailable(error.to_string()))?
                .is_none()
            {
                return Err(ApiError::invalid_argument(
                    "superseded revision is not imported in this bundle scope",
                ));
            }
        }
        let committed = self.ingest(ingest, context).await?;
        response.state = if committed.index_refresh_error.is_some() {
            "imported_index_pending"
        } else {
            "imported"
        }
        .into();
        response.audit.import_state = "imported".into();
        response.audit.stored_fact_status = Some(FactStatus::Proposed);
        response.graph_version = Some(committed.receipt.graph_version);
        response.ingest = Some(committed);
        Ok(response)
    }

    pub async fn export_evidence_bundle(
        &self,
        id: String,
        scope: String,
        revision: String,
    ) -> Result<AuthoredBundleExportResponse, ApiError> {
        validate_sha256(&revision)
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let store = self
            .store()
            .await
            .map_err(|error| ApiError::storage_unavailable(error.to_string()))?;
        let record = store
            .evidence_document(document_id(&id, &scope, &revision), scope.clone())
            .await
            .map_err(|error| ApiError::storage_unavailable(error.to_string()))?
            .ok_or_else(|| {
                ApiError::invalid_argument("bundle revision is not imported in the requested scope")
            })?;
        reader::compute(move |_| {
            // Exact lookup is byte-bounded at the store boundary. The digest checks
            // persisted content before deserializing any authored extension fields.
            if reader::digest(record.content.as_bytes()) != revision {
                return Err(ApiError::invalid_argument("stored bundle digest mismatch"));
            }
            let bundle: AuthoredEvidenceBundle = serde_json::from_str(&record.content)
                .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
            bundle
                .validate_shape()
                .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
            if bundle.id != id || bundle.source_scope != scope {
                return Err(ApiError::invalid_argument(
                    "stored bundle identity mismatch",
                ));
            }
            Ok(AuthoredBundleExportResponse {
                bundle,
                bundle_sha256: revision,
                fact_status: record.status,
                graph_version: record.graph_version,
            })
        })
        .await
    }
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
