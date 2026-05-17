use super::*;
use crate::{
    api::{
        IndexRefreshRequest, IngestEvidence, IngestEvidenceExtraction, InterfaceKind,
        MultimodalExtractionRequest,
    },
    domain::{
        EvidenceModality, ExtractionDiagnostic, ExtractionStatus, FreshnessPolicy, IndexKind,
        IndexState, RerankMode, RetrievalBackendState, RetrieverSource,
    },
    env::PlatformKind,
    retrieval::LOCAL_RERANK_MODEL,
    storage::{KnowledgeStore, SqliteGraphStore},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn status_includes_foundational_runtime_configuration() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
            ("RELAY_KNOWLEDGE_HTTP_BIND", "127.0.0.1:9000"),
            ("HTTPS_PROXY", "https://proxy.internal:8443"),
            ("NO_PROXY", "localhost,.internal"),
            ("SSL_VERIFY", "false"),
            ("RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH", "42"),
        ],
    )
    .expect("environment should parse");
    let service = service_with_environment(&environment).await;
    let context = RequestContext::with_ids(InterfaceKind::Cli, "req", "trace");

    let response = service
        .project_status(context)
        .await
        .expect("status should load");

    assert_eq!(response.runtime.config_dir, "/srv/relay/config");
    assert_eq!(response.runtime.data_dir, "/srv/relay/data");
    assert_eq!(response.runtime.http_bind, "127.0.0.1:9000");
    assert!(response.runtime.http_proxy_configured);
    assert_eq!(response.runtime.http_no_proxy_rules, 2);
    assert!(!response.runtime.http_ssl_verify);
    assert_eq!(response.runtime.qos_max_queue_depth, 42);
    assert_eq!(response.runtime.rerank_backend_mode, "local");
    assert_eq!(
        response.runtime.rerank_model.as_deref(),
        Some(LOCAL_RERANK_MODEL)
    );
    assert_eq!(response.runtime.rerank_candidate_multiplier, 4);
    assert_eq!(response.runtime.rerank_max_candidates, 64);
}

#[tokio::test]
async fn status_reflects_refreshed_network_environment() {
    let initial_environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
        ],
    )
    .expect("environment should parse");
    let service = service_with_environment(&initial_environment).await;

    let refreshed_environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HTTP_PROXY", "http://proxy.internal:8080"),
            ("SSL_VERIFY", "false"),
            ("RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS", "4"),
        ],
    )
    .expect("environment should parse");

    service
        .refresh_network_from_environment(&refreshed_environment)
        .await
        .expect("network refresh should succeed");
    let response = service
        .project_status(RequestContext::with_ids(InterfaceKind::Cli, "req", "trace"))
        .await
        .expect("status should load");

    assert!(response.runtime.http_proxy_configured);
    assert!(!response.runtime.http_ssl_verify);
    assert_eq!(response.runtime.qos_max_in_flight_requests, 4);
}

#[tokio::test]
async fn project_status_reports_current_graph_version() {
    let service = service_with_memory_store().await;
    service
        .ingest(
            ingest_request(vec![ingest_evidence(
                "ev-status",
                "Project status tracks graph versions",
                Vec::new(),
            )]),
            RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
        )
        .await
        .expect("ingest should succeed");

    let response = service
        .project_status(RequestContext::with_ids(
            InterfaceKind::Cli,
            "req-status",
            "trace-status",
        ))
        .await
        .expect("status should load");

    assert_eq!(response.metadata.graph_version, 1);
    assert_eq!(response.metadata.trace_id, "trace-status");
}

#[tokio::test]
async fn ingest_commits_graph_and_refreshes_all_indexes() {
    let service = service_with_memory_store().await;
    let context = RequestContext::with_ids(InterfaceKind::Cli, "req", "trace");

    let response = service
        .ingest(
            ingest_request(vec![ingest_evidence(
                "ev-1",
                "Hybrid retrieval uses BM25 and vector indexes",
                vec!["BM25".to_owned(), "Vector".to_owned()],
            )]),
            context,
        )
        .await
        .expect("ingest should succeed");

    assert_eq!(response.metadata.graph_version, 1);
    assert!(!response.metadata.stale);
    assert_eq!(response.receipt.evidence_count, 1);
    assert_eq!(response.indexes.len(), 3);
    assert!(
        response
            .indexes
            .iter()
            .all(|status| status.state == IndexState::Fresh)
    );
}

#[tokio::test]
async fn commits_multimodal_extraction_through_maintenance_boundary() {
    let service = service_with_memory_store().await;
    service
        .ingest(
            ingest_request(vec![IngestEvidence {
                id: Some("image-1".to_owned()),
                content: "Architecture diagram image asset".to_owned(),
                entity_labels: vec!["GraphRAG".to_owned()],
                extraction: Some(IngestEvidenceExtraction {
                    modality: EvidenceModality::ImageAsset,
                    media_hash: Some("sha256:image".to_owned()),
                    ..text_extraction()
                }),
                ..ingest_evidence("image-1", "", Vec::new())
            }]),
            RequestContext::with_ids(InterfaceKind::Cli, "req-image", "trace-image"),
        )
        .await
        .expect("parent image should ingest");

    let response = service
        .commit_multimodal_extraction(
            MultimodalExtractionRequest {
                source_scope: "docs".to_owned(),
                parent_evidence_id: "image-1".to_owned(),
                derived_evidence: vec![IngestEvidence {
                    id: Some("ocr-1".to_owned()),
                    content: "OCR text names the vector ANN read model".to_owned(),
                    entity_labels: vec!["Vector".to_owned()],
                    extraction: Some(IngestEvidenceExtraction {
                        modality: EvidenceModality::OcrText,
                        parent_evidence_id: Some("image-1".to_owned()),
                        extractor: Some("ocr-maintenance-worker".to_owned()),
                        extractor_version: Some("1.0".to_owned()),
                        ..text_extraction()
                    }),
                    ..ingest_evidence("ocr-1", "", Vec::new())
                }],
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-ocr", "trace-ocr"),
        )
        .await
        .expect("maintenance extraction should commit");

    assert_eq!(response.parent_evidence_id, "image-1");
    assert_eq!(response.derived_evidence_count, 1);
    assert_eq!(response.receipt.evidence_count, 1);
}

#[tokio::test]
async fn multimodal_caption_and_ocr_hits_group_by_parent_image() {
    let service = service_with_memory_store().await;
    service
        .ingest(
            ingest_request(vec![
                IngestEvidence {
                    id: Some("doc-text".to_owned()),
                    content: "PDF page text introduces the diagram".to_owned(),
                    source_path: Some("guide.pdf".to_owned()),
                    extraction: Some(IngestEvidenceExtraction {
                        source_uri: Some("file:///guide.pdf".to_owned()),
                        source_hash: Some("sha256:pdf".to_owned()),
                        ..text_extraction()
                    }),
                    ..ingest_evidence("doc-text", "", Vec::new())
                },
                IngestEvidence {
                    id: Some("image-parent".to_owned()),
                    content: "PDF page diagram image asset".to_owned(),
                    source_path: Some("guide.pdf".to_owned()),
                    extraction: Some(IngestEvidenceExtraction {
                        modality: EvidenceModality::ImageAsset,
                        source_uri: Some("file:///guide.pdf#page=1".to_owned()),
                        media_hash: Some("sha256:image-parent".to_owned()),
                        ..text_extraction()
                    }),
                    ..ingest_evidence("image-parent", "", Vec::new())
                },
            ]),
            RequestContext::with_ids(InterfaceKind::Cli, "req-doc", "trace-doc"),
        )
        .await
        .expect("document evidence should ingest");
    service
        .commit_multimodal_extraction(
            MultimodalExtractionRequest {
                source_scope: "docs".to_owned(),
                parent_evidence_id: "image-parent".to_owned(),
                derived_evidence: vec![
                    derived_image_text("ocr-parent", EvidenceModality::OcrText),
                    derived_image_text("caption-parent", EvidenceModality::Caption),
                ],
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-derived", "trace-derived"),
        )
        .await
        .expect("derived image evidence should commit");

    let response = service
        .retrieve_context(
            HybridRetrievalRequest {
                query: "diagram topology".to_owned(),
                source_scope: Some("docs".to_owned()),
                limit: 10,
                freshness: FreshnessPolicy::WaitUntilFresh,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-query", "trace-query"),
        )
        .await
        .expect("multimodal query should succeed");

    let grouped = response
        .results
        .iter()
        .filter(|hit| hit.evidence_id == "image-parent")
        .count();
    assert_eq!(grouped, 1);
    assert!(
        response
            .results
            .iter()
            .all(|hit| hit.source_scope == "docs")
    );
}

#[tokio::test]
async fn failed_ocr_diagnostic_does_not_block_image_or_text_retrieval() {
    let service = service_with_memory_store().await;
    service
        .ingest(
            ingest_request(vec![
                ingest_evidence(
                    "text-ok",
                    "Text extraction stays searchable when OCR fails",
                    vec!["OCR".to_owned()],
                ),
                IngestEvidence {
                    id: Some("image-failed-ocr".to_owned()),
                    content: "Scanned image asset with failed OCR diagnostic".to_owned(),
                    extraction: Some(IngestEvidenceExtraction {
                        modality: EvidenceModality::ImageAsset,
                        media_hash: Some("sha256:failed-image".to_owned()),
                        diagnostic: Some(
                            ExtractionDiagnostic::new(
                                ExtractionStatus::Failed,
                                Some("OCR worker timed out".to_owned()),
                            )
                            .expect("diagnostic should validate"),
                        ),
                        ..text_extraction()
                    }),
                    ..ingest_evidence("image-failed-ocr", "", Vec::new())
                },
            ]),
            RequestContext::with_ids(InterfaceKind::Cli, "req-ocr-fail", "trace-ocr-fail"),
        )
        .await
        .expect("failed OCR diagnostic should not block ingest");

    let text = service
        .retrieve_context(
            HybridRetrievalRequest {
                query: "searchable OCR".to_owned(),
                source_scope: Some("docs".to_owned()),
                limit: 10,
                freshness: FreshnessPolicy::WaitUntilFresh,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-text", "trace-text"),
        )
        .await
        .expect("text query should succeed");
    let image = service
        .retrieve_context(
            HybridRetrievalRequest {
                query: "failed diagnostic".to_owned(),
                source_scope: Some("docs".to_owned()),
                limit: 10,
                freshness: FreshnessPolicy::WaitUntilFresh,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-image", "trace-image"),
        )
        .await
        .expect("image query should succeed");

    assert!(text.results.iter().any(|hit| hit.evidence_id == "text-ok"));
    assert!(
        image
            .results
            .iter()
            .any(|hit| hit.evidence_id == "image-failed-ocr")
    );
}

#[tokio::test]
async fn retrieve_context_reports_results_and_index_freshness() {
    let service = service_with_memory_store().await;
    let context = RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest");
    service
        .ingest(
            ingest_request(vec![ingest_evidence(
                "ev-1",
                "Rust async services isolate blocking SQLite work",
                vec!["Rust".to_owned()],
            )]),
            context,
        )
        .await
        .expect("ingest should succeed");

    let response = service
        .retrieve_context(
            HybridRetrievalRequest {
                query: "SQLite".to_owned(),
                source_scope: Some("docs".to_owned()),
                limit: 5,
                freshness: FreshnessPolicy::WaitUntilFresh,
            },
            RequestContext::with_ids(InterfaceKind::Web, "req-query", "trace-query"),
        )
        .await
        .expect("query should succeed");

    assert_eq!(response.metadata.trace_id, "trace-query");
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].evidence_id, "ev-1");
    assert_eq!(response.context_pack.items.len(), 1);
    assert_eq!(
        response.context_pack.freshness,
        FreshnessPolicy::WaitUntilFresh
    );
    assert!(!response.truncated);
    assert_eq!(response.fusion.algorithm, "reciprocal_rank_fusion");
    assert_eq!(response.rerank.effective_mode, RerankMode::Local);
    assert_eq!(response.rerank.returned_count, 1);
    assert!(response.results[0].rerank.is_some());
    assert!(response.context_pack.items[0].rerank.is_some());
    assert!(
        response.results[0]
            .ranking
            .iter()
            .any(|signal| signal.source == crate::domain::RetrieverSource::Bm25)
    );
    assert!(!response.metadata.stale);
    assert_eq!(
        response
            .indexes
            .iter()
            .map(|status| status.kind)
            .collect::<Vec<_>>(),
        IndexKind::ALL
    );
}

#[tokio::test]
async fn disabled_read_model_backends_do_not_run_retriever_sources() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
            ("RELAY_KNOWLEDGE_SEMANTIC_BACKEND", "disabled"),
            ("RELAY_KNOWLEDGE_VECTOR_BACKEND", "disabled"),
        ],
    )
    .expect("environment should parse");
    let service = service_with_environment(&environment).await;
    service
        .ingest(
            ingest_request(vec![ingest_evidence(
                "ev-disabled",
                "Disabled read models still allow BM25 fallback retrieval",
                vec!["BM25".to_owned()],
            )]),
            RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
        )
        .await
        .expect("ingest should succeed");

    let response = service
        .retrieve_context(
            HybridRetrievalRequest {
                query: "read models".to_owned(),
                source_scope: Some("docs".to_owned()),
                limit: 5,
                freshness: FreshnessPolicy::WaitUntilFresh,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-query", "trace-query"),
        )
        .await
        .expect("query should succeed");

    assert!(!response.results.is_empty());
    assert!(response.backend_statuses.iter().all(|status| {
        matches!(
            status.source,
            RetrieverSource::Semantic | RetrieverSource::Vector
        ) && status.state == RetrievalBackendState::Unavailable
    }));
    assert!(response.results.iter().all(|hit| {
        !hit.retriever_sources.contains(&RetrieverSource::Semantic)
            && !hit.retriever_sources.contains(&RetrieverSource::Vector)
    }));
}

#[tokio::test]
async fn external_rerank_mode_degrades_to_local_deterministic_rerank() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
            ("RELAY_KNOWLEDGE_RERANK_BACKEND", "external"),
        ],
    )
    .expect("environment should parse");
    let service = service_with_environment(&environment).await;
    service
        .ingest(
            ingest_request(vec![ingest_evidence(
                "ev-rerank",
                "Rerank diagnostics preserve local deterministic fallback",
                vec!["Rerank".to_owned()],
            )]),
            RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
        )
        .await
        .expect("ingest should succeed");

    let response = service
        .retrieve_context(
            HybridRetrievalRequest {
                query: "rerank diagnostics".to_owned(),
                source_scope: Some("docs".to_owned()),
                limit: 5,
                freshness: FreshnessPolicy::AllowStale,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-query", "trace-query"),
        )
        .await
        .expect("query should succeed");

    assert_eq!(response.rerank.requested_mode, RerankMode::External);
    assert_eq!(response.rerank.effective_mode, RerankMode::Local);
    assert!(response.rerank.degraded);
    assert!(
        response
            .degraded_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("external rerank provider contract"))
    );
}

#[tokio::test]
async fn index_refresh_cursors_use_indexed_document_model_metadata() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
            ("RELAY_KNOWLEDGE_SEMANTIC_BACKEND", "external"),
            ("RELAY_KNOWLEDGE_VECTOR_BACKEND", "external"),
            (
                "RELAY_KNOWLEDGE_EMBEDDING_BASE_URL",
                "https://embeddings.example/v1",
            ),
            ("RELAY_KNOWLEDGE_EMBEDDING_API_KEY", "secret-key"),
            ("RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL", "runtime-model"),
            ("RELAY_KNOWLEDGE_EMBEDDING_DIMENSION", "1536"),
        ],
    )
    .expect("environment should parse");
    let service = service_with_environment(&environment).await;
    let mut evidence = ingest_evidence(
        "ev-model",
        "Model provenance should come from indexed document metadata",
        vec!["Model".to_owned()],
    );
    evidence.extraction = Some(IngestEvidenceExtraction {
        embedding_model: Some("stored-doc-model".to_owned()),
        embedding_dimension: Some(384),
        ..text_extraction()
    });
    service
        .ingest(
            ingest_request(vec![evidence]),
            RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
        )
        .await
        .expect("ingest should succeed");

    let response = service
        .refresh_indexes(
            IndexRefreshRequest {
                kinds: vec![IndexKind::Semantic, IndexKind::Vector],
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-refresh", "trace-refresh"),
        )
        .await
        .expect("refresh should succeed");

    let read_model_cursors = response
        .index_cursors
        .iter()
        .filter(|cursor| matches!(cursor.kind, IndexKind::Semantic | IndexKind::Vector))
        .collect::<Vec<_>>();
    assert_eq!(read_model_cursors.len(), 2);
    assert!(read_model_cursors.iter().all(|cursor| {
        cursor.model_name.as_deref() == Some("stored-doc-model")
            && cursor.model_dimension == Some(384)
    }));
}

#[tokio::test]
async fn probe_embedding_provider_reports_echo_success_without_secret_leakage() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
            ("RELAY_KNOWLEDGE_VECTOR_BACKEND", "external"),
            ("RELAY_KNOWLEDGE_LLM_PROVIDER", "echo"),
            (
                "RELAY_KNOWLEDGE_EMBEDDING_BASE_URL",
                "https://user:pass@embeddings.example/v1",
            ),
            ("RELAY_KNOWLEDGE_EMBEDDING_API_KEY", "secret-key"),
            ("RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL", "runtime-model"),
            ("RELAY_KNOWLEDGE_EMBEDDING_DIMENSION", "4"),
        ],
    )
    .expect("environment should parse");
    let service = service_with_environment(&environment).await;

    let response = service
        .probe_embedding_provider(RequestContext::with_ids(
            InterfaceKind::Cli,
            "req-provider",
            "trace-provider",
        ))
        .await
        .expect("probe should run");

    assert!(response.ok);
    assert_eq!(response.provider, Some("echo".to_owned()));
    assert_eq!(response.model, "runtime-model");
    assert_eq!(response.dimension, 4);
    assert_eq!(response.error_code, None);
    assert_eq!(
        service
            .project_status(RequestContext::with_ids(
                InterfaceKind::Cli,
                "req-status",
                "trace-status",
            ))
            .await
            .expect("status should load")
            .runtime
            .embedding_base_url,
        Some("https://embeddings.example".to_owned())
    );
}

#[tokio::test]
async fn probe_embedding_provider_reports_retryable_rate_limit_as_reachable() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("local addr should load");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("request should connect");
        let mut buffer = vec![0; 2048];
        let count = stream.read(&mut buffer).await.expect("request should read");
        let request = String::from_utf8_lossy(&buffer[..count]);

        assert!(request.starts_with("POST /v1/embeddings HTTP/1.1"));
        assert!(request.contains("authorization: Bearer secret-key"));
        let body =
            r#"{"error":{"code":"1113","message":"Insufficient balance or no resource package."}}"#;
        let response = format!(
            "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("response should write");
    });
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        vec![
            ("HOME".to_owned(), "/home/alice".to_owned()),
            ("TMPDIR".to_owned(), "/tmp".to_owned()),
            ("RELAY_KNOWLEDGE_HOME".to_owned(), "/srv/relay".to_owned()),
            (
                "RELAY_KNOWLEDGE_VECTOR_BACKEND".to_owned(),
                "external".to_owned(),
            ),
            (
                "RELAY_KNOWLEDGE_LLM_PROVIDER".to_owned(),
                "openai_compatible".to_owned(),
            ),
            (
                "RELAY_KNOWLEDGE_EMBEDDING_BASE_URL".to_owned(),
                format!("http://{addr}/v1"),
            ),
            (
                "RELAY_KNOWLEDGE_EMBEDDING_API_KEY".to_owned(),
                "secret-key".to_owned(),
            ),
            (
                "RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL".to_owned(),
                "runtime-model".to_owned(),
            ),
            (
                "RELAY_KNOWLEDGE_EMBEDDING_DIMENSION".to_owned(),
                "4".to_owned(),
            ),
        ],
    )
    .expect("environment should parse");
    let service = service_with_environment(&environment).await;

    let response = service
        .probe_embedding_provider(RequestContext::with_ids(
            InterfaceKind::Cli,
            "req-provider-rate-limit",
            "trace-provider-rate-limit",
        ))
        .await
        .expect("probe should run");

    assert!(response.ok);
    assert_eq!(response.provider, Some("openai_compatible".to_owned()));
    assert_eq!(response.error_code.as_deref(), Some("rate_limited"));
    assert_eq!(response.retryable, Some(true));
    server.await.expect("server should finish");
}

#[tokio::test]
async fn wait_until_fresh_query_does_not_increment_fresh_index_versions() {
    let service = service_with_memory_store().await;
    service
        .ingest(
            ingest_request(vec![ingest_evidence(
                "ev-fresh",
                "Fresh indexes should not refresh on read",
                vec!["Index".to_owned()],
            )]),
            RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
        )
        .await
        .expect("ingest should succeed");

    let first = retrieve_wait_until_fresh(&service, "req-query-1").await;
    let second = retrieve_wait_until_fresh(&service, "req-query-2").await;

    assert_eq!(first.metadata.index_version, Some(1));
    assert_eq!(second.metadata.index_version, Some(1));
    assert!(
        second
            .indexes
            .iter()
            .all(|status| status.index_version == 1)
    );
}

#[tokio::test]
async fn retrieve_context_reports_truncated_context_pack_budget() {
    let service = service_with_memory_store().await;
    for index in 0..3 {
        service
            .ingest(
                ingest_request(vec![ingest_evidence(
                    format!("ev-{index}"),
                    format!("Shared BM25 retrieval candidate {index}"),
                    vec!["BM25".to_owned()],
                )]),
                RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
            )
            .await
            .expect("ingest should succeed");
    }

    let response = service
        .retrieve_context(
            HybridRetrievalRequest {
                query: "BM25".to_owned(),
                source_scope: Some("docs".to_owned()),
                limit: 2,
                freshness: FreshnessPolicy::WaitUntilFresh,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-query", "trace-query"),
        )
        .await
        .expect("query should succeed");

    assert!(response.truncated);
    assert!(response.context_pack.truncated);
    assert_eq!(response.results.len(), 2);
    assert_eq!(response.budget_used.limit, 2);
    assert_eq!(response.budget_used.returned_count, 2);
    assert_eq!(response.budget_used.candidate_count, 3);
}

#[tokio::test]
async fn service_status_reports_current_graph_version() {
    let service = service_with_memory_store().await;
    service
        .ingest(
            ingest_request(vec![ingest_evidence(
                "ev-service",
                "Service status tracks graph versions",
                Vec::new(),
            )]),
            RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
        )
        .await
        .expect("ingest should succeed");

    let response = service
        .service_status(RequestContext::with_ids(
            InterfaceKind::Cli,
            "req-service",
            "trace-service",
        ))
        .await
        .expect("service status should load");

    assert_eq!(response.metadata.graph_version, 1);
    assert_eq!(response.metadata.trace_id, "trace-service");
}

#[tokio::test]
async fn rejects_empty_retrieval_query() {
    let service = service_with_memory_store().await;

    let error = service
        .retrieve_context(
            HybridRetrievalRequest::new(" "),
            RequestContext::with_ids(InterfaceKind::Cli, "req", "trace"),
        )
        .await
        .expect_err("empty query should fail");

    assert_eq!(error.message, "query must not be empty");
}

#[tokio::test]
async fn default_service_opens_sqlite_under_resolved_data_dir() {
    let root = std::env::temp_dir().join(format!("relay-knowledge-service-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            (
                "RELAY_KNOWLEDGE_HOME",
                root.to_str().expect("temp path is UTF-8"),
            ),
        ],
    )
    .expect("environment should parse");
    let service = RelayKnowledgeService::from_environment(&environment)
        .await
        .expect("service should compose");

    let health = service
        .health(RequestContext::with_ids(InterfaceKind::Cli, "req", "trace"))
        .await
        .expect("health should initialize storage");

    assert!(health.healthy);
    assert!(root.join("data").join("relay-knowledge.sqlite").exists());
}

async fn service_with_memory_store() -> RelayKnowledgeService {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));

    service_with_store(store).await
}

async fn service_with_store(store: Arc<dyn KnowledgeStore>) -> RelayKnowledgeService {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
        ],
    )
    .expect("environment should parse");

    service_with_environment_and_store(&environment, store).await
}

async fn service_with_environment(environment: &EnvironmentConfig) -> RelayKnowledgeService {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));

    service_with_environment_and_store(environment, store).await
}

async fn service_with_environment_and_store(
    environment: &EnvironmentConfig,
    store: Arc<dyn KnowledgeStore>,
) -> RelayKnowledgeService {
    let runtime = RuntimeConfiguration::from_environment(environment)
        .await
        .expect("runtime should compose");

    RelayKnowledgeService::with_store(runtime, store)
}

fn ingest_request(evidence: Vec<IngestEvidence>) -> IngestRequest {
    IngestRequest {
        source_scope: "docs".to_owned(),
        evidence,
        relations: Vec::new(),
        claims: Vec::new(),
        events: Vec::new(),
    }
}

fn ingest_evidence(
    id: impl Into<String>,
    content: impl Into<String>,
    entity_labels: Vec<String>,
) -> IngestEvidence {
    IngestEvidence {
        id: Some(id.into()),
        source_path: None,
        span: None,
        confidence: None,
        status: None,
        content: content.into(),
        entity_labels,
        extraction: None,
    }
}

fn text_extraction() -> IngestEvidenceExtraction {
    IngestEvidenceExtraction {
        modality: EvidenceModality::TextSpan,
        source_uri: None,
        source_hash: None,
        media_hash: None,
        extractor: None,
        extractor_version: None,
        observed_at: None,
        parent_evidence_id: None,
        layout_region: None,
        embedding_model: None,
        embedding_dimension: None,
        diagnostic: None,
    }
}

fn derived_image_text(id: &str, modality: EvidenceModality) -> IngestEvidence {
    IngestEvidence {
        id: Some(id.to_owned()),
        content: "diagram topology legend".to_owned(),
        entity_labels: vec!["Diagram".to_owned()],
        extraction: Some(IngestEvidenceExtraction {
            modality,
            parent_evidence_id: Some("image-parent".to_owned()),
            extractor: Some("fixture-worker".to_owned()),
            extractor_version: Some("1.0".to_owned()),
            ..text_extraction()
        }),
        ..ingest_evidence(id, "", Vec::new())
    }
}

async fn retrieve_wait_until_fresh(
    service: &RelayKnowledgeService,
    request_id: &str,
) -> HybridRetrievalResponse {
    service
        .retrieve_context(
            HybridRetrievalRequest {
                query: "Fresh".to_owned(),
                source_scope: Some("docs".to_owned()),
                limit: 5,
                freshness: FreshnessPolicy::WaitUntilFresh,
            },
            RequestContext::with_ids(InterfaceKind::Cli, request_id, "trace-query"),
        )
        .await
        .expect("query should succeed")
}
