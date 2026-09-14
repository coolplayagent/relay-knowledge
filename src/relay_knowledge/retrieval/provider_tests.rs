use super::*;

#[test]
fn embeddings_url_accepts_base_or_endpoint() {
    assert_eq!(
        embeddings_url("https://example.test"),
        "https://example.test/v1/embeddings"
    );
    assert_eq!(
        embeddings_url("https://example.test/v1"),
        "https://example.test/v1/embeddings"
    );
    assert_eq!(
        embeddings_url("https://example.test/v1/embeddings"),
        "https://example.test/v1/embeddings"
    );
    assert_eq!(
        embeddings_url("https://example.test/v4"),
        "https://example.test/v4/embeddings"
    );
    assert_eq!(
        embeddings_url("https://example.test/openai/v2"),
        "https://example.test/openai/v2/embeddings"
    );
    assert_eq!(
        embeddings_url("https://example.test/openai"),
        "https://example.test/openai/v1/embeddings"
    );
    assert_eq!(
        embeddings_url("https://example.test/openai/v4/?probe=true#fragment"),
        "https://example.test/openai/v4/embeddings"
    );
    assert_eq!(
        embeddings_url("https://example.test/v4/embeddings?probe=true"),
        "https://example.test/v4/embeddings"
    );
}

#[test]
fn rejects_embedding_dimension_mismatch() {
    let response = OpenAiEmbeddingResponse {
        data: vec![OpenAiEmbeddingData {
            embedding: vec![0.1, 0.2],
        }],
    };

    let error = parse_embedding_response(response, 1, 3).expect_err("dimension should fail");

    assert_eq!(error.code, "embedding_dimension_mismatch");
    assert_eq!(error.retry, ProviderRetryClass::Permanent);
}

#[test]
fn classifies_rate_limit_as_retryable() {
    let error = status_error(429, None);

    assert_eq!(error.retry, ProviderRetryClass::Retryable);
    assert_eq!(error.code, "rate_limited");
}

#[test]
fn classifies_provider_resource_limit_bodies_as_retryable() {
    let payment_required = status_error(402, None);
    let quota_forbidden = status_error(
            403,
            Some(
                r#"{"error":{"code":"insufficient_quota","message":"Insufficient balance or no resource package."}}"#
                    .to_owned(),
            ),
        );
    let invalid_request_quota = status_error(
        400,
        Some(r#"{"error":{"type":"resource_exhausted","message":"quota exceeded"}}"#.to_owned()),
    );
    let retry_after_resource_exhausted = status_error(
        503,
        Some(
            r#"{"error":{"status":"RESOURCE_EXHAUSTED","message":"rate limit exceeded"}}"#
                .to_owned(),
        ),
    );
    let top_level_message_quota = status_error(
        403,
        Some(
            r#"{"error":{"code":"invalid_request"},"message":"Quota exceeded for tenant"}"#
                .to_owned(),
        ),
    );
    let nested_detail_resource_exhausted = status_error(
        500,
        Some(
            r#"{"error":{"code":"provider_error"},"details":[{"reason":"Resource exhausted"}]}"#
                .to_owned(),
        ),
    );

    assert_eq!(payment_required.retry, ProviderRetryClass::Retryable);
    assert_eq!(payment_required.code, "rate_limited");
    assert_eq!(quota_forbidden.retry, ProviderRetryClass::Retryable);
    assert_eq!(quota_forbidden.code, "rate_limited");
    assert_eq!(invalid_request_quota.retry, ProviderRetryClass::Retryable);
    assert_eq!(invalid_request_quota.code, "rate_limited");
    assert_eq!(
        retry_after_resource_exhausted.retry,
        ProviderRetryClass::Retryable
    );
    assert_eq!(retry_after_resource_exhausted.code, "rate_limited");
    assert_eq!(top_level_message_quota.retry, ProviderRetryClass::Retryable);
    assert_eq!(top_level_message_quota.code, "rate_limited");
    assert_eq!(
        nested_detail_resource_exhausted.retry,
        ProviderRetryClass::Retryable
    );
    assert_eq!(nested_detail_resource_exhausted.code, "rate_limited");
}

#[test]
fn preserves_permanent_provider_errors_without_resource_limit_signals() {
    let auth_forbidden = status_error(
        403,
        Some(r#"{"error":{"code":"invalid_api_key","message":"Invalid API key"}}"#.to_owned()),
    );
    let invalid_request = status_error(
        400,
        Some(
            r#"{"error":{"code":"invalid_request","message":"quota field is not supported"}}"#
                .to_owned(),
        ),
    );
    let limit_key_without_limited_value = status_error(
        400,
        Some(r#"{"error":{"code":"invalid_request"},"rate_limit":false}"#.to_owned()),
    );

    assert_eq!(auth_forbidden.retry, ProviderRetryClass::Permanent);
    assert_eq!(auth_forbidden.code, "auth_invalid");
    assert_eq!(invalid_request.retry, ProviderRetryClass::Permanent);
    assert_eq!(invalid_request.code, "invalid_request");
    assert_eq!(
        limit_key_without_limited_value.retry,
        ProviderRetryClass::Permanent
    );
    assert_eq!(limit_key_without_limited_value.code, "invalid_request");
}

#[test]
fn classifies_provider_http_status_codes() {
    for (status, code, retry) in [
        (400, "invalid_request", ProviderRetryClass::Permanent),
        (401, "auth_invalid", ProviderRetryClass::Permanent),
        (403, "auth_invalid", ProviderRetryClass::Permanent),
        (
            404,
            "model_or_endpoint_not_found",
            ProviderRetryClass::Permanent,
        ),
        (408, "network_timeout", ProviderRetryClass::Retryable),
        (500, "provider_unavailable", ProviderRetryClass::Retryable),
        (418, "provider_http_error", ProviderRetryClass::Permanent),
    ] {
        let error = status_error(status, Some("x".repeat(300)));

        assert_eq!(error.code, code);
        assert_eq!(error.retry, retry);
        assert_eq!(error.message.len(), 240);
    }
}

#[tokio::test]
async fn openai_provider_posts_and_parses_embeddings() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("local addr should load");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("request should connect");
        let mut buffer = vec![0; 2048];
        let count = stream
            .readable()
            .await
            .and_then(|()| stream.try_read(&mut buffer));
        let request = String::from_utf8_lossy(&buffer[..count.expect("request should read")]);

        assert!(request.starts_with("POST /v1/embeddings HTTP/1.1"));
        assert!(request.contains("authorization: Bearer secret"));
        assert!(request.contains("\"model\":\"text-embedding-3-small\""));
        stream
            .writable()
            .await
            .expect("stream should become writable");
        stream
                .try_write(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 34\r\n\r\n{\"data\":[{\"embedding\":[0.1,0.2]}]}",
                )
                .expect("response should write");
    });
    let provider = OpenAiCompatibleEmbeddingProvider {
        config: remote_config(
            format!("http://{addr}/v1"),
            std::time::Duration::from_secs(5),
        ),
        client: reqwest::Client::new(),
        qos: QosRuntime::default(),
        policy: test_qos_policy(),
    };

    let vectors = provider
        .embed(EmbeddingRequest {
            inputs: vec!["probe".to_owned()],
            model: "text-embedding-3-small".to_owned(),
            dimension: 2,
        })
        .await
        .expect("provider response should parse");

    assert_eq!(vectors[0].values, [0.1, 0.2]);
    server.await.expect("server should finish");
}

#[tokio::test]
async fn echo_provider_returns_deterministic_vectors() {
    let provider = EchoEmbeddingProvider {
        config: remote_config("http://example.test/v1", std::time::Duration::from_secs(5)),
    };

    let vectors = provider
        .embed(EmbeddingRequest {
            inputs: vec!["abc".to_owned(), "abc".to_owned()],
            model: "echo".to_owned(),
            dimension: 4,
        })
        .await
        .expect("echo provider should embed");

    assert_eq!(vectors.len(), 2);
    assert_eq!(vectors[0], vectors[1]);
    assert_eq!(vectors[0].values.len(), 4);
}

#[test]
fn rejects_invalid_requests_and_response_values() {
    let empty = validate_request(&EmbeddingRequest {
        inputs: Vec::new(),
        model: "model".to_owned(),
        dimension: 1,
    })
    .expect_err("empty inputs should fail");
    let model = validate_request(&EmbeddingRequest {
        inputs: vec!["x".to_owned()],
        model: " ".to_owned(),
        dimension: 1,
    })
    .expect_err("blank model should fail");
    let dimension = validate_request(&EmbeddingRequest {
        inputs: vec!["x".to_owned()],
        model: "model".to_owned(),
        dimension: 0,
    })
    .expect_err("zero dimension should fail");
    let invalid_value = validate_vector(vec![f64::NAN], 1).expect_err("nan values should fail");

    assert_eq!(empty.code, "empty_embedding_batch");
    assert_eq!(model.code, "empty_embedding_model");
    assert_eq!(dimension.code, "invalid_dimension");
    assert_eq!(invalid_value.code, "invalid_embedding_value");
}

#[tokio::test]
async fn applies_configured_embedding_timeout() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("local addr should load");
    let server = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.expect("request should connect");
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    });
    let provider = OpenAiCompatibleEmbeddingProvider {
        config: RemoteEmbeddingConfig {
            provider: EmbeddingProviderKind::OpenAiCompatible,
            base_url: format!("http://{addr}/v1"),
            api_key: "secret".to_owned(),
            batch_size: 1,
            timeout: std::time::Duration::from_millis(20),
            max_concurrency: 1,
        },
        client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("client should build"),
        qos: QosRuntime::default(),
        policy: test_qos_policy(),
    };

    let error = provider
        .embed(EmbeddingRequest {
            inputs: vec!["probe".to_owned()],
            model: "text-embedding-3-small".to_owned(),
            dimension: 3,
        })
        .await
        .expect_err("provider request should use embedding timeout");

    assert_eq!(error.code, "network_timeout");
    server.abort();
}

#[tokio::test]
async fn rejects_remote_embedding_before_network_io_when_qos_is_exhausted() {
    let qos = QosRuntime::default();
    let policy = test_qos_policy();
    let _held = qos
        .admit_request(&policy)
        .expect("first request should consume the budget");
    let provider = OpenAiCompatibleEmbeddingProvider {
        config: remote_config("http://127.0.0.1:9/v1", std::time::Duration::from_secs(1)),
        client: reqwest::Client::new(),
        qos: qos.clone(),
        policy,
    };

    let error = provider
        .embed(EmbeddingRequest {
            inputs: vec!["probe".to_owned()],
            model: "model".to_owned(),
            dimension: 1,
        })
        .await
        .expect_err("exhausted QoS must reject before connecting");

    assert_eq!(error.code, "qos_rejected");
    assert_eq!(qos.diagnostics_snapshot().rejected_total, 1);
}

#[tokio::test]
async fn cancelled_remote_embedding_releases_qos_permit() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("local addr should load");
    let (accepted_sender, accepted_receiver) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.expect("request should connect");
        let _ = accepted_sender.send(());
        std::future::pending::<()>().await;
    });
    let qos = QosRuntime::default();
    let provider = OpenAiCompatibleEmbeddingProvider {
        config: remote_config(
            format!("http://{addr}/v1"),
            std::time::Duration::from_secs(5),
        ),
        client: reqwest::Client::new(),
        qos: qos.clone(),
        policy: test_qos_policy(),
    };
    let request = tokio::spawn(async move {
        provider
            .embed(EmbeddingRequest {
                inputs: vec!["probe".to_owned()],
                model: "model".to_owned(),
                dimension: 1,
            })
            .await
    });

    accepted_receiver
        .await
        .expect("server should observe the admitted request");
    request.abort();
    let _ = request.await;
    tokio::task::yield_now().await;

    let diagnostics = qos.diagnostics_snapshot();
    assert_eq!(diagnostics.cancelled_total, 1);
    assert_eq!(diagnostics.usage.in_flight_requests, 0);
    server.abort();
}

#[tokio::test]
#[allow(deprecated)]
async fn compatibility_constructor_refuses_remote_network_provider() {
    let provider = embedding_provider(
        remote_config("http://127.0.0.1:9/v1", std::time::Duration::from_secs(1)),
        reqwest::Client::new(),
    );

    let error = provider
        .embed(EmbeddingRequest {
            inputs: vec!["probe".to_owned()],
            model: "model".to_owned(),
            dimension: 1,
        })
        .await
        .expect_err("compatibility constructor must not bypass QoS");

    assert_eq!(error.code, "qos_required");
}

fn test_qos_policy() -> QosPolicy {
    QosPolicy::new(8, 1, 8).expect("test QoS policy should build")
}

fn remote_config(
    base_url: impl Into<String>,
    timeout: std::time::Duration,
) -> RemoteEmbeddingConfig {
    RemoteEmbeddingConfig {
        provider: EmbeddingProviderKind::OpenAiCompatible,
        base_url: base_url.into(),
        api_key: "secret".to_owned(),
        batch_size: 1,
        timeout,
        max_concurrency: 1,
    }
}
