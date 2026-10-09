use super::super::test_support::*;
use super::*;
use crate::ports::feedback::FeedbackProvider;

#[test]
fn rate_limit_headers_preserve_server_deadlines_and_safe_fallbacks() {
    for (status, pairs, expected) in [
        (403, vec![("retry-after", "120")], 130_000),
        (
            403,
            vec![("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "200")],
            200_000,
        ),
        (
            403,
            vec![
                ("retry-after", "120"),
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "200"),
            ],
            200_000,
        ),
        (403, vec![("x-ratelimit-remaining", "0")], 70_000),
        (403, vec![("retry-after", "invalid")], 70_000),
        (429, vec![], 70_000),
        (429, vec![("retry-after", "18446744073709551615")], u64::MAX),
        (
            429,
            vec![
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "18446744073709551615"),
            ],
            u64::MAX,
        ),
        (
            429,
            vec![("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "1")],
            10_000,
        ),
        (
            429,
            vec![
                ("retry-after", "-1"),
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "bad"),
            ],
            70_000,
        ),
    ] {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(name, value.parse().unwrap());
        }
        for creating in [false, true] {
            let error = classify_status(
                StatusCode::from_u16(status).unwrap(),
                &headers,
                creating,
                10_000,
            )
            .unwrap_err();
            assert_eq!(error.kind, FeedbackProviderErrorKind::Retryable);
            assert_eq!(error.retry_not_before_ms, Some(expected));
        }
    }
    let mut headers = HeaderMap::new();
    headers.insert("x-ratelimit-remaining", "10".parse().unwrap());
    headers.insert("x-ratelimit-reset", "200".parse().unwrap());
    let denied = classify_status(StatusCode::FORBIDDEN, &headers, true, 10_000).unwrap_err();
    assert_eq!(denied.kind, FeedbackProviderErrorKind::Rejected);
    assert_eq!(denied.retry_not_before_ms, None);
}

#[tokio::test]
async fn rate_limited_creation_keeps_deadline_without_repeating_post() {
    let response = MockResponse::Reply("HTTP/1.1 403 Forbidden\r\nRetry-After: 120\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
    let (provider, mut server) = mock_provider(vec![response], 1000).await;
    let before = crate::clock::system_now_millis().unwrap();
    let error = provider
        .create_issue("acme/project", "test", "body")
        .await
        .unwrap_err();
    assert_eq!(error.kind, FeedbackProviderErrorKind::Retryable);
    assert!(error.retry_not_before_ms.unwrap() >= before + 120_000);
    assert!(server.requests.recv().await.unwrap().starts_with("POST "));
    assert!(server.requests.recv().await.is_none());
    assert_eq!(
        provider
            .network
            .qos_runtime()
            .diagnostics_snapshot()
            .usage
            .in_flight_requests,
        0
    );
}

#[tokio::test]
async fn headerless_secondary_limits_are_retryable_but_permission_errors_are_rejected() {
    for (body, limited) in [
        (
            serde_json::json!({"message":"You have exceeded a secondary rate limit. Please wait."}),
            true,
        ),
        (
            serde_json::json!({"message":"API rate limit exceeded for account"}),
            true,
        ),
        (
            serde_json::json!({"message":"Resource not accessible by integration"}),
            false,
        ),
        (serde_json::json!({"message":null}), false),
    ] {
        let (provider, _server) = mock_provider(vec![json_response(403, body)], 1000).await;
        let before = crate::clock::system_now_millis().unwrap();
        let error = provider.read_issue("acme/project", 1).await.unwrap_err();
        if limited {
            assert_eq!(error.kind, FeedbackProviderErrorKind::Retryable);
            assert!(error.retry_not_before_ms.unwrap() >= before + 60_000);
        } else {
            assert_eq!(error.kind, FeedbackProviderErrorKind::Rejected);
            assert_eq!(error.retry_not_before_ms, None);
        }
        assert!(!error.message.contains("integration"));
    }
}

#[test]
fn status_classification_distinguishes_rejection_from_uncertain_creation() {
    for status in [400, 401, 403, 404, 410, 422] {
        assert_eq!(
            classify_status(
                StatusCode::from_u16(status).expect("status"),
                &HeaderMap::new(),
                true,
                1000
            )
            .expect_err("rejected")
            .kind,
            FeedbackProviderErrorKind::Rejected
        );
    }
    assert_eq!(
        classify_status(StatusCode::TOO_MANY_REQUESTS, &HeaderMap::new(), true, 1000)
            .expect_err("rate limited")
            .kind,
        FeedbackProviderErrorKind::Retryable
    );
    for status in [200, 202, 301, 307, 408, 500, 502, 503] {
        assert_eq!(
            classify_status(
                StatusCode::from_u16(status).expect("status"),
                &HeaderMap::new(),
                true,
                1000
            )
            .expect_err("uncertain")
            .kind,
            FeedbackProviderErrorKind::Ambiguous
        );
    }
    assert!(classify_status(StatusCode::CREATED, &HeaderMap::new(), true, 1000).is_ok());
    assert!(classify_status(StatusCode::OK, &HeaderMap::new(), false, 1000).is_ok());
    assert_eq!(
        classify_status(StatusCode::BAD_GATEWAY, &HeaderMap::new(), false, 1000)
            .expect_err("retry read")
            .kind,
        FeedbackProviderErrorKind::Retryable
    );
}

#[tokio::test]
async fn redirect_does_not_forward_credentials_or_create_another_request() {
    let response = MockResponse::Reply("HTTP/1.1 307 Redirect\r\nLocation: https://example.invalid/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned());
    let (provider, mut server) = mock_provider(vec![response], 1000).await;
    let error = provider
        .create_issue("acme/project", "test", "body")
        .await
        .expect_err("redirect blocked");
    assert_eq!(error.kind, FeedbackProviderErrorKind::Ambiguous);
    assert!(!error.message.contains("fixture-token"));
    assert!(
        server
            .requests
            .recv()
            .await
            .expect("one request")
            .contains("authorization: Bearer fixture-token")
    );
    assert!(server.requests.recv().await.is_none());
}

#[tokio::test]
async fn oversized_and_malformed_success_bodies_remain_ambiguous() {
    let oversized = MockResponse::Reply(format!(
        "HTTP/1.1 201 Created\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        MAX_RESPONSE_BYTES + 1
    ));
    let invalid = MockResponse::Reply(
        "HTTP/1.1 201 Created\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnope".to_owned(),
    );
    let (provider, _server) = mock_provider(vec![oversized, invalid], 1000).await;
    for _ in 0..2 {
        assert_eq!(
            provider
                .create_issue("acme/project", "test", "body")
                .await
                .expect_err("invalid response")
                .kind,
            FeedbackProviderErrorKind::Ambiguous
        );
    }
    assert_eq!(
        provider
            .network
            .qos_runtime()
            .diagnostics_snapshot()
            .usage
            .in_flight_requests,
        0
    );
}

#[tokio::test]
async fn chunked_body_is_bounded_without_content_length() {
    let body = "x".repeat(MAX_RESPONSE_BYTES + 1);
    let response = MockResponse::Reply(format!(
        "HTTP/1.1 201 Created\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n",
        body.len()
    ));
    let (provider, _server) = mock_provider(vec![response], 1000).await;
    let error = provider
        .create_issue("acme/project", "test", "body")
        .await
        .expect_err("stream bounded");
    assert_eq!(error.kind, FeedbackProviderErrorKind::Ambiguous);
    assert!(error.message.contains("byte budget"));
}

#[tokio::test]
async fn timeout_and_cancellation_release_qos_permits() {
    let (provider, _server) = mock_provider(vec![MockResponse::Hang], 20).await;
    assert_eq!(
        provider
            .create_issue("acme/project", "test", "body")
            .await
            .expect_err("timeout")
            .kind,
        FeedbackProviderErrorKind::Ambiguous
    );
    let metrics = provider.network.qos_runtime().diagnostics_snapshot();
    assert_eq!(metrics.usage.in_flight_requests, 0);
    assert_eq!(metrics.timed_out_total, 1);

    let (provider, mut server) = mock_provider(vec![MockResponse::Hang], 1000).await;
    let qos = provider.network.qos_runtime();
    let pending =
        tokio::spawn(async move { provider.create_issue("acme/project", "test", "body").await });
    server.requests.recv().await.expect("request started");
    pending.abort();
    assert!(pending.await.expect_err("cancelled").is_cancelled());
    let metrics = qos.diagnostics_snapshot();
    assert_eq!(metrics.usage.in_flight_requests, 0);
    assert_eq!(metrics.cancelled_total, 1);
}

#[tokio::test]
async fn qos_denial_is_retryable_before_network_io() {
    let (provider, mut server) = mock_provider(vec![], 1000).await;
    provider
        .network
        .refresh_from_overrides(&crate::env::NetworkEnvOverrides {
            qos_max_in_flight_requests: Some(1),
            ..Default::default()
        })
        .expect("bounded qos");
    let policy = provider.network.current().qos;
    let qos = provider.network.qos_runtime();
    let _permit = qos.admit_request(&policy).expect("occupy budget");
    assert_eq!(
        provider
            .create_issue("acme/project", "test", "body")
            .await
            .expect_err("admission denied")
            .kind,
        FeedbackProviderErrorKind::Retryable
    );
    assert!(server.requests.recv().await.is_none());
}
