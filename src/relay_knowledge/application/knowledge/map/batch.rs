//! Compare-and-swap batches on the existing root-last map publication boundary.

use std::{
    path::Path,
    sync::{Arc, LazyLock},
    time::Duration,
};

static BATCH_WORKERS: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(4)));

use serde::{Deserialize, Serialize};
use tokio::{fs, io::AsyncReadExt};

use crate::{
    api::{ApiMetadata, RequestContext},
    domain::map_batch::{MAX_BATCH_BYTES, MapBatchDiagnostic, MapBatchPreview, MapBatchRequest},
};

use super::{
    ARTIFACT_SCHEMA_VERSION, KnowledgeMapSchemaProbe, KnowledgeMapService,
    KnowledgeMapServiceError, content_digest, metadata, now_stamp,
};

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeMapBatchResponse {
    pub metadata: ApiMetadata,
    pub transaction: MapBatchRequest,
    pub state: String,
    pub map_version: u64,
    pub map_digest: String,
    pub preview: MapBatchPreview,
    pub cleanup: MapBatchCleanup,
}

#[derive(Debug, Clone, Serialize)]
pub struct MapBatchCleanup {
    pub state: String,
    pub reader_grace_seconds: u64,
    pub next_step: Option<String>,
}

/// Stored inside the existing summary string so older v4 writers preserve receipts.
#[derive(Debug, Serialize, Deserialize)]
struct BatchReceipt {
    transaction_id: String,
    request_digest: String,
    transaction: MapBatchRequest,
}

impl KnowledgeMapService {
    /// Bounded JSON input, shared by CLI and callers of the application service.
    pub async fn batch_from_file(
        &self,
        context: &RequestContext,
        input: &Path,
        apply: bool,
    ) -> Result<KnowledgeMapBatchResponse, KnowledgeMapServiceError> {
        if !fs::symlink_metadata(input).await?.is_file() {
            return Err(KnowledgeMapServiceError::InvalidRequest(
                "batch input must be a regular JSON file".into(),
            ));
        }
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
        let file = options.open(input).await?;
        if !file.metadata().await?.is_file() {
            return Err(KnowledgeMapServiceError::InvalidRequest(
                "batch input changed to a non-regular file".into(),
            ));
        }
        let mut bytes = Vec::new();
        file.take((MAX_BATCH_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() > MAX_BATCH_BYTES {
            return Err(KnowledgeMapServiceError::InvalidRequest(
                "batch input exceeds 256 KiB".into(),
            ));
        }
        let request = serde_json::from_slice(&bytes).map_err(|error| {
            KnowledgeMapServiceError::InvalidRequest(format!(
                "invalid map transaction JSON: {error}"
            ))
        })?;
        self.source_batch(context, request, apply).await
    }

    pub async fn source_batch(
        &self,
        context: &RequestContext,
        mut request: MapBatchRequest,
        apply: bool,
    ) -> Result<KnowledgeMapBatchResponse, KnowledgeMapServiceError> {
        self.require_knowledge_map("map plan/apply")?;
        request.validate()?;
        if apply && (request.expected_map_version.is_none() || request.expected_digest.is_none()) {
            return Err(KnowledgeMapServiceError::InvalidRequest(
                "apply requires expected_map_version and expected_digest from map plan".into(),
            ));
        }
        let _locks = if apply {
            Some(self.acquire_legacy_aware_mutation_locks().await?)
        } else {
            None
        };
        // A read-only plan can read the recovery root without renaming it. Apply
        // recovers only previously committed state before evaluating preconditions.
        if apply {
            self.recover_manifest_backup().await?;
        }
        let root = self.read_root_content().await?;
        let probe: KnowledgeMapSchemaProbe = serde_norway::from_str(&root)
            .map_err(|error| KnowledgeMapServiceError::Yaml(error.to_string()))?;
        if probe.schema_version != ARTIFACT_SCHEMA_VERSION {
            return Err(KnowledgeMapServiceError::InvalidRequest(
                "map batches require v4; run map init before planning".into(),
            ));
        }
        let snapshot = self.load_mutation_snapshot(&root).await?;
        snapshot.map.validate_reserved_repository_routes()?;
        if snapshot.requires_publish {
            return Err(KnowledgeMapServiceError::InvalidRequest(
                "map requires repair; run map init before planning".into(),
            ));
        }
        let digest = content_digest(root.as_bytes());
        let request_digest = content_digest(
            &serde_json::to_vec(&request)
                .map_err(|error| KnowledgeMapServiceError::InvalidRequest(error.to_string()))?,
        );
        let mut response = KnowledgeMapBatchResponse {
            metadata: metadata(context),
            transaction: request.clone(),
            state: "planned".into(),
            map_version: snapshot.map.map_version,
            map_digest: digest.clone(),
            preview: MapBatchPreview {
                differences: Vec::new(),
                affected_routes: Vec::new(),
                diagnostics: Vec::new(),
            },
            cleanup: MapBatchCleanup {
                state: "not_run".into(),
                reader_grace_seconds: 60,
                next_step: None,
            },
        };
        for entry in snapshot
            .map
            .history
            .iter()
            .filter(|entry| entry.action == "source.batch")
        {
            let receipt: BatchReceipt = serde_json::from_str(&entry.summary).map_err(|error| {
                KnowledgeMapServiceError::Integrity(format!("invalid batch receipt: {error}"))
            })?;
            if receipt.transaction_id == request.transaction_id {
                if receipt.request_digest == request_digest {
                    response.state = "already_applied".into();
                } else {
                    response.reject(
                        "transaction_conflict",
                        "transaction id already names a different request",
                    );
                }
                return Ok(response);
            }
        }
        if request
            .expected_map_version
            .is_some_and(|version| version != snapshot.map.map_version)
            || request
                .expected_digest
                .as_ref()
                .is_some_and(|expected| expected != &digest)
        {
            response.reject(
                "precondition_conflict",
                "map changed or receipt expired; inspect current state and plan a new transaction",
            );
            return Ok(response);
        }
        request.expected_map_version = Some(snapshot.map.map_version);
        request.expected_digest = Some(digest);
        response.transaction = request.clone();
        let permit = Arc::clone(&BATCH_WORKERS)
            .try_acquire_owned()
            .map_err(|_| {
                KnowledgeMapServiceError::InvalidRequest("map batch worker budget exhausted".into())
            })?;
        let preview_request = request.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let (candidate, preview) =
                preview_request.preview(&snapshot.map, snapshot.omitted_through);
            let unchanged = candidate == snapshot.map;
            (snapshot, candidate, preview, unchanged)
        });
        let (mut snapshot, candidate, preview, unchanged) =
            tokio::time::timeout(Duration::from_secs(30), worker)
                .await
                .map_err(|_| {
                    KnowledgeMapServiceError::InvalidRequest(
                        "map batch preview deadline exceeded".into(),
                    )
                })?
                .map_err(|error| {
                    KnowledgeMapServiceError::InvalidRequest(format!(
                        "map batch worker failed: {error}"
                    ))
                })?;
        response.preview = preview;
        if !response.preview.diagnostics.is_empty() {
            response.state = "invalid".into();
            return Ok(response);
        }
        if !apply {
            return Ok(response);
        }
        if unchanged {
            response.state = "unchanged".into();
            return Ok(response);
        }
        if snapshot.map.map_version == u64::MAX {
            return Err(KnowledgeMapServiceError::InvalidRequest(
                "map version overflow".into(),
            ));
        }
        snapshot.map = candidate;
        let receipt = BatchReceipt {
            transaction_id: request.transaction_id.clone(),
            request_digest,
            transaction: request,
        };
        snapshot.map.record_change(
            "source.batch",
            serde_json::to_string(&receipt)
                .map_err(|error| KnowledgeMapServiceError::InvalidRequest(error.to_string()))?,
            now_stamp(),
        );
        self.write_map(&mut snapshot).await?;
        response.state = "applied".into();
        response.map_version = snapshot.map.map_version;
        response.map_digest = content_digest(self.read_root_content().await?.as_bytes());
        // Publication succeeds independently of best-effort maintenance. The
        // previous root remains a recovery reference; no eager shard deletion.
        response.cleanup = MapBatchCleanup {
            state: "deferred".into(), reader_grace_seconds: 60,
            next_step: Some("map init resumes bounded cleanup after reader grace; recovery-root references stay protected".into()),
        };
        Ok(response)
    }
}

impl KnowledgeMapBatchResponse {
    fn reject(&mut self, code: &str, message: &str) {
        self.state = "conflict".into();
        self.preview.diagnostics.push(MapBatchDiagnostic {
            operation_index: None,
            code: code.into(),
            message: message.into(),
        });
    }
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
